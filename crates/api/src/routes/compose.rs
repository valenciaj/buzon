//! Redactar y enviar: identidades, borradores, adjuntos subidos y envío por SMTP.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use buzon_store::compose::{Draft, Identity, MessageRef};
use buzon_store::mail::Addr;
use buzon_store::user::user_raw_dir;
use buzon_store::{now, Id, UserDb};
use buzon_sync::send::{self, OutAttachment, Outgoing, SmtpConfig};
use buzon_sync::Security;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::error::{ApiError, ApiResult, ErrorBody};
use crate::models::{ContactOut, DraftIn, DraftOut, IdentityInput, IdentityOut, SendRequest, SendResult, UploadOut};
use crate::session::CurrentUser;
use crate::AppState;

/// Tamaño máximo de cada adjunto subido.
pub const MAX_UPLOAD: usize = 25 * 1024 * 1024;

fn parse_id(s: &str, what: &str) -> ApiResult<Id> {
    s.parse().map_err(|_| ApiError::not_found(format!("{what} no existe.")))
}

fn uploads_dir(st: &AppState, user: Id) -> PathBuf {
    st.data_dir.join("users").join(user.to_base58()).join("uploads")
}

pub(crate) fn valid_email(e: &str) -> bool {
    let e = e.trim();
    match e.split_once('@') {
        Some((local, domain)) => !local.is_empty() && domain.contains('.') && !e.contains(char::is_whitespace),
        None => false,
    }
}

// ---------- Identidades ----------

#[utoipa::path(get, path = "/api/v1/identities", tag = "compose",
    responses((status = 200, body = [IdentityOut]), (status = 401, body = ErrorBody)))]
pub async fn identities(State(st): State<Arc<AppState>>, cu: CurrentUser) -> ApiResult<Json<Vec<IdentityOut>>> {
    let udb = st.user_db(cu.user.id).await?;
    Ok(Json(udb.identities(None).await?.into_iter().map(Into::into).collect()))
}

async fn save_identity(udb: &UserDb, id: Id, account: Id, req: IdentityInput) -> ApiResult<IdentityOut> {
    if !valid_email(&req.email) {
        return Err(ApiError::bad_request("La dirección de correo no es válida."));
    }
    let i = Identity {
        id,
        account_id: account,
        name: req.name.trim().to_string(),
        email: req.email.trim().to_string(),
        signature: req.signature.trim_end().to_string(),
        is_default: req.is_default,
    };
    udb.save_identity(&i).await?;
    let saved = udb
        .identity(id)
        .await?
        .ok_or_else(|| ApiError::not_found("La identidad no existe."))?;
    Ok(saved.into())
}

/// Añade un alias (otra dirección desde la que enviar) a una cuenta.
#[utoipa::path(post, path = "/api/v1/identities", tag = "compose", request_body = IdentityInput,
    responses((status = 200, body = IdentityOut), (status = 400, body = ErrorBody)))]
pub async fn create_identity(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<IdentityInput>,
) -> ApiResult<Json<IdentityOut>> {
    let udb = st.user_db(cu.user.id).await?;
    let account = req
        .account_id
        .ok_or_else(|| ApiError::bad_request("Indica la cuenta (account_id)."))?;
    if udb.account(account).await?.is_none() {
        return Err(ApiError::not_found("Esa cuenta no existe."));
    }
    udb.identities(Some(account)).await?;
    Ok(Json(save_identity(&udb, Id::new(), account, req).await?))
}

#[utoipa::path(put, path = "/api/v1/identities/{id}", tag = "compose", request_body = IdentityInput,
    params(("id" = String, Path, description = "Id de la identidad")),
    responses((status = 200, body = IdentityOut), (status = 404, body = ErrorBody)))]
pub async fn update_identity(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<IdentityInput>,
) -> ApiResult<Json<IdentityOut>> {
    let id = parse_id(&id, "Esa identidad")?;
    let udb = st.user_db(cu.user.id).await?;
    let current = udb
        .identity(id)
        .await?
        .ok_or_else(|| ApiError::not_found("Esa identidad no existe."))?;
    // Siempre tiene que quedar una predeterminada.
    let req = IdentityInput {
        is_default: req.is_default || current.is_default,
        ..req
    };
    Ok(Json(save_identity(&udb, id, current.account_id, req).await?))
}

#[utoipa::path(delete, path = "/api/v1/identities/{id}", tag = "compose",
    params(("id" = String, Path, description = "Id de la identidad")),
    responses((status = 204), (status = 409, description = "Es la única de la cuenta", body = ErrorBody)))]
pub async fn delete_identity(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let id = parse_id(&id, "Esa identidad")?;
    let udb = st.user_db(cu.user.id).await?;
    if udb.identity(id).await?.is_none() {
        return Err(ApiError::not_found("Esa identidad no existe."));
    }
    if !udb.delete_identity(id).await? {
        return Err(ApiError::conflict(
            "last_identity",
            "Cada cuenta necesita al menos una identidad.",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Adjuntos subidos ----------

/// Sube un fichero para adjuntarlo. Envía el contenido tal cual en el cuerpo, con
/// `Content-Type` del fichero y su nombre en `X-Buzon-Filename` (codificado como URL).
#[utoipa::path(post, path = "/api/v1/uploads", tag = "compose",
    request_body(content = Vec<u8>, content_type = "application/octet-stream"),
    params(("X-Buzon-Filename" = String, Header, description = "Nombre del fichero, codificado como URL")),
    responses((status = 200, body = UploadOut), (status = 413, description = "Más de 25 MB", body = ErrorBody)))]
pub async fn upload(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<UploadOut>> {
    let name = headers
        .get("x-buzon-filename")
        .and_then(|v| v.to_str().ok())
        .map(percent_decode)
        .map(|n| n.replace(['/', '\\', '\0', '\r', '\n'], "_"))
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "adjunto".into());
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty() && v.len() < 200 && v.contains('/'))
        .unwrap_or("application/octet-stream")
        .to_string();
    let udb = st.user_db(cu.user.id).await?;
    let up = udb.add_upload(name.trim(), &content_type, body.len() as u64).await?;
    let dir = uploads_dir(&st, cu.user.id);
    tokio::fs::create_dir_all(&dir).await.map_err(buzon_store::Error::Io)?;
    tokio::fs::write(dir.join(up.id.to_base58()), &body)
        .await
        .map_err(buzon_store::Error::Io)?;
    Ok(Json(up.into()))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[utoipa::path(delete, path = "/api/v1/uploads/{id}", tag = "compose",
    params(("id" = String, Path, description = "Id del fichero subido")),
    responses((status = 204)))]
pub async fn delete_upload(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let id = parse_id(&id, "Ese fichero")?;
    let udb = st.user_db(cu.user.id).await?;
    udb.delete_upload(id).await?;
    let _ = tokio::fs::remove_file(uploads_dir(&st, cu.user.id).join(id.to_base58())).await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Borradores ----------

fn draft_from(req: DraftIn, id: Option<Id>) -> Draft {
    Draft {
        id,
        identity_id: req.identity_id,
        to: req.to.into_iter().map(Into::into).collect(),
        cc: req.cc.into_iter().map(Into::into).collect(),
        bcc: req.bcc.into_iter().map(Into::into).collect(),
        subject: req.subject,
        html: req.html,
        text: req.text,
        reply_to: req.reply_to,
        forward_of: req.forward_of,
        attachments: req.attachments,
        ..Default::default()
    }
}

/// Un borrador guardado, como si llegara por la API (para enviarlo).
pub(crate) fn draft_in(d: Draft) -> DraftIn {
    DraftIn {
        identity_id: d.identity_id,
        to: d.to.into_iter().map(Into::into).collect(),
        cc: d.cc.into_iter().map(Into::into).collect(),
        bcc: d.bcc.into_iter().map(Into::into).collect(),
        subject: d.subject,
        html: d.html,
        text: d.text,
        reply_to: d.reply_to,
        forward_of: d.forward_of,
        attachments: d.attachments,
    }
}

pub(crate) async fn draft_out(udb: &UserDb, d: Draft) -> ApiResult<DraftOut> {
    let mut attachments = Vec::new();
    for a in &d.attachments {
        if let Some(u) = udb.upload(*a).await? {
            attachments.push(u.into());
        }
    }
    Ok(DraftOut {
        id: d.id.expect("guardado"),
        identity_id: d.identity_id,
        to: d.to.into_iter().map(Into::into).collect(),
        cc: d.cc.into_iter().map(Into::into).collect(),
        bcc: d.bcc.into_iter().map(Into::into).collect(),
        subject: d.subject,
        html: d.html,
        text: d.text,
        reply_to: d.reply_to,
        forward_of: d.forward_of,
        attachments,
        updated_at: d.updated_at,
        send_at: d.send_at,
        send_error: d.send_error,
        follow_up_days: d.follow_up_days,
    })
}

#[utoipa::path(get, path = "/api/v1/drafts", tag = "compose",
    responses((status = 200, body = [DraftOut])))]
pub async fn drafts(State(st): State<Arc<AppState>>, cu: CurrentUser) -> ApiResult<Json<Vec<DraftOut>>> {
    let udb = st.user_db(cu.user.id).await?;
    let mut out = Vec::new();
    for d in udb.drafts().await? {
        out.push(draft_out(&udb, d).await?);
    }
    Ok(Json(out))
}

#[utoipa::path(post, path = "/api/v1/drafts", tag = "compose", request_body = DraftIn,
    responses((status = 200, body = DraftOut)))]
pub async fn create_draft(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<DraftIn>,
) -> ApiResult<Json<DraftOut>> {
    let udb = st.user_db(cu.user.id).await?;
    let d = udb.save_draft(&draft_from(req, None)).await?;
    upload_draft_later(&st, cu.user.id, d.id.expect("guardado"));
    Ok(Json(draft_out(&udb, d).await?))
}

#[utoipa::path(put, path = "/api/v1/drafts/{id}", tag = "compose", request_body = DraftIn,
    params(("id" = String, Path, description = "Id del borrador")),
    responses((status = 200, body = DraftOut), (status = 404, body = ErrorBody)))]
pub async fn update_draft(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<DraftIn>,
) -> ApiResult<Json<DraftOut>> {
    let id = parse_id(&id, "Ese borrador")?;
    let udb = st.user_db(cu.user.id).await?;
    let old = udb
        .draft(id)
        .await?
        .ok_or_else(|| ApiError::not_found("Ese borrador no existe."))?;
    let d = udb.save_draft(&draft_from(req, Some(id))).await?;
    // Si cambia de cuenta, la copia de la anterior sobra.
    if st.sync.enabled() {
        if let Some((account, mailbox)) = drafts_folder(&udb, &old).await {
            if drafts_folder(&udb, &d).await.map(|(a, _)| a) != Some(account) {
                st.sync.put_draft(account, &mailbox, &id.to_base58(), None);
            }
        }
    }
    upload_draft_later(&st, cu.user.id, id);
    Ok(Json(draft_out(&udb, d).await?))
}

/// Descarta un borrador y sus adjuntos subidos.
#[utoipa::path(delete, path = "/api/v1/drafts/{id}", tag = "compose",
    params(("id" = String, Path, description = "Id del borrador")),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn delete_draft(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let id = parse_id(&id, "Ese borrador")?;
    let udb = st.user_db(cu.user.id).await?;
    let d = udb
        .draft(id)
        .await?
        .ok_or_else(|| ApiError::not_found("Ese borrador no existe."))?;
    forget_draft_copy(&st, &udb, id).await;
    remove_uploads(&st, cu.user.id, &udb, &d.attachments).await?;
    udb.delete_draft(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_uploads(st: &AppState, user: Id, udb: &UserDb, ids: &[Id]) -> ApiResult<()> {
    for a in ids {
        udb.delete_upload(*a).await?;
        let _ = tokio::fs::remove_file(uploads_dir(st, user).join(a.to_base58())).await;
    }
    Ok(())
}

// ---------- Enviar ----------

/// Envía un correo por el SMTP de la cuenta de la identidad elegida, guarda la copia en
/// Enviados (si el servidor no lo hace solo) y marca como respondido el original.
#[utoipa::path(post, path = "/api/v1/send", tag = "compose", request_body = SendRequest,
    responses((status = 200, body = SendResult), (status = 400, body = ErrorBody),
        (status = 502, description = "El servidor SMTP no aceptó el envío", body = ErrorBody)))]
pub async fn send_mail(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<SendRequest>,
) -> ApiResult<Json<SendResult>> {
    let user = cu.user.id;
    if req.follow_up_days.is_some_and(|d| !(1..=30).contains(&d)) {
        return Err(ApiError::bad_request(
            "El aviso de seguimiento debe ser de 1 a 30 días.",
        ));
    }
    match req.send_at {
        Some(at) if at > now() => {
            if at > now() + MAX_SCHEDULE {
                return Err(ApiError::bad_request("Solo se puede programar hasta un año vista."));
            }
            let udb = st.user_db(user).await?;
            check_message(&udb, &req.message).await?;
            let d = udb.save_draft(&draft_from(req.message, req.draft_id)).await?;
            let id = d.id.expect("guardado");
            if req.draft_id.is_some() {
                forget_draft_copy(&st, &udb, id).await;
            }
            udb.schedule_draft(id, Some(at), req.follow_up_days, None).await?;
            st.sends_ready.notify_one();
            Ok(Json(SendResult {
                message_id: None,
                scheduled_id: Some(id),
                send_at: Some(at),
            }))
        }
        _ => {
            let message_id = deliver(&st, user, req.message, req.draft_id, req.follow_up_days).await?;
            Ok(Json(SendResult {
                message_id: Some(message_id),
                scheduled_id: None,
                send_at: None,
            }))
        }
    }
}

/// Lo más lejos que se puede programar un envío.
const MAX_SCHEDULE: i64 = 366 * 24 * 3600;

async fn identity_for(udb: &UserDb, m: &DraftIn) -> ApiResult<Identity> {
    match m.identity_id {
        Some(id) => udb.identity(id).await?,
        None => udb.identities(None).await?.into_iter().find(|i| i.is_default),
    }
    .ok_or_else(|| ApiError::bad_request("Elige desde qué dirección envías."))
}

fn check_recipients<'a>(all: impl Iterator<Item = &'a str>) -> ApiResult<()> {
    let mut any = false;
    for email in all {
        any = true;
        if !valid_email(email) {
            return Err(ApiError::bad_request(format!(
                "La dirección «{}» no es válida.",
                email.trim()
            )));
        }
    }
    if !any {
        return Err(ApiError::bad_request("Añade al menos un destinatario."));
    }
    Ok(())
}

/// Lo que se comprueba antes de programar un envío (lo demás, al enviarlo).
async fn check_message(udb: &UserDb, m: &DraftIn) -> ApiResult<()> {
    identity_for(udb, m).await?;
    check_recipients(m.to.iter().chain(&m.cc).chain(&m.bcc).map(|a| a.email.as_str()))
}

/// Envía ya un correo. Lo usan `POST /send` y los envíos programados.
pub(crate) async fn deliver(
    st: &AppState,
    user: Id,
    m: DraftIn,
    draft_id: Option<Id>,
    follow_up_days: Option<i64>,
) -> ApiResult<String> {
    let udb = st.user_db(user).await?;
    let identity = identity_for(&udb, &m).await?;
    let account = udb
        .account(identity.account_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Esa cuenta no existe."))?;

    check_recipients(m.to.iter().chain(&m.cc).chain(&m.bcc).map(|a| a.email.as_str()))?;
    let attachment_ids = m.attachments.clone();
    let (outgoing, replied) = outgoing(st, user, &udb, &identity, m, true).await?;
    let recipients: Vec<Addr> = outgoing
        .to
        .iter()
        .chain(&outgoing.cc)
        .chain(&outgoing.bcc)
        .cloned()
        .collect();
    let hook = serde_json::json!({
        "account": account.id,
        "from": identity.email,
        "to": outgoing.to,
        "cc": outgoing.cc,
        "subject": outgoing.subject,
        "in_reply_to": replied.as_ref().map(|(id, _)| *id),
    });
    let (message, message_id) = send::build(&outgoing).map_err(|e| ApiError::bad_request(e.to_string()))?;

    let creds = st.creds(user, &account).await?;
    let auth = st
        .sync
        .auth(&udb, account.id, &creds)
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, "smtp_failed", format!("{e}.")))?;
    let cfg = SmtpConfig {
        host: account.smtp_host.clone(),
        port: account.smtp_port,
        security: Security::parse(&account.smtp_security).unwrap_or(Security::Tls),
        login: account.login.clone(),
        auth,
    };
    if let Err(e) = send::send(&cfg, &message).await {
        return Err(ApiError::new(StatusCode::BAD_GATEWAY, "smtp_failed", format!("{e}.")));
    }

    // A partir de aquí el correo ya ha salido: lo demás no debe fallar el envío.
    if !send::server_saves_sent(&account.smtp_host) {
        if let Ok(Some(sent)) = udb.mailbox_name_by_role(account.id, "sent").await {
            st.sync.append(account.id, &sent, message.formatted());
        }
    }
    if let Some((id, r)) = &replied {
        let _ = udb.set_answered(*id).await;
        // Contestado: sale de «Responder luego».
        let _ = udb.replied(*id).await;
        st.sync.store_flag(
            &[buzon_store::mail::ImapTarget {
                account_id: r.account_id,
                mailbox: r.mailbox.clone(),
                uid: r.uid,
            }],
            "\\Answered",
            true,
        );
    }
    if let Some(d) = draft_id {
        forget_draft_copy(st, &udb, d).await;
        let _ = udb.delete_draft(d).await;
    }
    if let Some(days) = follow_up_days {
        let who: Vec<Addr> = hook["to"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(hook["cc"].as_array().into_iter().flatten())
            .filter_map(|a| serde_json::from_value(a.clone()).ok())
            .collect();
        let subject = hook["subject"].as_str().unwrap_or_default().to_string();
        let _ = udb.add_followup(account.id, &message_id, &subject, &who, days).await;
    }
    // A quien escribes pasa a tu libreta.
    let _ = udb.remember_recipients(&recipients).await;
    let _ = remove_uploads(st, user, &udb, &attachment_ids).await;
    st.sync.emit(user, account.id);
    let _ = st.db.audit(Some(user), "mail.sent", None).await;
    let mut hook = hook;
    hook["message_id"] = serde_json::json!(message_id);
    st.emit_hook(user, "message.sent", hook).await;
    Ok(message_id)
}

/// El mensaje MIME de un correo a punto de salir o de un borrador. Con `strict`, falla si
/// falta un adjunto; si no (borradores), se salta lo que no se pueda incluir.
async fn outgoing(
    st: &AppState,
    user: Id,
    udb: &UserDb,
    identity: &Identity,
    m: DraftIn,
    strict: bool,
) -> ApiResult<(Outgoing, Option<(Id, MessageRef)>)> {
    let addrs = |v: Vec<crate::models::AddrIn>| -> Vec<Addr> {
        v.into_iter()
            .map(Into::into)
            .filter(|a: &Addr| strict || valid_email(&a.email))
            .collect()
    };
    let (to, cc, bcc) = (addrs(m.to), addrs(m.cc), addrs(m.bcc));

    // Respuesta: cabeceras para que el destinatario lo vea en el mismo hilo.
    let replied = match m.reply_to {
        Some(id) => udb.message_ref(id).await?.map(|r| (id, r)),
        None => None,
    };
    let (in_reply_to, references) = match &replied {
        Some((_, r)) => {
            let mut refs = r.refs.clone();
            refs.extend(r.message_id.clone());
            (r.message_id.clone(), refs)
        }
        None => (None, Vec::new()),
    };

    // Adjuntos: los subidos y, al reenviar, los del original.
    let mut attachments = Vec::new();
    let missing = || ApiError::bad_request("Un adjunto ya no existe. Vuelve a añadirlo.");
    for a in &m.attachments {
        let up = match udb.upload(*a).await? {
            Some(up) => up,
            None if strict => return Err(missing()),
            None => continue,
        };
        let data = match tokio::fs::read(uploads_dir(st, user).join(a.to_base58())).await {
            Ok(d) => d,
            Err(_) if strict => return Err(missing()),
            Err(_) => continue,
        };
        attachments.push(OutAttachment {
            name: up.name,
            content_type: up.content_type,
            data,
        });
    }
    if let Some(fwd) = m.forward_of {
        if let Some((_, list)) = udb.attachments(fwd).await? {
            let raw = tokio::fs::read(user_raw_dir(&st.data_dir, user).join(format!("{fwd}.eml")))
                .await
                .unwrap_or_default();
            for a in list.into_iter().filter(|a| !a.inline) {
                if let Some(data) = buzon_sync::parse::attachment(&raw, a.index) {
                    attachments.push(OutAttachment {
                        name: a.name,
                        content_type: a.content_type,
                        data,
                    });
                }
            }
        }
    }

    let html = Some(m.html.trim())
        .filter(|h| !h.is_empty())
        .map(|h| buzon_sync::parse::sanitize_html(h, &HashMap::new()).html);
    let o = Outgoing {
        from: Addr {
            name: Some(identity.name.clone()).filter(|n| !n.is_empty()),
            email: identity.email.clone(),
        },
        to,
        cc,
        bcc,
        subject: m.subject,
        text: m.text,
        html,
        in_reply_to,
        references,
        attachments,
    };
    Ok((o, replied))
}

// ---------- Copia en la carpeta Borradores del servidor ----------

/// Espera antes de subir un borrador: la web lo guarda cada poco mientras escribes.
const DRAFT_UPLOAD_DELAY: std::time::Duration = std::time::Duration::from_secs(10);

/// Apunta una versión nueva del borrador; las subidas anteriores pendientes ya no valen.
fn next_draft_version(st: &AppState, id: Id) -> u64 {
    let v = st.draft_seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    st.draft_copies.lock().expect("mutex").insert(id, v);
    v
}

fn is_current_draft_version(st: &AppState, id: Id, v: u64) -> bool {
    st.draft_copies.lock().expect("mutex").get(&id) == Some(&v)
}

/// Dónde va la copia del borrador: la cuenta de su identidad y su carpeta Borradores.
async fn drafts_folder(udb: &UserDb, d: &Draft) -> Option<(Id, String)> {
    let m = draft_in(d.clone());
    let identity = identity_for(udb, &m).await.ok()?;
    let mailbox = udb.mailbox_name_by_role(identity.account_id, "drafts").await.ok()??;
    Some((identity.account_id, mailbox))
}

/// Sube el borrador a la carpeta Borradores del servidor, para verlo también desde
/// otros programas. Espera un poco por si sigue cambiando.
pub(crate) fn upload_draft_later(st: &Arc<AppState>, user: Id, id: Id) {
    if !st.sync.enabled() {
        return;
    }
    let v = next_draft_version(st, id);
    let st = st.clone();
    tokio::spawn(async move {
        tokio::time::sleep(DRAFT_UPLOAD_DELAY).await;
        if !is_current_draft_version(&st, id, v) {
            return;
        }
        if let Err(e) = upload_draft(&st, user, id, v).await {
            tracing::info!("borrador {id}: no se pudo subir a Borradores: {}", e.message);
        }
        let mut copies = st.draft_copies.lock().expect("mutex");
        if copies.get(&id) == Some(&v) {
            copies.remove(&id);
        }
    });
}

async fn upload_draft(st: &AppState, user: Id, id: Id, v: u64) -> ApiResult<()> {
    let udb = st.user_db(user).await?;
    // Los programados no son borradores: van a salir solos.
    let Some(d) = udb.draft(id).await?.filter(|d| d.send_at.is_none()) else {
        return Ok(());
    };
    let Some((account, mailbox)) = drafts_folder(&udb, &d).await else {
        return Ok(());
    };
    let m = draft_in(d);
    let identity = identity_for(&udb, &m).await?;
    let (o, _) = outgoing(st, user, &udb, &identity, m, false).await?;
    let raw = send::build_draft(&o, &id.to_base58()).map_err(|e| ApiError::bad_request(e.to_string()))?;
    // Si mientras tanto se ha enviado o descartado, ya no se sube.
    if is_current_draft_version(st, id, v) {
        st.sync.put_draft(account, &mailbox, &id.to_base58(), Some(raw));
    }
    Ok(())
}

/// Quita la copia del borrador de la carpeta Borradores (al enviarlo, programarlo o descartarlo).
pub(crate) async fn forget_draft_copy(st: &AppState, udb: &UserDb, id: Id) {
    if !st.sync.enabled() {
        return;
    }
    next_draft_version(st, id);
    if let Ok(Some(d)) = udb.draft(id).await {
        if let Some((account, mailbox)) = drafts_folder(udb, &d).await {
            st.sync.put_draft(account, &mailbox, &id.to_base58(), None);
        }
    }
    st.draft_copies.lock().expect("mutex").remove(&id);
}

// ---------- Contactos ----------

#[derive(Deserialize, IntoParams)]
pub struct ContactParams {
    /// Texto a buscar en el nombre o la dirección.
    pub q: Option<String>,
}

/// Direcciones conocidas para autocompletar destinatarios.
#[utoipa::path(get, path = "/api/v1/contacts/suggest", tag = "compose", params(ContactParams),
    responses((status = 200, body = [ContactOut])))]
pub async fn contacts(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Query(p): Query<ContactParams>,
) -> ApiResult<Json<Vec<ContactOut>>> {
    let udb = st.user_db(cu.user.id).await?;
    let q = p.q.unwrap_or_default();
    Ok(Json(
        udb.contacts(q.trim(), 8)
            .await?
            .into_iter()
            .map(|c| ContactOut {
                name: c.name,
                email: c.email,
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(percent_decode("men%C3%BA%20del%20d%C3%ADa.pdf"), "menú del día.pdf");
        assert_eq!(percent_decode("100%"), "100%");
        assert!(valid_email("ana@ejemplo.es"));
        assert!(!valid_email("ana@ejemplo"));
        assert!(!valid_email("a na@ejemplo.es"));
    }
}

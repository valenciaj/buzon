//! Redactar y enviar: identidades, borradores, adjuntos subidos y envío por SMTP.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use buzon_store::compose::{Draft, Identity};
use buzon_store::mail::Addr;
use buzon_store::user::user_raw_dir;
use buzon_store::{Id, UserDb};
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

fn valid_email(e: &str) -> bool {
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
        updated_at: 0,
    }
}

async fn draft_out(udb: &UserDb, d: Draft) -> ApiResult<DraftOut> {
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
    if udb.draft(id).await?.is_none() {
        return Err(ApiError::not_found("Ese borrador no existe."));
    }
    let d = udb.save_draft(&draft_from(req, Some(id))).await?;
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
    let udb = st.user_db(user).await?;
    let m = req.message;

    let identity = match m.identity_id {
        Some(id) => udb.identity(id).await?,
        None => udb.identities(None).await?.into_iter().find(|i| i.is_default),
    }
    .ok_or_else(|| ApiError::bad_request("Elige desde qué dirección envías."))?;
    let account = udb
        .account(identity.account_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Esa cuenta no existe."))?;

    let to: Vec<Addr> = m.to.into_iter().map(Into::into).collect();
    let cc: Vec<Addr> = m.cc.into_iter().map(Into::into).collect();
    let bcc: Vec<Addr> = m.bcc.into_iter().map(Into::into).collect();
    if to.is_empty() && cc.is_empty() && bcc.is_empty() {
        return Err(ApiError::bad_request("Añade al menos un destinatario."));
    }
    if let Some(bad) = to.iter().chain(&cc).chain(&bcc).find(|a| !valid_email(&a.email)) {
        return Err(ApiError::bad_request(format!(
            "La dirección «{}» no es válida.",
            bad.email
        )));
    }

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
    for a in &m.attachments {
        let up = udb
            .upload(*a)
            .await?
            .ok_or_else(|| ApiError::bad_request("Un adjunto ya no existe. Vuelve a añadirlo."))?;
        let data = tokio::fs::read(uploads_dir(&st, user).join(a.to_base58()))
            .await
            .map_err(|_| ApiError::bad_request("Un adjunto ya no existe. Vuelve a añadirlo."))?;
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
    let outgoing = Outgoing {
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
    let (message, message_id) = send::build(&outgoing).map_err(|e| ApiError::bad_request(e.to_string()))?;

    let password = udb
        .account_password(account.id, &st.secrets_for(user))
        .await?
        .unwrap_or_default();
    let cfg = SmtpConfig {
        host: account.smtp_host.clone(),
        port: account.smtp_port,
        security: Security::parse(&account.smtp_security).unwrap_or(Security::Tls),
        login: account.login.clone(),
        password,
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
    if let Some(d) = req.draft_id {
        let _ = udb.delete_draft(d).await;
    }
    let _ = remove_uploads(&st, user, &udb, &m.attachments).await;
    st.sync.emit(user, account.id);
    let _ = st.db.audit(Some(user), "mail.sent", None).await;
    Ok(Json(SendResult { message_id }))
}

// ---------- Contactos ----------

#[derive(Deserialize, IntoParams)]
pub struct ContactParams {
    /// Texto a buscar en el nombre o la dirección.
    pub q: Option<String>,
}

/// Direcciones conocidas para autocompletar destinatarios.
#[utoipa::path(get, path = "/api/v1/contacts", tag = "compose", params(ContactParams),
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

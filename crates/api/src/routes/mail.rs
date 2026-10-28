//! Lectura del correo: carpetas, hilos, mensajes, adjuntos y avisos en vivo.

use std::convert::Infallible;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderName, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use buzon_store::mail::{decode_mailbox_name, ThreadQuery};
use buzon_store::user::user_raw_dir;
use buzon_store::Id;
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use utoipa::IntoParams;

use crate::error::{ApiError, ApiResult, ErrorBody};
use crate::models::{AttachmentOut, MailboxOut, MessageOut, ThreadDetail, ThreadOut, ThreadPage, ThreadUpdate};
use crate::session::CurrentUser;
use crate::AppState;

const ROLES: [&str; 6] = ["inbox", "sent", "drafts", "archive", "junk", "trash"];

fn parse_id(s: &str) -> ApiResult<Id> {
    s.parse().map_err(|_| ApiError::not_found("No existe."))
}

#[derive(Deserialize, IntoParams)]
pub struct MailboxParams {
    /// Solo las carpetas de esta cuenta.
    pub account: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/mailboxes", tag = "mail", params(MailboxParams),
    responses((status = 200, body = [MailboxOut]), (status = 401, body = ErrorBody)))]
pub async fn mailboxes(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Query(p): Query<MailboxParams>,
) -> ApiResult<Json<Vec<MailboxOut>>> {
    let account = p.account.as_deref().map(parse_id).transpose()?;
    let udb = st.user_db(cu.user.id).await?;
    let out = udb
        .mailboxes(account)
        .await?
        .into_iter()
        .map(|c| MailboxOut {
            id: c.mailbox.id,
            account_id: c.mailbox.account_id,
            name: decode_mailbox_name(&c.mailbox.name),
            delimiter: c.mailbox.delimiter,
            role: c.mailbox.role,
            total: c.total,
            unread: c.unread,
        })
        .collect();
    Ok(Json(out))
}

#[derive(Deserialize, IntoParams)]
pub struct ThreadParams {
    /// Solo esta cuenta. Sin ella, todas las cuentas combinadas.
    pub account: Option<String>,
    /// Una carpeta concreta.
    pub mailbox: Option<String>,
    /// Un tipo de carpeta en todas las cuentas: `inbox`, `sent`, `drafts`, `archive`, `junk` o `trash`.
    pub role: Option<String>,
    /// `next_cursor` de la página anterior.
    pub cursor: Option<String>,
    /// Hilos por página (1 a 100; 50 por defecto).
    pub limit: Option<u32>,
}

fn parse_cursor(c: &str) -> ApiResult<(i64, Id)> {
    let bad = || ApiError::bad_request("El cursor no es válido.");
    let (date, id) = c.split_once('.').ok_or_else(bad)?;
    Ok((date.parse().map_err(|_| bad())?, id.parse().map_err(|_| bad())?))
}

/// Hilos del más reciente al más antiguo. Indica `mailbox` o `role`; sin ninguno, todo.
#[utoipa::path(get, path = "/api/v1/threads", tag = "mail", params(ThreadParams),
    responses((status = 200, body = ThreadPage), (status = 400, body = ErrorBody)))]
pub async fn threads(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Query(p): Query<ThreadParams>,
) -> ApiResult<Json<ThreadPage>> {
    if let Some(r) = &p.role {
        if !ROLES.contains(&r.as_str()) {
            return Err(ApiError::bad_request("Ese tipo de carpeta no existe."));
        }
    }
    let limit = p.limit.unwrap_or(50).clamp(1, 100);
    let q = ThreadQuery {
        account: p.account.as_deref().map(parse_id).transpose()?,
        mailbox: p.mailbox.as_deref().map(parse_id).transpose()?,
        role: p.role,
        before: p.cursor.as_deref().map(parse_cursor).transpose()?,
        limit,
    };
    let udb = st.user_db(cu.user.id).await?;
    let threads = udb.threads(&q).await?;
    let next_cursor = (threads.len() == limit as usize)
        .then(|| threads.last().map(|t| format!("{}.{}", t.last_date, t.id)))
        .flatten();
    Ok(Json(ThreadPage {
        threads: threads.into_iter().map(ThreadOut::from).collect(),
        next_cursor,
    }))
}

#[utoipa::path(get, path = "/api/v1/threads/{id}", tag = "mail",
    params(("id" = String, Path, description = "Id del hilo")),
    responses((status = 200, body = ThreadDetail), (status = 404, body = ErrorBody)))]
pub async fn thread(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
) -> ApiResult<Json<ThreadDetail>> {
    let udb = st.user_db(cu.user.id).await?;
    let t = udb
        .thread(parse_id(&id)?)
        .await?
        .ok_or_else(|| ApiError::not_found("Ese hilo no existe."))?;
    let messages = t
        .messages
        .into_iter()
        .map(|m| MessageOut {
            id: m.id,
            mailbox_id: m.mailbox_id,
            subject: m.subject,
            from: m.from.map(Into::into),
            to: m.to.into_iter().map(Into::into).collect(),
            cc: m.cc.into_iter().map(Into::into).collect(),
            date: m.date,
            seen: m.flags.seen,
            flagged: m.flags.flagged,
            answered: m.flags.answered,
            text: m.text,
            html: m.html,
            remote_images: m.remote_images,
            attachments: m
                .attachments
                .into_iter()
                .map(|a| AttachmentOut {
                    url: buzon_sync::parse::attachment_url(m.id, a.index),
                    index: a.index,
                    name: a.name,
                    content_type: a.content_type,
                    size: a.size,
                    inline: a.inline,
                })
                .collect(),
        })
        .collect();
    Ok(Json(ThreadDetail {
        id: t.id,
        account_id: t.account_id,
        subject: t.subject,
        messages,
    }))
}

/// Marca un hilo como leído o no leído, o lo destaca. También se aplica en el servidor IMAP.
#[utoipa::path(patch, path = "/api/v1/threads/{id}", tag = "mail", request_body = ThreadUpdate,
    params(("id" = String, Path, description = "Id del hilo")),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn update_thread(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<ThreadUpdate>,
) -> ApiResult<StatusCode> {
    let id = parse_id(&id)?;
    let udb = st.user_db(cu.user.id).await?;
    let account = udb
        .thread(id)
        .await?
        .ok_or_else(|| ApiError::not_found("Ese hilo no existe."))?
        .account_id;
    let changes = udb.update_thread(id, req.seen, req.flagged).await?;
    if let Some(v) = req.seen {
        st.sync.store_flag(&changes.seen, "\\Seen", v);
    }
    if let Some(v) = req.flagged {
        st.sync.store_flag(&changes.flagged, "\\Flagged", v);
    }
    if !changes.seen.is_empty() || !changes.flagged.is_empty() {
        st.sync.emit(cu.user.id, account);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Tipos que el navegador puede mostrar sin riesgo; el resto se descarga como binario.
const SAFE_TYPES: [&str; 6] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "application/pdf",
    "text/plain",
];

/// `filename*` de RFC 6266/5987, para nombres con acentos.
fn content_disposition(name: &str) -> String {
    let mut enc = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            enc.push(b as char);
        } else {
            enc.push_str(&format!("%{b:02X}"));
        }
    }
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{enc}")
}

#[utoipa::path(get, path = "/api/v1/messages/{id}/attachments/{index}", tag = "mail",
    params(("id" = String, Path, description = "Id del mensaje"), ("index" = u32, Path, description = "Posición del adjunto")),
    responses((status = 200, description = "El adjunto", content_type = "application/octet-stream"), (status = 404, body = ErrorBody)))]
pub async fn attachment(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path((id, index)): Path<(String, u32)>,
) -> ApiResult<Response> {
    let id = parse_id(&id)?;
    let missing = || ApiError::not_found("Ese adjunto no existe.");
    let udb = st.user_db(cu.user.id).await?;
    let (_, list) = udb.attachments(id).await?.ok_or_else(missing)?;
    let meta = list.into_iter().find(|a| a.index == index).ok_or_else(missing)?;
    let path = user_raw_dir(&st.data_dir, cu.user.id).join(format!("{id}.eml"));
    let raw = tokio::fs::read(&path).await.map_err(|_| missing())?;
    let bytes = tokio::task::spawn_blocking(move || buzon_sync::parse::attachment(&raw, index))
        .await
        .ok()
        .flatten()
        .ok_or_else(missing)?;
    let ct = if SAFE_TYPES.contains(&meta.content_type.as_str()) {
        meta.content_type.as_str()
    } else {
        "application/octet-stream"
    };
    Ok((
        [
            (header::CONTENT_TYPE, ct.to_string()),
            (header::CONTENT_DISPOSITION, content_disposition(&meta.name)),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
            (header::CONTENT_SECURITY_POLICY, "default-src 'none'; sandbox".into()),
            (header::CACHE_CONTROL, "private, max-age=86400".into()),
        ],
        Body::from(bytes),
    )
        .into_response())
}

/// Avisos en vivo (Server-Sent Events): `mail` cuando cambia el correo de una cuenta
/// y `status` cuando cambia su estado de sincronización.
#[utoipa::path(get, path = "/api/v1/events", tag = "mail",
    responses((status = 200, description = "Flujo de eventos", content_type = "text/event-stream")))]
pub async fn events(State(st): State<Arc<AppState>>, cu: CurrentUser) -> impl IntoResponse {
    let user = cu.user.id;
    let stream: std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> =
        Box::pin(BroadcastStream::new(st.sync.subscribe()).filter_map(move |r| match r {
            Ok(e) if e.user == user => Event::default().event(e.kind).json_data(&e).ok().map(Ok),
            Ok(_) => None,
            // Si se pierden avisos, que la web lo recargue todo.
            Err(_) => Some(Ok(Event::default().event("mail").data("{}"))),
        }));
    (
        [(HeaderName::from_static("x-accel-buffering"), "no")],
        Sse::new(stream).keep_alive(KeepAlive::default()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposition_handles_accents() {
        assert_eq!(
            content_disposition("Factura año.pdf"),
            "attachment; filename=\"Factura_a_o.pdf\"; filename*=UTF-8''Factura%20a%C3%B1o.pdf"
        );
    }
}

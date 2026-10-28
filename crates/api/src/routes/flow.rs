//! El flujo del modo Enfoque: filtro de remitentes, pilas, hilos pospuestos y notas.
//! Todo vive en Buzón; el servidor IMAP no se entera.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use buzon_store::flow::{FlowChange, BUCKETS, STATUSES};
use buzon_store::Id;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::error::{ApiError, ApiResult, ErrorBody};
use crate::models::{FlowCountsOut, NoteInput, SenderDecision, SenderOut, SenderSummaryOut, ThreadsFlow};
use crate::routes::compose::valid_email;
use crate::routes::organize::{accounts_of, check_threads};
use crate::session::CurrentUser;
use crate::AppState;

/// Una nota no es un documento.
const MAX_NOTE: usize = 10_000;

#[derive(Deserialize, IntoParams)]
pub struct FlowParams {
    /// Solo esta cuenta. Sin ella, todas.
    pub account: Option<String>,
}

/// Cuántos hilos hay en cada bandeja y pila del modo Enfoque.
#[utoipa::path(get, path = "/api/v1/flow", tag = "flow", params(FlowParams),
    responses((status = 200, body = FlowCountsOut)))]
pub async fn counts(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Query(p): Query<FlowParams>,
) -> ApiResult<Json<FlowCountsOut>> {
    let account = match p.account.as_deref() {
        Some(a) => Some(
            a.parse::<Id>()
                .map_err(|_| ApiError::not_found("Esa cuenta no existe."))?,
        ),
        None => None,
    };
    let udb = st.user_db(cu.user.id).await?;
    Ok(Json(udb.flow_counts(account).await?.into()))
}

#[derive(Deserialize, IntoParams)]
pub struct SenderParams {
    /// `pending`, `approved` o `blocked`. Sin él, todos.
    pub status: Option<String>,
}

/// Remitentes con correo en la entrada. Con `status=pending`, los que esperan aprobación.
#[utoipa::path(get, path = "/api/v1/senders", tag = "flow", params(SenderParams),
    responses((status = 200, body = Vec<SenderSummaryOut>), (status = 400, body = ErrorBody)))]
pub async fn senders(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Query(p): Query<SenderParams>,
) -> ApiResult<Json<Vec<SenderSummaryOut>>> {
    if let Some(s) = &p.status {
        if !STATUSES.contains(&s.as_str()) {
            return Err(ApiError::bad_request("Ese estado no existe."));
        }
    }
    let udb = st.user_db(cu.user.id).await?;
    let list = udb.senders(p.status.as_deref()).await?;
    Ok(Json(list.into_iter().map(Into::into).collect()))
}

/// Aprueba o rechaza a un remitente, o cambia la bandeja a la que va su correo.
#[utoipa::path(put, path = "/api/v1/senders", tag = "flow", request_body = SenderDecision,
    responses((status = 200, body = SenderOut), (status = 400, body = ErrorBody)))]
pub async fn decide(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<SenderDecision>,
) -> ApiResult<Json<SenderOut>> {
    if !valid_email(req.email.trim()) {
        return Err(ApiError::bad_request("La dirección no es válida."));
    }
    if !STATUSES.contains(&req.status.as_str()) {
        return Err(ApiError::bad_request("Ese estado no existe."));
    }
    let bucket = req.bucket.unwrap_or_default();
    if !bucket.is_empty() && !BUCKETS.contains(&bucket.as_str()) {
        return Err(ApiError::bad_request("Esa bandeja no existe."));
    }
    let udb = st.user_db(cu.user.id).await?;
    let s = udb.decide_sender(&req.email, &req.status, &bucket).await?;
    for a in udb.accounts().await? {
        st.sync.emit(cu.user.id, a.id);
    }
    let _ = st.db.audit(Some(cu.user.id), "contact.screened", None).await;
    Ok(Json(s.into()))
}

/// Mete o saca hilos de «Responder luego» y «Apartados», o los pospone.
#[utoipa::path(post, path = "/api/v1/threads/flow", tag = "flow", request_body = ThreadsFlow,
    responses((status = 204), (status = 400, body = ErrorBody)))]
pub async fn set_flow(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<ThreadsFlow>,
) -> ApiResult<StatusCode> {
    check_threads(&req.threads)?;
    if req.reply_later.is_none() && req.aside.is_none() && req.snooze_until.is_none() {
        return Err(ApiError::bad_request("No hay nada que cambiar."));
    }
    let snooze = match req.snooze_until {
        Some(0) => Some(None),
        Some(t) if t <= buzon_store::now() => return Err(ApiError::bad_request("Esa fecha ya ha pasado.")),
        Some(t) => Some(Some(t)),
        None => None,
    };
    let udb = st.user_db(cu.user.id).await?;
    udb.set_flow(
        &req.threads,
        FlowChange {
            reply_later: req.reply_later,
            aside: req.aside,
            snooze,
        },
    )
    .await?;
    for a in accounts_of(&udb, &req.threads).await? {
        st.sync.emit(cu.user.id, a);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Guarda la nota privada de un hilo (vacía para quitarla).
#[utoipa::path(put, path = "/api/v1/threads/{id}/note", tag = "flow", request_body = NoteInput,
    params(("id" = String, Path, description = "Id del hilo")),
    responses((status = 204), (status = 400, body = ErrorBody), (status = 404, body = ErrorBody)))]
pub async fn set_note(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<NoteInput>,
) -> ApiResult<StatusCode> {
    if req.note.chars().count() > MAX_NOTE {
        return Err(ApiError::bad_request("La nota es demasiado larga."));
    }
    let missing = || ApiError::not_found("Ese hilo no existe.");
    let id: Id = id.parse().map_err(|_| missing())?;
    let udb = st.user_db(cu.user.id).await?;
    if !udb.set_note(id, &req.note).await? {
        return Err(missing());
    }
    Ok(StatusCode::NO_CONTENT)
}

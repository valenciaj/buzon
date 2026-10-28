//! Webhooks del usuario: alta, cambios, prueba e historial de envíos.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use buzon_store::hooks::{NewWebhook, WebhookChange, EVENTS, PING};
use buzon_store::{crypto, Id};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::error::{ApiError, ApiResult, ErrorBody};
use crate::hooks::check_url;
use crate::models::{DeliveryOut, WebhookCreated, WebhookInput, WebhookOut, WebhookUpdate};
use crate::session::CurrentUser;
use crate::AppState;

const MAX_WEBHOOKS: usize = 20;

fn missing() -> ApiError {
    ApiError::not_found("Ese webhook no existe.")
}

fn parse(id: &str) -> ApiResult<Id> {
    id.parse().map_err(|_| missing())
}

fn check_events(events: &[String]) -> ApiResult<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for e in events {
        if !EVENTS.contains(&e.as_str()) {
            return Err(ApiError::bad_request(format!("El evento «{e}» no existe.")));
        }
        if !out.contains(e) {
            out.push(e.clone());
        }
    }
    if out.is_empty() {
        return Err(ApiError::bad_request("Elige al menos un evento."));
    }
    Ok(out)
}

fn check_description(d: &str) -> ApiResult<&str> {
    let d = d.trim();
    if d.chars().count() > 200 {
        return Err(ApiError::bad_request("La descripción es demasiado larga."));
    }
    Ok(d)
}

#[utoipa::path(get, path = "/api/v1/webhooks", tag = "webhooks", responses((status = 200, body = [WebhookOut])))]
pub async fn list(State(st): State<Arc<AppState>>, cu: CurrentUser) -> ApiResult<Json<Vec<WebhookOut>>> {
    let list = st.db.webhooks(cu.user.id).await?;
    Ok(Json(list.into_iter().map(Into::into).collect()))
}

/// Crea un webhook. La respuesta trae el secreto de firma, que no se vuelve a mostrar.
#[utoipa::path(post, path = "/api/v1/webhooks", tag = "webhooks", request_body = WebhookInput,
    responses((status = 201, body = WebhookCreated), (status = 400, body = ErrorBody)))]
pub async fn create(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<WebhookInput>,
) -> ApiResult<(StatusCode, Json<WebhookCreated>)> {
    let url = req.url.trim();
    check_url(url, st.allow_private_webhooks).map_err(ApiError::bad_request)?;
    let events = check_events(&req.events)?;
    let description = check_description(&req.description)?;
    if st.db.webhooks(cu.user.id).await?.len() >= MAX_WEBHOOKS {
        return Err(ApiError::bad_request("Tienes demasiados webhooks."));
    }
    let secret = format!("whsec_{}", crypto::random_token());
    let sealed = st.hooks_box().seal(secret.as_bytes())?;
    let w = st
        .db
        .create_webhook(
            cu.user.id,
            NewWebhook {
                url,
                description,
                events: &events,
                secret: &sealed,
            },
        )
        .await?;
    let _ = st.db.audit(Some(cu.user.id), "webhook.created", Some(url)).await;
    Ok((StatusCode::CREATED, Json(WebhookCreated { info: w.into(), secret })))
}

/// Cambia la URL, los eventos o la descripción, o lo pausa (`active: false`).
#[utoipa::path(put, path = "/api/v1/webhooks/{id}", tag = "webhooks", request_body = WebhookUpdate,
    params(("id" = String, Path, description = "Id del webhook")),
    responses((status = 200, body = WebhookOut), (status = 400, body = ErrorBody), (status = 404, body = ErrorBody)))]
pub async fn update(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<WebhookUpdate>,
) -> ApiResult<Json<WebhookOut>> {
    let id = parse(&id)?;
    let url = req.url.as_deref().map(str::trim);
    if let Some(u) = url {
        check_url(u, st.allow_private_webhooks).map_err(ApiError::bad_request)?;
    }
    let events = req.events.as_deref().map(check_events).transpose()?;
    let description = req.description.as_deref().map(check_description).transpose()?;
    let w = st
        .db
        .update_webhook(
            cu.user.id,
            id,
            WebhookChange {
                url,
                description,
                events: events.as_deref(),
                active: req.active,
            },
        )
        .await?
        .ok_or_else(missing)?;
    if w.active {
        // Al reanudarlo, sale lo que esperaba en la cola.
        st.hooks_ready.notify_one();
    }
    Ok(Json(w.into()))
}

#[utoipa::path(delete, path = "/api/v1/webhooks/{id}", tag = "webhooks",
    params(("id" = String, Path, description = "Id del webhook")),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn remove(State(st): State<Arc<AppState>>, cu: CurrentUser, Path(id): Path<String>) -> ApiResult<StatusCode> {
    if !st.db.delete_webhook(cu.user.id, parse(&id)?).await? {
        return Err(missing());
    }
    let _ = st.db.audit(Some(cu.user.id), "webhook.deleted", None).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Envía un evento `ping` de prueba. Sale enseguida; el resultado aparece en el historial.
#[utoipa::path(post, path = "/api/v1/webhooks/{id}/test", tag = "webhooks",
    params(("id" = String, Path, description = "Id del webhook")),
    responses((status = 202), (status = 404, body = ErrorBody)))]
pub async fn test(State(st): State<Arc<AppState>>, cu: CurrentUser, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let data = serde_json::json!({ "message": "Hola desde Buzón" }).to_string();
    if st.db.enqueue_event(cu.user.id, PING, &data, Some(parse(&id)?)).await? == 0 {
        return Err(missing());
    }
    st.hooks_ready.notify_one();
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize, IntoParams)]
pub struct DeliveryParams {
    /// Cuántos (como mucho 100). Por defecto, 50.
    pub limit: Option<i64>,
}

/// Historial de envíos, del más reciente al más antiguo (se guardan 30 días).
#[utoipa::path(get, path = "/api/v1/webhooks/{id}/deliveries", tag = "webhooks",
    params(("id" = String, Path, description = "Id del webhook"), DeliveryParams),
    responses((status = 200, body = [DeliveryOut]), (status = 404, body = ErrorBody)))]
pub async fn deliveries(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
    Query(p): Query<DeliveryParams>,
) -> ApiResult<Json<Vec<DeliveryOut>>> {
    let id = parse(&id)?;
    st.db.webhook(cu.user.id, id).await?.ok_or_else(missing)?;
    let list = st
        .db
        .deliveries(cu.user.id, id, p.limit.unwrap_or(50).clamp(1, 100))
        .await?;
    Ok(Json(list.into_iter().map(Into::into).collect()))
}

/// Vuelve a enviar un evento del historial, con el mismo contenido.
#[utoipa::path(post, path = "/api/v1/webhooks/deliveries/{id}/redeliver", tag = "webhooks",
    params(("id" = String, Path, description = "Id del envío")),
    responses((status = 201, body = DeliveryOut), (status = 404, body = ErrorBody)))]
pub async fn redeliver(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
) -> ApiResult<(StatusCode, Json<DeliveryOut>)> {
    let gone = || ApiError::not_found("Ese envío no existe.");
    let id: Id = id.parse().map_err(|_| gone())?;
    let d = st.db.redeliver(cu.user.id, id).await?.ok_or_else(gone)?;
    st.hooks_ready.notify_one();
    Ok((StatusCode::CREATED, Json(d.into())))
}

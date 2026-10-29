//! Envíos programados y seguimiento de respuestas.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use buzon_store::Id;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::error::{ApiError, ApiResult, ErrorBody};
use crate::models::{DraftOut, FollowUpOut};
use crate::routes::compose::draft_out;
use crate::session::CurrentUser;
use crate::AppState;

/// Envíos programados, del más próximo al más lejano.
#[utoipa::path(get, path = "/api/v1/scheduled", tag = "compose", responses((status = 200, body = [DraftOut])))]
pub async fn scheduled(State(st): State<Arc<AppState>>, cu: CurrentUser) -> ApiResult<Json<Vec<DraftOut>>> {
    let udb = st.user_db(cu.user.id).await?;
    let mut out = Vec::new();
    for d in udb.scheduled().await? {
        out.push(draft_out(&udb, d).await?);
    }
    Ok(Json(out))
}

/// Cancela un envío programado (o «deshace» uno recién enviado): vuelve a Borradores.
#[utoipa::path(post, path = "/api/v1/scheduled/{id}/cancel", tag = "compose",
    params(("id" = String, Path, description = "Id del borrador programado")),
    responses((status = 200, body = DraftOut), (status = 404, body = ErrorBody),
        (status = 409, description = "Ya ha salido o está saliendo", body = ErrorBody)))]
pub async fn cancel(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
) -> ApiResult<Json<DraftOut>> {
    let gone = || {
        ApiError::conflict(
            "already_sent",
            "Demasiado tarde: el correo ya ha salido o está saliendo.",
        )
    };
    let id: Id = id.parse().map_err(|_| gone())?;
    let udb = st.user_db(cu.user.id).await?;
    if !udb.unschedule_draft(id).await? {
        return Err(gone());
    }
    let d = udb.draft(id).await?.ok_or_else(gone)?;
    Ok(Json(draft_out(&udb, d).await?))
}

#[derive(Deserialize, IntoParams)]
pub struct FollowUpParams {
    /// Solo los que ya han pasado el plazo sin respuesta.
    #[serde(default)]
    pub due: bool,
}

/// Correos enviados que esperan respuesta. Los que ya la tienen desaparecen solos.
#[utoipa::path(get, path = "/api/v1/followups", tag = "compose", params(FollowUpParams),
    responses((status = 200, body = [FollowUpOut])))]
pub async fn followups(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Query(p): Query<FollowUpParams>,
) -> ApiResult<Json<Vec<FollowUpOut>>> {
    let udb = st.user_db(cu.user.id).await?;
    Ok(Json(udb.followups(p.due).await?.into_iter().map(Into::into).collect()))
}

/// Deja de esperar respuesta a un correo.
#[utoipa::path(delete, path = "/api/v1/followups/{id}", tag = "compose",
    params(("id" = String, Path, description = "Id del seguimiento")),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn dismiss(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let missing = || ApiError::not_found("Ese seguimiento no existe.");
    let id: Id = id.parse().map_err(|_| missing())?;
    if !st.user_db(cu.user.id).await?.dismiss_followup(id).await? {
        return Err(missing());
    }
    Ok(StatusCode::NO_CONTENT)
}

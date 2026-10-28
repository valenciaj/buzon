//! Tokens personales de la API. Solo se gestionan desde la web, nunca con otro token.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use buzon_store::tokens::{NewToken, SCOPES};
use buzon_store::{crypto, now, Id};

use crate::error::{ApiError, ApiResult, ErrorBody};
use crate::models::{TokenCreated, TokenInput, TokenOut};
use crate::session::{CurrentUser, TOKEN_PREFIX};
use crate::AppState;

/// Nadie necesita más.
const MAX_TOKENS: usize = 50;

#[utoipa::path(get, path = "/api/v1/tokens", tag = "tokens", responses((status = 200, body = [TokenOut])))]
pub async fn list(State(st): State<Arc<AppState>>, cu: CurrentUser) -> ApiResult<Json<Vec<TokenOut>>> {
    let list = st.db.tokens(cu.user.id).await?;
    Ok(Json(list.into_iter().map(Into::into).collect()))
}

/// Crea un token. La respuesta trae el token completo, que no se vuelve a mostrar.
#[utoipa::path(post, path = "/api/v1/tokens", tag = "tokens", request_body = TokenInput,
    responses((status = 201, body = TokenCreated), (status = 400, body = ErrorBody)))]
pub async fn create(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<TokenInput>,
) -> ApiResult<(StatusCode, Json<TokenCreated>)> {
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(ApiError::bad_request("El nombre debe tener entre 1 y 60 caracteres."));
    }
    let mut scopes: Vec<String> = Vec::new();
    for s in &req.scopes {
        if !SCOPES.contains(&s.as_str()) {
            return Err(ApiError::bad_request(format!("El permiso «{s}» no existe.")));
        }
        if !scopes.contains(s) {
            scopes.push(s.clone());
        }
    }
    if scopes.is_empty() {
        return Err(ApiError::bad_request("Elige al menos un permiso."));
    }
    let expires_at = match req.expires_days {
        None => None,
        Some(d @ 1..=365) => Some(now() + d * 24 * 3600),
        Some(_) => return Err(ApiError::bad_request("La caducidad debe ser de 1 a 365 días.")),
    };
    if st.db.tokens(cu.user.id).await?.len() >= MAX_TOKENS {
        return Err(ApiError::bad_request(
            "Tienes demasiados tokens. Revoca los que no uses.",
        ));
    }
    let token = format!("{TOKEN_PREFIX}{}", crypto::random_token());
    let info = st
        .db
        .create_token(NewToken {
            user: cu.user.id,
            name,
            token_hash: &crypto::token_hash(&token),
            prefix: &token[..TOKEN_PREFIX.len() + 6],
            scopes: &scopes,
            expires_at,
        })
        .await?;
    let _ = st.db.audit(Some(cu.user.id), "token.created", Some(name)).await;
    Ok((
        StatusCode::CREATED,
        Json(TokenCreated {
            info: info.into(),
            token,
        }),
    ))
}

/// Revoca un token: deja de funcionar al momento.
#[utoipa::path(delete, path = "/api/v1/tokens/{id}", tag = "tokens",
    params(("id" = String, Path, description = "Id del token")),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn revoke(State(st): State<Arc<AppState>>, cu: CurrentUser, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let missing = || ApiError::not_found("Ese token no existe.");
    let id: Id = id.parse().map_err(|_| missing())?;
    if !st.db.revoke_token(cu.user.id, id).await? {
        return Err(missing());
    }
    let _ = st.db.audit(Some(cu.user.id), "token.revoked", None).await;
    Ok(StatusCode::NO_CONTENT)
}

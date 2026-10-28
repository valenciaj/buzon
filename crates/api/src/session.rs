use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::header::{COOKIE, SET_COOKIE, USER_AGENT};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue};
use buzon_store::app::User;
use buzon_store::crypto;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub const COOKIE_NAME: &str = "buzon_session";
pub const SESSION_TTL: i64 = 30 * 24 * 3600;

pub fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE_NAME)
        .map(|(_, v)| v.to_string())
}

fn cookie(state: &AppState, value: &str, max_age: i64) -> HeaderValue {
    let secure = if state.cookie_secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{COOKIE_NAME}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}"
    ))
    .expect("cabecera de cookie válida")
}

/// Abre una sesión para el usuario y devuelve la cabecera `Set-Cookie`.
pub async fn start(state: &AppState, user: &User, headers: &HeaderMap) -> ApiResult<HeaderMap> {
    let token = crypto::random_token();
    let ua = headers.get(USER_AGENT).and_then(|v| v.to_str().ok());
    state
        .db
        .create_session(user.id, &crypto::token_hash(&token), ua, SESSION_TTL)
        .await?;
    let mut out = HeaderMap::new();
    out.insert(SET_COOKIE, cookie(state, &token, SESSION_TTL));
    Ok(out)
}

pub fn clear(state: &AppState) -> HeaderMap {
    let mut out = HeaderMap::new();
    out.insert(SET_COOKIE, cookie(state, "", 0));
    out
}

/// Usuario de la sesión actual. Responde 401 si no hay sesión válida.
pub struct CurrentUser {
    pub user: User,
    pub token_hash: String,
}

impl FromRequestParts<Arc<AppState>> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, ApiError> {
        let token = session_token(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        let token_hash = crypto::token_hash(&token);
        let user = state
            .db
            .session_user(&token_hash)
            .await?
            .ok_or_else(ApiError::unauthorized)?;
        Ok(CurrentUser { user, token_hash })
    }
}

/// Como `CurrentUser`, pero exige rol de administrador.
pub struct AdminUser(pub User);

impl FromRequestParts<Arc<AppState>> for AdminUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, ApiError> {
        let cu = CurrentUser::from_request_parts(parts, state).await?;
        if cu.user.is_admin() {
            Ok(AdminUser(cu.user))
        } else {
            Err(ApiError::forbidden())
        }
    }
}

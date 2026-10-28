use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::header::{AUTHORIZATION, COOKIE, SET_COOKIE, USER_AGENT};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use buzon_store::app::User;
use buzon_store::tokens::ApiToken;
use buzon_store::{crypto, now};

use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub const COOKIE_NAME: &str = "buzon_session";
pub const SESSION_TTL: i64 = 30 * 24 * 3600;
/// Los tokens personales empiezan así, para reconocerlos (y para los escáneres de secretos).
pub const TOKEN_PREFIX: &str = "bz_";
/// Peticiones por minuto que admite cada token.
pub const RATE_LIMIT: u32 = 300;

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

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

/// Permiso que necesita un token para cada ruta. `None`: solo con sesión en la web
/// (la cuenta, la seguridad, los propios tokens, las invitaciones…).
pub fn scope_for(method: &Method, path: &str) -> Option<&'static str> {
    let path = path.strip_prefix("/api/v1").unwrap_or(path);
    let seg: Vec<&str> = path.trim_matches('/').split('/').collect();
    let read = *method == Method::GET;
    match seg.as_slice() {
        ["contacts", ..] => Some("contacts"),
        ["webhooks", ..] => Some("webhooks"),
        ["send"] | ["uploads", ..] => Some("mail:send"),
        ["drafts", ..] => Some(if read { "mail:read" } else { "mail:send" }),
        ["accounts"] if read => Some("mail:read"),
        ["accounts", _, "sync"] => Some("mail:write"),
        ["identities", ..] if read => Some("mail:read"),
        ["mailboxes"] | ["events"] | ["messages", ..] if read => Some("mail:read"),
        ["threads", ..] | ["flow"] | ["senders"] | ["labels", ..] | ["rules", ..] => {
            Some(if read { "mail:read" } else { "mail:write" })
        }
        _ => None,
    }
}

/// Usuario de la sesión actual o del token (`Authorization: Bearer bz_…`).
/// Responde 401 si no hay ninguno válido.
pub struct CurrentUser {
    pub user: User,
    /// Hash de la sesión (vacío si se entra con token).
    pub token_hash: String,
    /// Token con el que se entró, si no es una sesión de la web.
    pub token: Option<ApiToken>,
}

impl FromRequestParts<Arc<AppState>> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, ApiError> {
        if let Some(raw) = bearer(&parts.headers) {
            return from_token(raw, parts, state).await;
        }
        let token = session_token(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        let token_hash = crypto::token_hash(&token);
        let user = state
            .db
            .session_user(&token_hash)
            .await?
            .ok_or_else(ApiError::unauthorized)?;
        Ok(CurrentUser {
            user,
            token_hash,
            token: None,
        })
    }
}

async fn from_token(raw: &str, parts: &Parts, state: &AppState) -> Result<CurrentUser, ApiError> {
    let invalid = || {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_token",
            "El token no es válido, ha caducado o se ha revocado.",
        )
    };
    if !raw.starts_with(TOKEN_PREFIX) {
        return Err(invalid());
    }
    let (user, token) = state
        .db
        .token_user(&crypto::token_hash(raw))
        .await?
        .ok_or_else(invalid)?;
    let Some(scope) = scope_for(&parts.method, parts.uri.path()) else {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "session_required",
            "Esto solo se puede hacer desde la web, no con un token.",
        ));
    };
    if !token.scopes.iter().any(|s| s == scope) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "scope_required",
            format!("El token no tiene el permiso «{scope}»."),
        ));
    }
    if !state.rate_ok(token.id, now()) {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            format!("Demasiadas peticiones: como mucho {RATE_LIMIT} por minuto con cada token."),
        ));
    }
    Ok(CurrentUser {
        user,
        token_hash: String::new(),
        token: Some(token),
    })
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

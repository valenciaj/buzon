use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use buzon_store::{crypto, now};

use crate::error::{ApiError, ApiResult, ErrorBody};
use crate::models::{LoginRequest, Me};
use crate::session::{self, CurrentUser};
use crate::{prefs, AppState};

pub const MAX_FAILED_LOGINS: i64 = 5;
pub const LOCK_SECS: i64 = 15 * 60;

fn bad_credentials() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        "invalid_credentials",
        "Usuario o contraseña incorrectos.",
    )
}

/// Inicia sesión con el usuario y la contraseña de Buzón (no los del correo).
/// Si el usuario tiene 2FA y no se envía `totp`, responde 401 con `totp_required`.
#[utoipa::path(post, path = "/api/v1/auth/login", tag = "auth", request_body = LoginRequest,
    responses(
        (status = 200, body = Me, description = "Sesión iniciada; se envía la cookie `buzon_session`"),
        (status = 401, body = ErrorBody, description = "`invalid_credentials`, `totp_required` o `invalid_totp`"),
        (status = 423, body = ErrorBody, description = "Cuenta bloqueada por demasiados intentos"),
    ))]
pub async fn login(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> ApiResult<(HeaderMap, Json<Me>)> {
    let Some(user) = st.db.user_by_username(req.username.trim()).await? else {
        // Mismo coste que una contraseña real, para no revelar qué usuarios existen.
        crypto::verify_password(&req.password, dummy_hash());
        return Err(bad_credentials());
    };
    if let Some(until) = user.locked_until.filter(|t| *t > now()) {
        let mins = (until - now() + 59) / 60;
        return Err(ApiError::new(
            StatusCode::LOCKED,
            "locked",
            format!("Demasiados intentos fallidos. Prueba de nuevo en {mins} min."),
        ));
    }
    if !crypto::verify_password(&req.password, &user.password_hash) {
        st.db.record_failed_login(user.id, MAX_FAILED_LOGINS, LOCK_SECS).await?;
        return Err(bad_credentials());
    }
    if user.totp_enabled {
        let Some(code) = req.totp.as_deref().map(str::trim).filter(|c| !c.is_empty()) else {
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "totp_required",
                "Escribe el código de tu app de autenticación.",
            ));
        };
        if !st.check_totp(&user, code)? {
            st.db.record_failed_login(user.id, MAX_FAILED_LOGINS, LOCK_SECS).await?;
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_totp",
                "El código no es correcto o ha caducado.",
            ));
        }
    }
    st.db.reset_failed_logins(user.id).await?;
    st.db.audit(Some(user.id), "auth.login", None).await?;
    let cookie = session::start(&st, &user, &headers).await?;
    let udb = st.user_db(user.id).await?;
    Ok((cookie, Json(Me::new(&user, prefs::load(&udb).await?))))
}

#[utoipa::path(post, path = "/api/v1/auth/logout", tag = "auth",
    responses((status = 204, description = "Sesión cerrada")))]
pub async fn logout(State(st): State<Arc<AppState>>, cu: CurrentUser) -> ApiResult<(StatusCode, HeaderMap)> {
    st.db.delete_session(&cu.token_hash).await?;
    Ok((StatusCode::NO_CONTENT, session::clear(&st)))
}

/// Hash Argon2 de una contraseña cualquiera, solo para igualar tiempos.
fn dummy_hash() -> &'static str {
    static H: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    H.get_or_init(|| crypto::hash_password("buzon-dummy-password").unwrap_or_default())
}

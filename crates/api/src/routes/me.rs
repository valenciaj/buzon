use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use buzon_store::crypto;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{ApiError, ApiResult, ErrorBody};
use crate::models::{Me, PreferencesUpdate};
use crate::session::CurrentUser;
use crate::{prefs, validate, AppState};

#[utoipa::path(get, path = "/api/v1/me", tag = "me",
    responses((status = 200, body = Me), (status = 401, body = ErrorBody)))]
pub async fn get(State(st): State<Arc<AppState>>, cu: CurrentUser) -> ApiResult<Json<Me>> {
    let udb = st.user_db(cu.user.id).await?;
    Ok(Json(Me::new(&cu.user, prefs::load(&udb).await?)))
}

/// Cambia preferencias de interfaz (tema, modo, vista, fuente, tamaño, densidad e idioma).
#[utoipa::path(put, path = "/api/v1/me/preferences", tag = "me", request_body = PreferencesUpdate,
    responses((status = 200, body = Me), (status = 400, body = ErrorBody)))]
pub async fn update_preferences(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<PreferencesUpdate>,
) -> ApiResult<Json<Me>> {
    let mut user = cu.user;
    if let Some(l) = &req.language {
        validate::language(l)?;
        st.db.set_language(user.id, l).await?;
        user.language = l.clone();
    }
    let udb = st.user_db(user.id).await?;
    prefs::save(&udb, &req).await?;
    Ok(Json(Me::new(&user, prefs::load(&udb).await?)))
}

#[derive(Serialize, ToSchema)]
pub struct TotpSetup {
    /// Semilla en Base32, para escribirla a mano si no se puede leer el QR.
    pub secret: String,
    /// URL `otpauth://` para generar el código QR.
    pub otpauth_url: String,
}

/// Genera una semilla 2FA nueva. No se activa hasta confirmar un código con `/me/totp/enable`.
#[utoipa::path(post, path = "/api/v1/me/totp/setup", tag = "me",
    responses((status = 200, body = TotpSetup), (status = 409, body = ErrorBody)))]
pub async fn totp_setup(State(st): State<Arc<AppState>>, cu: CurrentUser) -> ApiResult<Json<TotpSetup>> {
    if cu.user.totp_enabled {
        return Err(ApiError::conflict(
            "totp_enabled",
            "La verificación en dos pasos ya está activada.",
        ));
    }
    let secret = crypto::new_totp_secret();
    let totp = crypto::totp(&secret, &cu.user.username)?;
    st.db
        .set_totp(cu.user.id, Some(&st.totp_box().seal(&secret)?), false)
        .await?;
    Ok(Json(TotpSetup {
        secret: crypto::totp_secret_base32(&secret),
        otpauth_url: totp.get_url(),
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct TotpCode {
    pub code: String,
}

#[utoipa::path(post, path = "/api/v1/me/totp/enable", tag = "me", request_body = TotpCode,
    responses((status = 200, body = Me), (status = 400, body = ErrorBody)))]
pub async fn totp_enable(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<TotpCode>,
) -> ApiResult<Json<Me>> {
    if cu.user.totp_secret.is_none() {
        return Err(ApiError::bad_request("Primero genera el código QR."));
    }
    if !st.check_totp(&cu.user, req.code.trim())? {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_totp",
            "El código no es correcto o ha caducado.",
        ));
    }
    st.db.set_totp(cu.user.id, cu.user.totp_secret.as_deref(), true).await?;
    st.db.audit(Some(cu.user.id), "totp.enabled", None).await?;
    let mut user = cu.user;
    user.totp_enabled = true;
    let udb = st.user_db(user.id).await?;
    Ok(Json(Me::new(&user, prefs::load(&udb).await?)))
}

#[derive(Deserialize, ToSchema)]
pub struct PasswordConfirm {
    pub password: String,
}

#[utoipa::path(post, path = "/api/v1/me/totp/disable", tag = "me", request_body = PasswordConfirm,
    responses((status = 200, body = Me), (status = 401, body = ErrorBody)))]
pub async fn totp_disable(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<PasswordConfirm>,
) -> ApiResult<Json<Me>> {
    if !crypto::verify_password(&req.password, &cu.user.password_hash) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "La contraseña no es correcta.",
        ));
    }
    st.db.set_totp(cu.user.id, None, false).await?;
    st.db.audit(Some(cu.user.id), "totp.disabled", None).await?;
    let mut user = cu.user;
    user.totp_enabled = false;
    user.totp_secret = None;
    let udb = st.user_db(user.id).await?;
    Ok(Json(Me::new(&user, prefs::load(&udb).await?)))
}

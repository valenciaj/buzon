use std::sync::Arc;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use buzon_store::app::{NewUser, Role};
use buzon_store::crypto;
use buzon_store::user::NewAccount;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{ApiError, ApiResult};
use crate::models::{AccountInput, Me, PreferencesUpdate};
use crate::routes::accounts::validate_account;
use crate::{prefs, session, validate, AppState};

#[derive(Serialize, ToSchema)]
pub struct SetupStatus {
    /// `true` mientras no exista ningún usuario: hay que pasar por el asistente.
    pub needs_setup: bool,
}

#[utoipa::path(get, path = "/api/v1/setup", tag = "setup",
    responses((status = 200, body = SetupStatus)))]
pub async fn status(State(st): State<Arc<AppState>>) -> ApiResult<Json<SetupStatus>> {
    Ok(Json(SetupStatus {
        needs_setup: st.db.user_count().await? == 0,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct AdminInput {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct ServerInput {
    /// URL pública, por ejemplo `https://correo.ejemplo.es`. Se usa en los enlaces de invitación.
    pub public_url: Option<String>,
    /// Idioma por defecto: `es` o `en`.
    pub language: String,
    /// Zona horaria IANA, por ejemplo `Europe/Madrid`.
    pub timezone: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct SetupRequest {
    pub admin: AdminInput,
    pub server: ServerInput,
    pub preferences: Option<PreferencesUpdate>,
    /// Primera cuenta de correo. Opcional: se puede añadir después.
    pub account: Option<AccountInput>,
}

/// Asistente de primer arranque: crea el administrador, guarda los ajustes del
/// servidor y, si se envía, la primera cuenta de correo. Solo funciona una vez.
#[utoipa::path(post, path = "/api/v1/setup", tag = "setup", request_body = SetupRequest,
    responses((status = 200, body = Me), (status = 409, body = crate::error::ErrorBody)))]
pub async fn run(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<SetupRequest>,
) -> ApiResult<(HeaderMap, Json<Me>)> {
    if st.db.user_count().await? > 0 {
        return Err(ApiError::conflict("already_setup", "Buzón ya está configurado."));
    }
    validate::username(&req.admin.username)?;
    validate::password(&req.admin.password)?;
    validate::language(&req.server.language)?;
    if let Some(a) = &req.account {
        validate_account(a)?;
        crate::routes::accounts::check_security(&st, a)?;
    }
    if let Some(p) = req.preferences.as_ref().and_then(|p| p.font_size) {
        if !prefs::valid_font_size(p) {
            return Err(ApiError::bad_request(
                "El tamaño del texto va de 87.5 a 150, en pasos de 12.5.",
            ));
        }
    }

    let hash = crypto::hash_password(&req.admin.password)?;
    let user = st
        .db
        .create_user(NewUser {
            username: &req.admin.username,
            email: req.admin.email.as_deref().filter(|e| !e.is_empty()),
            password_hash: &hash,
            role: Role::Admin,
            language: &req.server.language,
        })
        .await?;

    st.db.set_setting("default_language", &req.server.language).await?;
    if let Some(url) = req
        .server
        .public_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
    {
        st.db.set_setting("public_url", url.trim_end_matches('/')).await?;
    }
    if let Some(tz) = req.server.timezone.as_deref().filter(|t| !t.is_empty()) {
        st.db.set_setting("timezone", tz).await?;
    }

    let udb = st.user_db(user.id).await?;
    if let Some(p) = &req.preferences {
        prefs::save(&udb, p).await?;
    }
    if let Some(a) = &req.account {
        let acc = udb.add_account(new_account(a), &st.secrets_for(user.id)).await?;
        st.start_account(user.id, &acc).await?;
    }
    st.db.audit(Some(user.id), "setup.completed", None).await?;

    let cookie = session::start(&st, &user, &headers).await?;
    let me = Me::new(&user, prefs::load(&udb).await?);
    Ok((cookie, Json(me)))
}

pub fn new_account(a: &AccountInput) -> NewAccount<'_> {
    NewAccount {
        name: a.name.trim(),
        email: a.email.trim(),
        imap_host: a.imap_host.trim(),
        imap_port: a.imap_port,
        smtp_host: a.smtp_host.trim(),
        smtp_port: a.smtp_port,
        login: a.login.trim(),
        password: &a.password,
        imap_security: &a.imap_security,
        smtp_security: &a.smtp_security,
    }
}

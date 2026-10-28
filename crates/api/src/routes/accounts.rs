use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use crate::error::{ApiError, ApiResult, ErrorBody};
use crate::models::{AccountInput, AccountOut};
use crate::routes::setup::new_account;
use crate::session::CurrentUser;
use crate::{validate, AppState};

pub fn validate_account(a: &AccountInput) -> ApiResult<()> {
    for (f, v) in [
        ("name", &a.name),
        ("email", &a.email),
        ("imap_host", &a.imap_host),
        ("smtp_host", &a.smtp_host),
        ("login", &a.login),
        ("password", &a.password),
    ] {
        validate::non_empty(f, v)?;
    }
    if !a.email.contains('@') {
        return Err(ApiError::bad_request("La dirección de correo no es válida."));
    }
    if a.imap_port == 0 || a.smtp_port == 0 {
        return Err(ApiError::bad_request("Los puertos IMAP y SMTP no pueden ser 0."));
    }
    Ok(())
}

#[utoipa::path(get, path = "/api/v1/accounts", tag = "accounts",
    responses((status = 200, body = [AccountOut]), (status = 401, body = ErrorBody)))]
pub async fn list(State(st): State<Arc<AppState>>, cu: CurrentUser) -> ApiResult<Json<Vec<AccountOut>>> {
    let udb = st.user_db(cu.user.id).await?;
    Ok(Json(udb.accounts().await?.into_iter().map(Into::into).collect()))
}

/// Añade una cuenta de correo. La contraseña se guarda cifrada y nunca se devuelve.
#[utoipa::path(post, path = "/api/v1/accounts", tag = "accounts", request_body = AccountInput,
    responses((status = 200, body = AccountOut), (status = 400, body = ErrorBody)))]
pub async fn create(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<AccountInput>,
) -> ApiResult<Json<AccountOut>> {
    validate_account(&req)?;
    let udb = st.user_db(cu.user.id).await?;
    let acc = udb.add_account(new_account(&req), &st.secrets_for(cu.user.id)).await?;
    Ok(Json(acc.into()))
}

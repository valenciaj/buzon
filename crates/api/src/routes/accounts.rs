use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use buzon_store::user::user_raw_dir;
use buzon_store::Id;
use buzon_sync::{Security, ServerConfig};

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
    for s in [&a.imap_security, &a.smtp_security] {
        if Security::parse(s).is_none() {
            return Err(ApiError::bad_request("El cifrado debe ser tls, starttls o none."));
        }
    }
    Ok(())
}

/// Las conexiones sin cifrar solo se permiten si la configuración lo dice (pruebas).
pub fn check_security(st: &AppState, a: &AccountInput) -> ApiResult<()> {
    if !st.allow_insecure_mail && (a.imap_security == "none" || a.smtp_security == "none") {
        return Err(ApiError::bad_request(
            "Este servidor no permite conexiones de correo sin cifrar. Usa TLS o STARTTLS.",
        ));
    }
    Ok(())
}

fn parse_id(s: &str) -> ApiResult<Id> {
    s.parse().map_err(|_| ApiError::not_found("Esa cuenta no existe."))
}

#[utoipa::path(get, path = "/api/v1/accounts", tag = "accounts",
    responses((status = 200, body = [AccountOut]), (status = 401, body = ErrorBody)))]
pub async fn list(State(st): State<Arc<AppState>>, cu: CurrentUser) -> ApiResult<Json<Vec<AccountOut>>> {
    let udb = st.user_db(cu.user.id).await?;
    Ok(Json(
        udb.accounts()
            .await?
            .into_iter()
            .map(|a| {
                let state = st.sync.state(a.id);
                AccountOut::new(a, state)
            })
            .collect(),
    ))
}

/// Añade una cuenta de correo y empieza a sincronizarla. Antes comprueba que el servidor
/// IMAP acepta el usuario y la contraseña (salvo con `verify: false`).
/// La contraseña se guarda cifrada y nunca se devuelve.
#[utoipa::path(post, path = "/api/v1/accounts", tag = "accounts", request_body = AccountInput,
    responses((status = 200, body = AccountOut), (status = 400, body = ErrorBody),
        (status = 422, description = "No se pudo entrar en el servidor IMAP", body = ErrorBody)))]
pub async fn create(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<AccountInput>,
) -> ApiResult<Json<AccountOut>> {
    validate_account(&req)?;
    check_security(&st, &req)?;
    if req.verify {
        let cfg = ServerConfig {
            host: req.imap_host.trim().to_string(),
            port: req.imap_port,
            security: Security::parse(&req.imap_security).unwrap_or(Security::Tls),
            login: req.login.trim().to_string(),
            password: req.password.clone(),
        };
        if let Err(e) = buzon_sync::check(&cfg).await {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "mail_login_failed",
                format!("No se pudo entrar en {}: {e}.", cfg.host),
            ));
        }
    }
    let udb = st.user_db(cu.user.id).await?;
    let acc = udb.add_account(new_account(&req), &st.secrets_for(cu.user.id)).await?;
    st.start_account(cu.user.id, &acc).await?;
    let state = st.sync.state(acc.id);
    Ok(Json(AccountOut::new(acc, state)))
}

/// Borra la cuenta de Buzón y su correo guardado. No toca nada en el servidor de correo.
#[utoipa::path(delete, path = "/api/v1/accounts/{id}", tag = "accounts",
    params(("id" = String, Path, description = "Id de la cuenta")),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn remove(State(st): State<Arc<AppState>>, cu: CurrentUser, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let id = parse_id(&id)?;
    let udb = st.user_db(cu.user.id).await?;
    if udb.account(id).await?.is_none() {
        return Err(ApiError::not_found("Esa cuenta no existe."));
    }
    st.sync.stop(id);
    let raw = user_raw_dir(&st.data_dir, cu.user.id);
    for m in udb.delete_account(id).await? {
        let _ = std::fs::remove_file(raw.join(format!("{m}.eml")));
    }
    st.db.audit(Some(cu.user.id), "account.deleted", None).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Sincroniza ahora, sin esperar a la siguiente vuelta.
#[utoipa::path(post, path = "/api/v1/accounts/{id}/sync", tag = "accounts",
    params(("id" = String, Path, description = "Id de la cuenta")),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn sync(State(st): State<Arc<AppState>>, cu: CurrentUser, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let id = parse_id(&id)?;
    let udb = st.user_db(cu.user.id).await?;
    let acc = udb
        .account(id)
        .await?
        .ok_or_else(|| ApiError::not_found("Esa cuenta no existe."))?;
    if st.sync.state(id) == buzon_sync::State::Off {
        st.start_account(cu.user.id, &acc).await?;
    } else {
        st.sync.sync_now(id);
    }
    Ok(StatusCode::NO_CONTENT)
}

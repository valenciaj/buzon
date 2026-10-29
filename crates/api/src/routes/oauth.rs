//! Conectar cuentas de Gmail y Outlook con OAuth2, sin contraseña.
//!
//! La web pide `start`, manda al usuario al proveedor y este vuelve a `callback`
//! con un código, que se cambia por los tokens. Se guarda cifrado solo el de renovación.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::Redirect;
use axum::Json;
use buzon_store::crypto::random_token;
use buzon_store::user::NewAccount;
use buzon_store::{now, Id};
use buzon_sync::oauth::{pkce_challenge, urlencode};
use buzon_sync::{Auth, Provider, Security, ServerConfig};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{ApiError, ApiResult, ErrorBody};
use crate::session::CurrentUser;
use crate::AppState;

/// Tiempo para dar permiso en el proveedor.
const PENDING_TTL: i64 = 10 * 60;
/// A dónde vuelve la web al terminar.
const BACK: &str = "/settings/accounts";

pub(crate) struct Pending {
    user: Id,
    provider: Provider,
    verifier: String,
    expires: i64,
}

#[derive(Serialize, ToSchema)]
pub struct OAuthProviders {
    /// Proveedores que ha configurado el administrador: `google`, `microsoft`.
    pub providers: Vec<String>,
}

#[derive(Deserialize, ToSchema, Default)]
#[serde(default)]
pub struct OAuthStart {
    /// Dirección que se propone en la pantalla del proveedor.
    pub login_hint: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct OAuthUrl {
    /// Abre esta dirección en el navegador (en la misma pestaña).
    pub url: String,
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

fn redirect_uri(base: &str) -> String {
    format!("{base}/api/v1/oauth/callback")
}

/// Con qué proveedores se pueden conectar cuentas sin contraseña.
#[utoipa::path(get, path = "/api/v1/oauth/providers", tag = "accounts",
    responses((status = 200, body = OAuthProviders), (status = 401, body = ErrorBody)))]
pub async fn providers(State(st): State<Arc<AppState>>, _cu: CurrentUser) -> Json<OAuthProviders> {
    let mut providers: Vec<String> = st.oauth.keys().map(|p| p.as_str().to_string()).collect();
    providers.sort();
    Json(OAuthProviders { providers })
}

/// Empieza a conectar una cuenta de Gmail (`google`) u Outlook (`microsoft`). Devuelve la
/// dirección del proveedor; al terminar, vuelve a Ajustes → Cuentas. Si la cuenta ya existe,
/// se vuelve a conectar (por ejemplo, tras retirar el permiso).
#[utoipa::path(post, path = "/api/v1/oauth/{provider}/start", tag = "accounts", request_body = OAuthStart,
    params(("provider" = String, Path, description = "`google` o `microsoft`")),
    responses((status = 200, body = OAuthUrl), (status = 400, body = ErrorBody), (status = 404, body = ErrorBody)))]
pub async fn start(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(provider): Path<String>,
    Json(req): Json<OAuthStart>,
) -> ApiResult<Json<OAuthUrl>> {
    let (provider, app) = Provider::parse(&provider)
        .and_then(|p| st.oauth.get(&p).map(|a| (p, a)))
        .ok_or_else(|| ApiError::not_found("Ese proveedor no está configurado en este servidor."))?;
    let base = st.public_url().await?;
    if base.is_empty() {
        return Err(ApiError::bad_request(
            "Falta la URL pública del servidor (public_url), necesaria para volver del proveedor.",
        ));
    }
    let state = random_token();
    let verifier = random_token();
    let url = app.authorize_url(
        &redirect_uri(&base),
        &state,
        &pkce_challenge(&verifier),
        req.login_hint.as_deref().map(str::trim),
    );
    let t = now();
    let mut pending = st.oauth_pending.lock().expect("mutex");
    pending.retain(|_, p| p.expires > t);
    pending.insert(
        state,
        Pending {
            user: cu.user.id,
            provider,
            verifier,
            expires: t + PENDING_TTL,
        },
    );
    Ok(Json(OAuthUrl { url }))
}

/// Vuelta del proveedor. Redirige a la web con `?oauth=ok`, o con `?oauth_error=` y el motivo.
#[utoipa::path(get, path = "/api/v1/oauth/callback", tag = "accounts",
    params(("code" = Option<String>, Query, description = "Código del proveedor"),
        ("state" = Option<String>, Query, description = "El `state` de la autorización")),
    responses((status = 303, description = "Vuelve a la web")))]
pub async fn callback(
    State(st): State<Arc<AppState>>,
    cu: Result<CurrentUser, ApiError>,
    Query(q): Query<CallbackQuery>,
) -> Redirect {
    let Ok(cu) = cu else {
        return Redirect::to("/login");
    };
    match finish(&st, &cu, q).await {
        Ok(()) => Redirect::to(&format!("{BACK}?oauth=ok")),
        Err(e) => Redirect::to(&format!("{BACK}?oauth_error={}", urlencode(&e.message))),
    }
}

async fn finish(st: &AppState, cu: &CurrentUser, q: CallbackQuery) -> ApiResult<()> {
    // Solo vale para el usuario que la empezó: así nadie puede colar su cuenta en la de otro.
    let pending = q
        .state
        .and_then(|s| st.oauth_pending.lock().expect("mutex").remove(&s))
        .filter(|p| p.user == cu.user.id && p.expires > now())
        .ok_or_else(|| ApiError::bad_request("La autorización ha caducado o no es válida. Inténtalo otra vez."))?;
    if let Some(e) = q.error {
        return Err(ApiError::bad_request(if e == "access_denied" {
            "No se dio permiso para acceder al correo.".to_string()
        } else {
            format!("El proveedor respondió con un error: {e}.")
        }));
    }
    let code = q
        .code
        .ok_or_else(|| ApiError::bad_request("Falta el código del proveedor."))?;
    let app = st
        .oauth
        .get(&pending.provider)
        .ok_or_else(|| ApiError::bad_request("Ese proveedor ya no está configurado."))?;
    let base = st.public_url().await?;
    let tokens = app
        .exchange(&code, &redirect_uri(&base), &pending.verifier)
        .await
        .map_err(|e| ApiError::bad_request(format!("No se pudo conectar la cuenta: {e}.")))?;
    let refresh = tokens
        .refresh_token
        .clone()
        .ok_or_else(|| ApiError::bad_request("El proveedor no dio permiso de acceso continuo. Inténtalo otra vez."))?;
    let email = tokens
        .email()
        .ok_or_else(|| ApiError::bad_request("El proveedor no dijo la dirección de la cuenta."))?;
    let provider = pending.provider;
    let (imap_host, imap_port, imap_security) = provider.imap();
    let (smtp_host, smtp_port, smtp_security) = provider.smtp();

    if st.sync.enabled() {
        let cfg = ServerConfig {
            host: imap_host.into(),
            port: imap_port,
            security: Security::parse(imap_security).unwrap_or(Security::Tls),
            login: email.clone(),
            auth: Auth::XOAuth2(tokens.access_token.clone()),
            mech: Default::default(),
        };
        buzon_sync::check(&cfg)
            .await
            .map_err(|e| ApiError::bad_request(format!("No se pudo entrar en {imap_host}: {e}.")))?;
    }

    let user = cu.user.id;
    let udb = st.user_db(user).await?;
    let secrets = st.secrets_for(user);
    let acc = match udb.account_by_email(provider.as_str(), &email).await? {
        Some(acc) => {
            st.sync.stop(acc.id);
            udb.set_account_secret(acc.id, &refresh, &secrets).await?;
            st.db.audit(Some(user), "account.reconnected", Some(&email)).await?;
            acc
        }
        None => {
            let name = match provider {
                Provider::Google => "Gmail",
                Provider::Microsoft => "Outlook",
            };
            let acc = udb
                .add_account(
                    NewAccount {
                        name,
                        email: &email,
                        imap_host,
                        imap_port,
                        smtp_host,
                        smtp_port,
                        login: &email,
                        password: &refresh,
                        auth: provider.as_str(),
                        imap_security,
                        smtp_security,
                    },
                    &secrets,
                )
                .await?;
            st.db.audit(Some(user), "account.connected", Some(&email)).await?;
            acc
        }
    };
    st.start_account(user, &acc).await
}

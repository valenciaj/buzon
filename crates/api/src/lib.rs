//! API HTTP de Buzón (`/api/v1`), su especificación OpenAPI y el visor público.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{DefaultBodyLimit, Request};
use axum::http::{header, Method};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use buzon_store::app::User;
use buzon_store::crypto::{self, MasterKey, SecretBox};
use buzon_store::user::Account;
use buzon_store::{AppDb, Id, UserDb};
use buzon_sync::SyncHub;
use tower_http::services::{ServeDir, ServeFile};
use utoipa::OpenApi;
use utoipa_scalar::{Scalar, Servable};

pub mod error;
pub mod models;
mod prefs;
pub mod routes;
pub mod session;
mod validate;

use error::{ApiError, ApiResult};
use routes::{accounts, auth, compose, flow, invitations, mail, me, organize, setup};

pub struct AppState {
    pub db: AppDb,
    pub data_dir: PathBuf,
    pub master: MasterKey,
    /// Marca la cookie de sesión como `Secure`. Desactívalo solo en desarrollo sin HTTPS.
    pub cookie_secure: bool,
    /// URL pública de la configuración; si falta, se usa la guardada por el asistente.
    pub public_url: Option<String>,
    /// Sincronización IMAP. `AppState::new` la deja apagada; actívala con `with_sync`.
    pub sync: SyncHub,
    /// Permite cuentas de correo sin cifrar (solo para pruebas locales).
    pub allow_insecure_mail: bool,
    user_dbs: Mutex<HashMap<Id, UserDb>>,
}

impl AppState {
    pub fn new(
        db: AppDb,
        data_dir: PathBuf,
        master: MasterKey,
        cookie_secure: bool,
        public_url: Option<String>,
    ) -> Self {
        AppState {
            sync: SyncHub::new(data_dir.clone(), false),
            db,
            data_dir,
            master,
            cookie_secure,
            public_url,
            allow_insecure_mail: false,
            user_dbs: Mutex::new(HashMap::new()),
        }
    }

    /// Activa la sincronización IMAP de verdad.
    pub fn with_sync(mut self) -> Self {
        self.sync = SyncHub::new(self.data_dir.clone(), true);
        self
    }

    /// Empieza a sincronizar una cuenta.
    pub async fn start_account(&self, user: Id, account: &Account) -> ApiResult<()> {
        let udb = self.user_db(user).await?;
        let password = udb
            .account_password(account.id, &self.secrets_for(user))
            .await?
            .unwrap_or_default();
        self.sync.start(user, udb, account, password);
        Ok(())
    }

    /// Al arrancar el servidor: sincroniza las cuentas de todos los usuarios.
    pub async fn start_all_accounts(&self) -> ApiResult<usize> {
        let mut n = 0;
        for user in self.db.user_ids().await? {
            for acc in self.user_db(user).await?.accounts().await? {
                self.start_account(user, &acc).await?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// Base de datos del usuario (la crea la primera vez).
    pub async fn user_db(&self, user: Id) -> ApiResult<UserDb> {
        if let Some(db) = self.user_dbs.lock().expect("mutex").get(&user) {
            return Ok(db.clone());
        }
        let db = UserDb::open(&self.data_dir, user).await?;
        self.user_dbs.lock().expect("mutex").insert(user, db.clone());
        Ok(db)
    }

    /// Clave para cifrar los secretos de un usuario (contraseñas de sus cuentas).
    pub fn secrets_for(&self, user: Id) -> SecretBox {
        self.master.derive(&format!("user:{user}"))
    }

    pub fn totp_box(&self) -> SecretBox {
        self.master.derive("app:totp")
    }

    pub fn check_totp(&self, user: &User, code: &str) -> ApiResult<bool> {
        let Some(sealed) = &user.totp_secret else {
            return Ok(false);
        };
        let secret = self.totp_box().open(sealed)?;
        Ok(crypto::totp(&secret, &user.username)?
            .check_current(code)
            .unwrap_or(false))
    }

    pub async fn public_url(&self) -> ApiResult<String> {
        let url = match &self.public_url {
            Some(u) => Some(u.clone()),
            None => self.db.setting("public_url").await?,
        };
        Ok(url.unwrap_or_default().trim_end_matches('/').to_string())
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Buzón API",
        description = "API del webmail Buzón. La web usa esta misma API.\n\n\
            Autenticación: cookie de sesión `buzon_session` (se obtiene con `/api/v1/auth/login`). \
            Las peticiones que cambian datos deben enviar `Content-Type: application/json`.\n\n\
            Los identificadores son UUIDv7 codificados en Base58.",
        license(name = "MIT")
    ),
    paths(
        health,
        setup::status, setup::run,
        auth::login, auth::logout,
        me::get, me::update_preferences, me::totp_setup, me::totp_enable, me::totp_disable,
        accounts::list, accounts::create, accounts::remove, accounts::sync,
        mail::mailboxes, mail::threads, mail::thread, mail::update_thread, mail::attachment, mail::events,
        compose::identities, compose::create_identity, compose::update_identity, compose::delete_identity,
        compose::upload, compose::delete_upload,
        compose::drafts, compose::create_draft, compose::update_draft, compose::delete_draft,
        compose::send_mail, compose::contacts,
        organize::move_threads, organize::delete_threads, organize::flag_threads, organize::label_threads,
        organize::labels, organize::create_label, organize::update_label, organize::delete_label,
        organize::rules, organize::create_rule, organize::update_rule, organize::delete_rule,
        organize::book, organize::create_entry, organize::update_entry, organize::delete_entry, organize::import_vcard,
        flow::counts, flow::senders, flow::decide, flow::set_flow, flow::set_note,
        invitations::create, invitations::list, invitations::revoke, invitations::check, invitations::accept,
    ),
    tags(
        (name = "setup", description = "Asistente de primer arranque"),
        (name = "auth", description = "Inicio y cierre de sesión"),
        (name = "me", description = "Usuario actual, preferencias y 2FA"),
        (name = "accounts", description = "Cuentas de correo del usuario"),
        (name = "mail", description = "Carpetas, hilos, mensajes y avisos en vivo"),
        (name = "compose", description = "Redactar y enviar: identidades, firmas, borradores y adjuntos"),
        (name = "organize", description = "Mover y borrar hilos, etiquetas, filtros automáticos y contactos"),
        (name = "flow", description = "Modo Enfoque: filtro de remitentes, Principal/Novedades/Recibos, pilas, pospuestos y notas"),
        (name = "invitations", description = "Registro por invitación (caduca a las 72 h)"),
    )
)]
pub struct ApiDoc;

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct Health {
    pub status: &'static str,
    pub version: &'static str,
}

#[utoipa::path(get, path = "/api/v1/health", tag = "setup", responses((status = 200, body = Health)))]
pub async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// Rechaza peticiones que cambian datos sin `Content-Type: application/json`.
/// Un formulario de otro sitio no puede enviar ese tipo sin una comprobación CORS previa,
/// así que esto, junto a la cookie `SameSite=Lax`, cierra la puerta al CSRF.
/// Las subidas de ficheros llevan la cabecera propia `X-Buzon-Filename`, que también
/// obliga a esa comprobación previa.
async fn require_json(req: Request, next: Next) -> Response {
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS)
        || req.headers().contains_key("x-buzon-filename");
    let json = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    if safe || json {
        next.run(req).await
    } else {
        ApiError::new(
            axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "json_required",
            "Envía las peticiones con Content-Type: application/json.",
        )
        .into_response()
    }
}

pub fn api_router(state: Arc<AppState>) -> Router {
    let v1 = Router::new()
        .route("/health", get(health))
        .route("/setup", get(setup::status).post(setup::run))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(me::get))
        .route("/me/preferences", put(me::update_preferences))
        .route("/me/totp/setup", post(me::totp_setup))
        .route("/me/totp/enable", post(me::totp_enable))
        .route("/me/totp/disable", post(me::totp_disable))
        .route("/accounts", get(accounts::list).post(accounts::create))
        .route("/accounts/{id}", delete(accounts::remove))
        .route("/accounts/{id}/sync", post(accounts::sync))
        .route("/mailboxes", get(mail::mailboxes))
        .route("/threads", get(mail::threads))
        .route("/threads/{id}", get(mail::thread).patch(mail::update_thread))
        .route("/messages/{id}/attachments/{index}", get(mail::attachment))
        .route("/events", get(mail::events))
        .route("/identities", get(compose::identities).post(compose::create_identity))
        .route(
            "/identities/{id}",
            put(compose::update_identity).delete(compose::delete_identity),
        )
        .route(
            "/uploads",
            post(compose::upload).layer(DefaultBodyLimit::max(compose::MAX_UPLOAD)),
        )
        .route("/uploads/{id}", delete(compose::delete_upload))
        .route("/drafts", get(compose::drafts).post(compose::create_draft))
        .route("/drafts/{id}", put(compose::update_draft).delete(compose::delete_draft))
        .route("/send", post(compose::send_mail))
        .route("/contacts/suggest", get(compose::contacts))
        .route("/contacts", get(organize::book).post(organize::create_entry))
        .route("/contacts/import", post(organize::import_vcard))
        .route(
            "/contacts/{id}",
            put(organize::update_entry).delete(organize::delete_entry),
        )
        .route("/threads/move", post(organize::move_threads))
        .route("/threads/delete", post(organize::delete_threads))
        .route("/threads/flags", post(organize::flag_threads))
        .route("/threads/labels", post(organize::label_threads))
        .route("/threads/flow", post(flow::set_flow))
        .route("/threads/{id}/note", put(flow::set_note))
        .route("/flow", get(flow::counts))
        .route("/senders", get(flow::senders).put(flow::decide))
        .route("/labels", get(organize::labels).post(organize::create_label))
        .route(
            "/labels/{id}",
            put(organize::update_label).delete(organize::delete_label),
        )
        .route("/rules", get(organize::rules).post(organize::create_rule))
        .route("/rules/{id}", put(organize::update_rule).delete(organize::delete_rule))
        .route("/invitations", get(invitations::list).post(invitations::create))
        .route("/invitations/{id}", delete(invitations::revoke))
        .route("/invite/{token}", get(invitations::check).post(invitations::accept))
        .fallback(|| async { ApiError::not_found("Esa ruta de la API no existe.") })
        .layer(middleware::from_fn(require_json))
        .with_state(state);

    Router::new()
        .nest("/api/v1", v1)
        .route("/api/openapi.json", get(|| async { Json(ApiDoc::openapi()) }))
        .merge(Scalar::with_url("/api/docs", ApiDoc::openapi()))
}

/// Router completo: API, documentación y, si se indica, el frontend compilado.
pub fn app(state: Arc<AppState>, web_dir: Option<PathBuf>) -> Router {
    let router = api_router(state);
    match web_dir {
        Some(dir) => {
            let index = ServeFile::new(dir.join("index.html"));
            router.fallback_service(ServeDir::new(dir).fallback(index))
        }
        None => router,
    }
}

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
use buzon_sync::{Creds, OAuthApp, Provider, SyncHub};
use tower_http::services::{ServeDir, ServeFile};
use utoipa::OpenApi;
use utoipa_scalar::{Scalar, Servable};

pub mod error;
pub mod hooks;
pub mod models;
mod prefs;
pub mod routes;
mod scheduler;
pub mod session;
mod validate;

use error::{ApiError, ApiResult};
use routes::{accounts, auth, compose, extras, flow, invitations, mail, me, oauth, organize, setup, tokens, webhooks};

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
    /// Permite webhooks a la red local y sin HTTPS.
    pub allow_private_webhooks: bool,
    /// Despierta al repartidor de webhooks cuando hay envíos nuevos en la cola.
    pub hooks_ready: tokio::sync::Notify,
    /// Despierta al repartidor de envíos programados.
    pub sends_ready: tokio::sync::Notify,
    /// Aplicaciones OAuth2 configuradas (Gmail, Outlook).
    pub oauth: HashMap<Provider, Arc<OAuthApp>>,
    /// Autorizaciones de OAuth2 en curso, por su `state`.
    pub(crate) oauth_pending: Mutex<HashMap<String, routes::oauth::Pending>>,
    /// Correo nuevo, para las notificaciones del navegador: (usuario, datos).
    pub notices: tokio::sync::broadcast::Sender<(Id, serde_json::Value)>,
    user_dbs: Mutex<HashMap<Id, UserDb>>,
    opening: tokio::sync::Mutex<()>,
    /// Peticiones de cada token en el minuto en curso.
    rate: Mutex<HashMap<Id, (i64, u32)>>,
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
            allow_private_webhooks: false,
            hooks_ready: tokio::sync::Notify::new(),
            sends_ready: tokio::sync::Notify::new(),
            notices: tokio::sync::broadcast::channel(256).0,
            oauth: HashMap::new(),
            oauth_pending: Mutex::new(HashMap::new()),
            user_dbs: Mutex::new(HashMap::new()),
            opening: tokio::sync::Mutex::new(()),
            rate: Mutex::new(HashMap::new()),
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
        match self.creds(user, account).await {
            Ok(creds) => self.sync.start(user, udb, account, creds),
            Err(e) => {
                tracing::warn!("cuenta {}: {}", account.id, e.message);
                let _ = udb.set_sync_result(account.id, Some(&e.message)).await;
            }
        }
        Ok(())
    }

    /// Con qué se entra en el correo de la cuenta: su contraseña o su app de OAuth2.
    pub async fn creds(&self, user: Id, account: &Account) -> ApiResult<Creds> {
        let secrets = self.secrets_for(user);
        if account.auth == "password" {
            let udb = self.user_db(user).await?;
            let password = udb.account_password(account.id, &secrets).await?.unwrap_or_default();
            return Ok(Creds::Password(password));
        }
        let app = Provider::parse(&account.auth)
            .and_then(|p| self.oauth.get(&p))
            .ok_or_else(|| {
                ApiError::bad_request(format!(
                    "El servidor ya no tiene configurado el acceso con {}. Avisa al administrador.",
                    account.auth
                ))
            })?;
        Ok(Creds::OAuth {
            app: app.clone(),
            secrets: Arc::new(secrets),
        })
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
        let cached = || self.user_dbs.lock().expect("mutex").get(&user).cloned();
        if let Some(db) = cached() {
            return Ok(db);
        }
        // Una apertura cada vez: dos a la vez aplicarían las migraciones dos veces.
        let _guard = self.opening.lock().await;
        if let Some(db) = cached() {
            return Ok(db);
        }
        let db = UserDb::open(&self.data_dir, user).await?;
        self.user_dbs.lock().expect("mutex").insert(user, db.clone());
        Ok(db)
    }

    /// Clave para cifrar los secretos de un usuario (contraseñas de sus cuentas).
    pub fn secrets_for(&self, user: Id) -> SecretBox {
        self.master.derive(&format!("user:{user}"))
    }

    /// Clave para cifrar los secretos de firma de los webhooks.
    pub fn hooks_box(&self) -> SecretBox {
        self.master.derive("app:webhooks")
    }

    /// Cuenta una petición del token. `false` si ya ha pasado del límite en este minuto.
    pub fn rate_ok(&self, token: Id, now: i64) -> bool {
        let minute = now / 60;
        let mut rate = self.rate.lock().expect("mutex");
        rate.retain(|_, (m, _)| *m == minute);
        let (_, n) = rate.entry(token).or_insert((minute, 0));
        *n += 1;
        *n <= session::RATE_LIMIT
    }

    /// Avisa a los webhooks del usuario. Nunca falla: un webhook no debe romper nada.
    pub async fn emit_hook(&self, user: Id, event: &str, data: serde_json::Value) {
        match self.db.enqueue_event(user, event, &data.to_string(), None).await {
            Ok(0) => {}
            Ok(_) => self.hooks_ready.notify_one(),
            Err(e) => tracing::warn!("webhook {event}: {e}"),
        }
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
            ## Autenticación\n\n\
            - **Token personal** (para scripts y otras apps): créalo en Ajustes → API y envíalo como \
            `Authorization: Bearer bz_…`. Cada token tiene permisos: `mail:read` (leer), `mail:write` \
            (mover, marcar, etiquetar, filtros), `mail:send` (borradores y envío), `contacts` y `webhooks`. \
            La cuenta, la seguridad, los tokens y las invitaciones solo se gestionan desde la web. \
            Límite: 300 peticiones por minuto por token (responde 429).\n            - **Cookie de sesión** `buzon_session` (la usa la web; se obtiene con `/api/v1/auth/login`).\n\n\
            Las peticiones que cambian datos deben enviar `Content-Type: application/json`. \
            Los identificadores son UUIDv7 codificados en Base58.\n\n\
            ## Webhooks\n\n\
            Buzón hace `POST` con un JSON `{id, event, created_at, data}` y estas cabeceras: \
            `Buzon-Event`, `Buzon-Delivery` (id del envío) y `Buzon-Signature: t=<segundos>,v1=<firma>`. \
            La firma es HMAC-SHA256 en hexadecimal, con el secreto del webhook, de `<t>.<cuerpo>`. \
            Comprueba la firma y rechaza un `t` de hace más de 5 minutos. \
            Responde con un código 2xx en menos de 10 s; si no, se reintenta a los 1 min, 5 min, 30 min, \
            2 h, 6 h y 15 h. El `id` del cuerpo es el mismo en los reintentos y reenvíos.\n\n\
            Eventos: `message.received`, `message.sent`, `message.read`, `message.moved`, \
            `thread.labeled`, `contact.screened`, `account.sync_failed` y `ping` (prueba).",
        license(name = "MIT")
    ),
    paths(
        health,
        setup::status, setup::run,
        auth::login, auth::logout,
        me::get, me::update_preferences, me::totp_setup, me::totp_enable, me::totp_disable,
        accounts::list, accounts::create, accounts::remove, accounts::sync,
        oauth::providers, oauth::start, oauth::callback,
        mail::mailboxes, mail::threads, mail::thread, mail::update_thread, mail::attachment, mail::events,
        compose::identities, compose::create_identity, compose::update_identity, compose::delete_identity,
        compose::upload, compose::delete_upload,
        compose::drafts, compose::create_draft, compose::update_draft, compose::delete_draft,
        compose::send_mail, compose::contacts,
        extras::scheduled, extras::cancel, extras::followups, extras::dismiss,
        organize::move_threads, organize::delete_threads, organize::flag_threads, organize::label_threads,
        organize::labels, organize::create_label, organize::update_label, organize::delete_label,
        organize::rules, organize::create_rule, organize::update_rule, organize::delete_rule,
        organize::book, organize::create_entry, organize::update_entry, organize::delete_entry, organize::import_vcard,
        flow::counts, flow::senders, flow::decide, flow::set_flow, flow::set_note,
        tokens::list, tokens::create, tokens::revoke,
        webhooks::list, webhooks::create, webhooks::update, webhooks::remove, webhooks::test,
        webhooks::deliveries, webhooks::redeliver,
        invitations::create, invitations::list, invitations::revoke, invitations::check, invitations::accept,
    ),
    tags(
        (name = "setup", description = "Asistente de primer arranque"),
        (name = "auth", description = "Inicio y cierre de sesión"),
        (name = "me", description = "Usuario actual, preferencias y 2FA"),
        (name = "accounts", description = "Cuentas de correo del usuario, con contraseña o con OAuth2 (Gmail, Outlook)"),
        (name = "mail", description = "Carpetas, hilos, mensajes y avisos en vivo"),
        (name = "compose", description = "Redactar y enviar: identidades, firmas, borradores y adjuntos"),
        (name = "organize", description = "Mover y borrar hilos, etiquetas, filtros automáticos y contactos"),
        (name = "flow", description = "Modo Enfoque: filtro de remitentes, Principal/Novedades/Recibos, pilas, pospuestos y notas"),
        (name = "tokens", description = "Tokens personales de la API (solo desde la web)"),
        (name = "webhooks", description = "Avisos firmados a otras apps. Ver la descripción general para comprobar la firma"),
        (name = "invitations", description = "Registro por invitación (caduca a las 72 h)"),
    ),
    modifiers(&BearerAuth)
)]
pub struct ApiDoc;

struct BearerAuth;

impl utoipa::Modify for BearerAuth {
    fn modify(&self, api: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{
            ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityRequirement, SecurityScheme,
        };
        let c = api.components.get_or_insert_with(Default::default);
        c.add_security_scheme(
            "token",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("bz_…")
                    .build(),
            ),
        );
        c.add_security_scheme(
            "session",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("buzon_session"))),
        );
        api.security = Some(vec![
            SecurityRequirement::new("token", Vec::<String>::new()),
            SecurityRequirement::new("session", Vec::<String>::new()),
        ]);
    }
}

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
        .route("/oauth/providers", get(oauth::providers))
        .route("/oauth/{provider}/start", post(oauth::start))
        .route("/oauth/callback", get(oauth::callback))
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
        .route("/scheduled", get(extras::scheduled))
        .route("/scheduled/{id}/cancel", post(extras::cancel))
        .route("/followups", get(extras::followups))
        .route("/followups/{id}", delete(extras::dismiss))
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
        .route("/tokens", get(tokens::list).post(tokens::create))
        .route("/tokens/{id}", delete(tokens::revoke))
        .route("/webhooks", get(webhooks::list).post(webhooks::create))
        .route("/webhooks/{id}", put(webhooks::update).delete(webhooks::remove))
        .route("/webhooks/{id}/test", post(webhooks::test))
        .route("/webhooks/{id}/deliveries", get(webhooks::deliveries))
        .route("/webhooks/deliveries/{id}/redeliver", post(webhooks::redeliver))
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

/// Arranca las tareas de fondo: webhooks y envíos programados.
pub fn start_background(state: Arc<AppState>) {
    hooks::start(state.clone());
    scheduler::start(state);
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

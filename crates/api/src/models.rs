//! Tipos de entrada y salida de la API (con su esquema OpenAPI).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use buzon_store::app::User;
use buzon_store::Id;

#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// `focus` es el modo Enfoque; `classic`, el modo Clásico.
#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Focus,
    Classic,
}

/// Todas las cuentas combinadas o una cuenta cada vez.
#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccountView {
    #[default]
    Unified,
    PerAccount,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Font {
    #[default]
    Atkinson,
    Inter,
    SourceSans,
    Plex,
    Lexend,
    System,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    #[default]
    Comfortable,
    Compact,
}

/// Preferencias de interfaz de un usuario. Se guardan en su propia base de datos.
#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, PartialEq)]
pub struct Preferences {
    pub theme: Theme,
    pub mode: Mode,
    pub view: AccountView,
    pub font: Font,
    /// Tamaño del texto en %, de 87.5 a 150 en pasos de 12.5.
    pub font_size: f64,
    pub density: Density,
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            theme: Theme::default(),
            mode: Mode::default(),
            view: AccountView::default(),
            font: Font::default(),
            font_size: 100.0,
            density: Density::default(),
        }
    }
}

/// Cambio parcial de preferencias: solo se aplican los campos presentes.
#[derive(Deserialize, ToSchema, Default, Debug)]
pub struct PreferencesUpdate {
    pub theme: Option<Theme>,
    pub mode: Option<Mode>,
    pub view: Option<AccountView>,
    pub font: Option<Font>,
    pub font_size: Option<f64>,
    pub density: Option<Density>,
    /// Idioma de la interfaz: `es` o `en`. Se guarda en el usuario.
    pub language: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct Me {
    #[schema(value_type = String, example = "1C4xWbQq9k3vT7hF2mYp8z")]
    pub id: Id,
    pub username: String,
    pub email: Option<String>,
    pub is_admin: bool,
    pub totp_enabled: bool,
    pub language: String,
    pub preferences: Preferences,
}

impl Me {
    pub fn new(u: &User, preferences: Preferences) -> Self {
        Me {
            id: u.id,
            username: u.username.clone(),
            email: u.email.clone(),
            is_admin: u.is_admin(),
            totp_enabled: u.totp_enabled,
            language: u.language.clone(),
            preferences,
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    /// Código de 6 cifras de la app de autenticación, si el usuario tiene 2FA.
    pub totp: Option<String>,
}

/// Datos de una cuenta de correo. Este usuario y contraseña son los del
/// servidor de correo, no los de Buzón.
#[derive(Deserialize, ToSchema)]
pub struct AccountInput {
    pub name: String,
    pub email: String,
    pub imap_host: String,
    #[schema(example = 993)]
    pub imap_port: u16,
    pub smtp_host: String,
    #[schema(example = 465)]
    pub smtp_port: u16,
    pub login: String,
    pub password: String,
    /// Cifrado IMAP: `tls` (por defecto, puerto 993), `starttls` (143) o `none`.
    #[serde(default = "tls")]
    pub imap_security: String,
    /// Cifrado SMTP: `tls` (por defecto, puerto 465), `starttls` (587) o `none`.
    #[serde(default = "tls")]
    pub smtp_security: String,
    /// Comprobar antes de guardar que el servidor IMAP acepta el usuario y la contraseña.
    #[serde(default = "yes")]
    pub verify: bool,
}

fn tls() -> String {
    "tls".into()
}

fn yes() -> bool {
    true
}

#[derive(Serialize, ToSchema)]
pub struct AccountOut {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub login: String,
    pub imap_security: String,
    pub smtp_security: String,
    /// Estado de la sincronización: `off`, `connecting`, `syncing`, `idle` o `error`.
    pub state: String,
    /// Última sincronización completa (segundos Unix).
    pub synced_at: Option<i64>,
    /// Último error al sincronizar, si lo hubo.
    pub sync_error: Option<String>,
}

impl AccountOut {
    pub fn new(a: buzon_store::user::Account, state: buzon_sync::State) -> Self {
        let state = serde_json::to_value(state)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        AccountOut {
            id: a.id,
            name: a.name,
            email: a.email,
            imap_host: a.imap_host,
            imap_port: a.imap_port,
            smtp_host: a.smtp_host,
            smtp_port: a.smtp_port,
            login: a.login,
            imap_security: a.imap_security,
            smtp_security: a.smtp_security,
            state,
            synced_at: a.synced_at,
            sync_error: a.sync_error,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct AddrOut {
    pub name: Option<String>,
    pub email: String,
}

impl From<buzon_store::mail::Addr> for AddrOut {
    fn from(a: buzon_store::mail::Addr) -> Self {
        AddrOut {
            name: a.name,
            email: a.email,
        }
    }
}

/// Carpeta IMAP de una cuenta.
#[derive(Serialize, ToSchema)]
pub struct MailboxOut {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub account_id: Id,
    /// Nombre completo y legible, por ejemplo `Proyectos/2026`.
    pub name: String,
    /// Separador de niveles (`/` o `.`).
    pub delimiter: Option<String>,
    /// `inbox`, `sent`, `drafts`, `archive`, `junk`, `trash` o nada.
    pub role: Option<String>,
    pub total: i64,
    pub unread: i64,
}

/// Resumen de un hilo para la lista.
#[derive(Serialize, ToSchema)]
pub struct ThreadOut {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub account_id: Id,
    pub subject: String,
    /// Fecha del último mensaje (segundos Unix).
    pub last_date: i64,
    /// Remitentes, del más antiguo al más reciente.
    pub participants: Vec<String>,
    pub snippet: String,
    pub messages: i64,
    pub unread: i64,
    pub flagged: bool,
    pub has_attachments: bool,
}

impl From<buzon_store::mail::ThreadSummary> for ThreadOut {
    fn from(t: buzon_store::mail::ThreadSummary) -> Self {
        ThreadOut {
            id: t.id,
            account_id: t.account_id,
            subject: t.subject,
            last_date: t.last_date,
            participants: t.participants,
            snippet: t.snippet,
            messages: t.messages,
            unread: t.unread,
            flagged: t.flagged,
            has_attachments: t.has_attachments,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct ThreadPage {
    pub threads: Vec<ThreadOut>,
    /// Pásalo como `cursor` para la página siguiente. Falta si no hay más.
    pub next_cursor: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct AttachmentOut {
    pub index: u32,
    pub name: String,
    pub content_type: String,
    pub size: u64,
    /// Imagen incrustada en el HTML del mensaje.
    pub inline: bool,
    pub url: String,
}

#[derive(Serialize, ToSchema)]
pub struct MessageOut {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub mailbox_id: Id,
    pub subject: String,
    pub from: Option<AddrOut>,
    pub to: Vec<AddrOut>,
    pub cc: Vec<AddrOut>,
    pub date: i64,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub text: Option<String>,
    /// HTML ya saneado. Las imágenes remotas siguen en él: muéstralo con una CSP que
    /// las bloquee hasta que la persona las pida.
    pub html: Option<String>,
    pub remote_images: u32,
    pub attachments: Vec<AttachmentOut>,
}

#[derive(Serialize, ToSchema)]
pub struct ThreadDetail {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub account_id: Id,
    pub subject: String,
    pub messages: Vec<MessageOut>,
}

/// Cambios en un hilo. `seen` afecta a todos sus mensajes; `flagged: true` destaca el último.
#[derive(Deserialize, ToSchema)]
pub struct ThreadUpdate {
    pub seen: Option<bool>,
    pub flagged: Option<bool>,
}

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
}

impl From<buzon_store::user::Account> for AccountOut {
    fn from(a: buzon_store::user::Account) -> Self {
        AccountOut {
            id: a.id,
            name: a.name,
            email: a.email,
            imap_host: a.imap_host,
            imap_port: a.imap_port,
            smtp_host: a.smtp_host,
            smtp_port: a.smtp_port,
            login: a.login,
        }
    }
}

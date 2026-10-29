//! Almacenamiento de Buzón: la base de datos de la aplicación (`app.db`)
//! y una base de datos SQLite por usuario (`users/<id>.db`).

pub mod app;
pub mod backup;
pub mod compose;
pub mod crypto;
pub mod extras;
pub mod flow;
pub mod hooks;
pub mod id;
pub mod mail;
pub mod organize;
pub mod tokens;
pub mod user;

pub use app::AppDb;
pub use id::Id;
pub use user::UserDb;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("base de datos: {0}")]
    Db(#[from] sqlx::Error),
    #[error("migración: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("entrada/salida: {0}")]
    Io(#[from] std::io::Error),
    #[error("cifrado: {0}")]
    Crypto(&'static str),
    #[error("identificador no válido")]
    BadId,
    #[error("ya existe")]
    Duplicate,
    /// La cuenta no tiene una carpeta de este tipo (archive, trash, junk…).
    #[error("la cuenta no tiene carpeta «{0}»")]
    MissingFolder(String),
}

pub(crate) fn map_unique(e: sqlx::Error) -> Error {
    match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => Error::Duplicate,
        _ => Error::Db(e),
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Segundos desde la época Unix.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

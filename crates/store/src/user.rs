use std::path::{Path, PathBuf};

use sqlx::{Row, SqlitePool};

use crate::app::open_pool;
use crate::crypto::SecretBox;
use crate::{now, Id, Result};

static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/user");

/// Ruta de la base de datos de un usuario dentro de la carpeta de datos.
pub fn user_db_path(data_dir: &Path, user: Id) -> PathBuf {
    data_dir.join("users").join(format!("{}.db", user.to_base58()))
}

#[derive(Debug, Clone)]
pub struct Account {
    pub id: Id,
    pub name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub login: String,
    pub created_at: i64,
}

pub struct NewAccount<'a> {
    pub name: &'a str,
    pub email: &'a str,
    pub imap_host: &'a str,
    pub imap_port: u16,
    pub smtp_host: &'a str,
    pub smtp_port: u16,
    pub login: &'a str,
    pub password: &'a str,
}

/// Base de datos aislada de un usuario: preferencias y cuentas de correo.
#[derive(Clone)]
pub struct UserDb {
    pool: SqlitePool,
}

impl UserDb {
    /// Abre (o crea) la base de datos del usuario y aplica sus migraciones.
    pub async fn open(data_dir: &Path, user: Id) -> Result<Self> {
        let pool = open_pool(&user_db_path(data_dir, user)).await?;
        MIGRATIONS.run(&pool).await?;
        Ok(UserDb { pool })
    }

    pub async fn prefs(&self) -> Result<Vec<(String, String)>> {
        let rows = sqlx::query("SELECT key, value FROM prefs ORDER BY key")
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| Ok((r.try_get("key")?, r.try_get("value")?)))
            .collect()
    }

    pub async fn set_pref(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO prefs (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn add_account(&self, a: NewAccount<'_>, secrets: &SecretBox) -> Result<Account> {
        let id = Id::new();
        let t = now();
        sqlx::query(
            "INSERT INTO accounts (id, name, email, imap_host, imap_port, smtp_host, smtp_port, login, secret, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_bytes().as_slice())
        .bind(a.name)
        .bind(a.email)
        .bind(a.imap_host)
        .bind(a.imap_port)
        .bind(a.smtp_host)
        .bind(a.smtp_port)
        .bind(a.login)
        .bind(secrets.seal(a.password.as_bytes())?)
        .bind(t)
        .execute(&self.pool)
        .await?;
        Ok(Account {
            id,
            name: a.name.into(),
            email: a.email.into(),
            imap_host: a.imap_host.into(),
            imap_port: a.imap_port,
            smtp_host: a.smtp_host.into(),
            smtp_port: a.smtp_port,
            login: a.login.into(),
            created_at: t,
        })
    }

    pub async fn accounts(&self) -> Result<Vec<Account>> {
        let rows = sqlx::query(
            "SELECT id, name, email, imap_host, imap_port, smtp_host, smtp_port, login, created_at FROM accounts ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(Account {
                    id: Id::from_slice(r.try_get::<&[u8], _>("id")?)?,
                    name: r.try_get("name")?,
                    email: r.try_get("email")?,
                    imap_host: r.try_get("imap_host")?,
                    imap_port: r.try_get("imap_port")?,
                    smtp_host: r.try_get("smtp_host")?,
                    smtp_port: r.try_get("smtp_port")?,
                    login: r.try_get("login")?,
                    created_at: r.try_get("created_at")?,
                })
            })
            .collect()
    }

    /// Contraseña IMAP/SMTP descifrada, para la sincronización.
    pub async fn account_password(&self, id: Id, secrets: &SecretBox) -> Result<Option<String>> {
        let sealed: Option<Vec<u8>> = sqlx::query_scalar("SELECT secret FROM accounts WHERE id = ?")
            .bind(id.as_bytes().as_slice())
            .fetch_optional(&self.pool)
            .await?;
        sealed
            .map(|s| secrets.open(&s).map(|p| String::from_utf8_lossy(&p).into_owned()))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::MasterKey;

    #[tokio::test]
    async fn each_user_gets_own_file_and_encrypted_secret() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (Id::new(), Id::new());
        let db_a = UserDb::open(dir.path(), a).await.unwrap();
        UserDb::open(dir.path(), b).await.unwrap();
        assert!(user_db_path(dir.path(), a).exists());
        assert!(user_db_path(dir.path(), b).exists());

        let key = MasterKey::generate().derive(&format!("user:{a}"));
        let acc = db_a
            .add_account(
                NewAccount {
                    name: "Personal",
                    email: "yo@ejemplo.es",
                    imap_host: "imap.ejemplo.es",
                    imap_port: 993,
                    smtp_host: "smtp.ejemplo.es",
                    smtp_port: 465,
                    login: "yo@ejemplo.es",
                    password: "secreto",
                },
                &key,
            )
            .await
            .unwrap();
        assert_eq!(db_a.accounts().await.unwrap().len(), 1);
        assert_eq!(
            db_a.account_password(acc.id, &key).await.unwrap().as_deref(),
            Some("secreto")
        );
        let raw: Vec<u8> = sqlx::query_scalar("SELECT secret FROM accounts")
            .fetch_one(&db_a.pool)
            .await
            .unwrap();
        assert!(!raw.windows(7).any(|w| w == b"secreto"));
    }
}

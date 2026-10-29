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
    /// `tls`, `starttls` o `none`.
    pub imap_security: String,
    pub smtp_security: String,
    /// `password`, o el proveedor de OAuth2: `google` o `microsoft`.
    pub auth: String,
    /// Con contraseña, cómo se manda: `auto`, `plain` o `login`.
    pub auth_mech: String,
    pub created_at: i64,
    /// Última sincronización completa sin errores.
    pub synced_at: Option<i64>,
    pub sync_error: Option<String>,
}

pub struct NewAccount<'a> {
    pub name: &'a str,
    pub email: &'a str,
    pub imap_host: &'a str,
    pub imap_port: u16,
    pub smtp_host: &'a str,
    pub smtp_port: u16,
    pub login: &'a str,
    /// Contraseña, o token de renovación si `auth` es un proveedor de OAuth2.
    pub password: &'a str,
    pub auth: &'a str,
    pub imap_security: &'a str,
    pub smtp_security: &'a str,
}

/// Carpeta de los mensajes originales (`.eml`) de un usuario.
pub fn user_raw_dir(data_dir: &Path, user: Id) -> PathBuf {
    data_dir.join("users").join(user.to_base58()).join("raw")
}

const ACCOUNT_COLS: &str = "id, name, email, imap_host, imap_port, smtp_host, smtp_port, login, \
    imap_security, smtp_security, auth, auth_mech, created_at, synced_at, sync_error";

fn account_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Account> {
    Ok(Account {
        id: Id::from_slice(r.try_get::<&[u8], _>("id")?)?,
        name: r.try_get("name")?,
        email: r.try_get("email")?,
        imap_host: r.try_get("imap_host")?,
        imap_port: r.try_get("imap_port")?,
        smtp_host: r.try_get("smtp_host")?,
        smtp_port: r.try_get("smtp_port")?,
        login: r.try_get("login")?,
        imap_security: r.try_get("imap_security")?,
        smtp_security: r.try_get("smtp_security")?,
        auth: r.try_get("auth")?,
        auth_mech: r.try_get("auth_mech")?,
        created_at: r.try_get("created_at")?,
        synced_at: r.try_get("synced_at")?,
        sync_error: r.try_get("sync_error")?,
    })
}

/// Base de datos aislada de un usuario: preferencias y cuentas de correo.
#[derive(Clone)]
pub struct UserDb {
    pub(crate) pool: SqlitePool,
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
            "INSERT INTO accounts (id, name, email, imap_host, imap_port, smtp_host, smtp_port, login, secret, \
             imap_security, smtp_security, auth, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        .bind(a.imap_security)
        .bind(a.smtp_security)
        .bind(a.auth)
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
            imap_security: a.imap_security.into(),
            smtp_security: a.smtp_security.into(),
            auth: a.auth.into(),
            auth_mech: "auto".into(),
            created_at: t,
            synced_at: None,
            sync_error: None,
        })
    }

    pub async fn accounts(&self) -> Result<Vec<Account>> {
        let rows = sqlx::query(&format!("SELECT {ACCOUNT_COLS} FROM accounts ORDER BY id"))
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(account_from_row).collect()
    }

    pub async fn account(&self, id: Id) -> Result<Option<Account>> {
        let row = sqlx::query(&format!("SELECT {ACCOUNT_COLS} FROM accounts WHERE id = ?"))
            .bind(id.as_bytes().as_slice())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(account_from_row).transpose()
    }

    /// Borra la cuenta y todo su correo en caché. Devuelve los mensajes borrados.
    pub async fn delete_account(&self, id: Id) -> Result<Vec<Id>> {
        let mut tx = self.pool.begin().await?;
        let ids: Vec<Vec<u8>> = sqlx::query_scalar("SELECT id FROM messages WHERE account_id = ?")
            .bind(id.as_bytes().as_slice())
            .fetch_all(&mut *tx)
            .await?;
        for table in ["messages", "threads", "mailboxes"] {
            sqlx::query(&format!("DELETE FROM {table} WHERE account_id = ?"))
                .bind(id.as_bytes().as_slice())
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("DELETE FROM accounts WHERE id = ?")
            .bind(id.as_bytes().as_slice())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        ids.iter().map(|b| Id::from_slice(b)).collect()
    }

    /// Guarda el resultado de la última sincronización: `None` si fue bien.
    pub async fn set_sync_result(&self, id: Id, error: Option<&str>) -> Result<()> {
        let q = match error {
            None => sqlx::query("UPDATE accounts SET synced_at = ?, sync_error = NULL WHERE id = ?").bind(now()),
            Some(e) => sqlx::query("UPDATE accounts SET sync_error = ? WHERE id = ?").bind(e),
        };
        q.bind(id.as_bytes().as_slice()).execute(&self.pool).await?;
        Ok(())
    }

    /// Cambia el secreto guardado: contraseña o token de renovación.
    /// Cambia los datos de una cuenta. Con `password` vacía se queda la que había; `auth` no cambia.
    pub async fn update_account(&self, id: Id, a: NewAccount<'_>, secrets: &SecretBox) -> Result<()> {
        sqlx::query(
            "UPDATE accounts SET name = ?, email = ?, imap_host = ?, imap_port = ?, smtp_host = ?, smtp_port = ?, \
             login = ?, imap_security = ?, smtp_security = ?, sync_error = NULL WHERE id = ?",
        )
        .bind(a.name)
        .bind(a.email)
        .bind(a.imap_host)
        .bind(a.imap_port)
        .bind(a.smtp_host)
        .bind(a.smtp_port)
        .bind(a.login)
        .bind(a.imap_security)
        .bind(a.smtp_security)
        .bind(id.as_bytes().as_slice())
        .execute(&self.pool)
        .await?;
        if !a.password.is_empty() {
            self.set_account_secret(id, a.password, secrets).await?;
        }
        Ok(())
    }

    /// Cómo se manda la contraseña: `auto`, `plain` o `login`.
    pub async fn set_account_mech(&self, id: Id, mech: &str) -> Result<()> {
        sqlx::query("UPDATE accounts SET auth_mech = ? WHERE id = ?")
            .bind(mech)
            .bind(id.as_bytes().as_slice())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_account_secret(&self, id: Id, plain: &str, secrets: &SecretBox) -> Result<()> {
        sqlx::query("UPDATE accounts SET secret = ? WHERE id = ?")
            .bind(secrets.seal(plain.as_bytes())?)
            .bind(id.as_bytes().as_slice())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Cuenta de OAuth2 ya conectada con esa dirección, para volver a conectarla en vez de duplicarla.
    pub async fn account_by_email(&self, auth: &str, email: &str) -> Result<Option<Account>> {
        let row = sqlx::query(&format!(
            "SELECT {ACCOUNT_COLS} FROM accounts WHERE auth = ? AND lower(email) = lower(?)"
        ))
        .bind(auth)
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(account_from_row).transpose()
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
                    auth: "password",
                    imap_security: "tls",
                    smtp_security: "tls",
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

        // El secreto se puede cambiar (token de renovación nuevo) y sigue cifrado.
        assert_eq!(acc.auth, "password");
        db_a.set_account_secret(acc.id, "otro", &key).await.unwrap();
        assert_eq!(
            db_a.account_password(acc.id, &key).await.unwrap().as_deref(),
            Some("otro")
        );
        assert!(db_a
            .account_by_email("google", "YO@ejemplo.es")
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            db_a.account_by_email("password", "YO@ejemplo.es")
                .await
                .unwrap()
                .map(|a| a.id),
            Some(acc.id)
        );
    }
}

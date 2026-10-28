use std::path::Path;
use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{Row, SqlitePool};

use crate::{map_unique, now, Id, Result};

static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/app");

pub(crate) async fn open_pool(path: &Path) -> Result<SqlitePool> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);
    Ok(SqlitePoolOptions::new().max_connections(8).connect_with(opts).await?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Admin,
    User,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
        }
    }
}

#[derive(Debug, Clone)]
pub struct User {
    pub id: Id,
    pub username: String,
    pub email: Option<String>,
    pub password_hash: String,
    pub role: Role,
    pub totp_secret: Option<Vec<u8>>,
    pub totp_enabled: bool,
    pub failed_logins: i64,
    pub locked_until: Option<i64>,
    pub language: String,
    pub created_at: i64,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }
}

pub struct NewUser<'a> {
    pub username: &'a str,
    pub email: Option<&'a str>,
    pub password_hash: &'a str,
    pub role: Role,
    pub language: &'a str,
}

#[derive(Debug, Clone)]
pub struct Invitation {
    pub id: Id,
    pub note: Option<String>,
    pub created_by: Id,
    pub created_at: i64,
    pub expires_at: i64,
    pub used_at: Option<i64>,
}

fn user_from_row(r: &SqliteRow) -> Result<User> {
    Ok(User {
        id: Id::from_slice(r.try_get::<&[u8], _>("id")?)?,
        username: r.try_get("username")?,
        email: r.try_get("email")?,
        password_hash: r.try_get("password_hash")?,
        role: if r.try_get::<&str, _>("role")? == "admin" {
            Role::Admin
        } else {
            Role::User
        },
        totp_secret: r.try_get("totp_secret")?,
        totp_enabled: r.try_get("totp_enabled")?,
        failed_logins: r.try_get("failed_logins")?,
        locked_until: r.try_get("locked_until")?,
        language: r.try_get("language")?,
        created_at: r.try_get("created_at")?,
    })
}

fn invitation_from_row(r: &SqliteRow) -> Result<Invitation> {
    Ok(Invitation {
        id: Id::from_slice(r.try_get::<&[u8], _>("id")?)?,
        note: r.try_get("note")?,
        created_by: Id::from_slice(r.try_get::<&[u8], _>("created_by")?)?,
        created_at: r.try_get("created_at")?,
        expires_at: r.try_get("expires_at")?,
        used_at: r.try_get("used_at")?,
    })
}

const USER_COLS: &str = "id, username, email, password_hash, role, totp_secret, totp_enabled, \
                         failed_logins, locked_until, language, created_at";

/// Base de datos global de la aplicación.
#[derive(Clone)]
pub struct AppDb {
    pool: SqlitePool,
}

impl AppDb {
    pub async fn open(path: &Path) -> Result<Self> {
        let pool = open_pool(path).await?;
        MIGRATIONS.run(&pool).await?;
        Ok(AppDb { pool })
    }

    pub async fn user_count(&self) -> Result<i64> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn create_user(&self, u: NewUser<'_>) -> Result<User> {
        let id = Id::new();
        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, role, language, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_bytes().as_slice())
        .bind(u.username)
        .bind(u.email)
        .bind(u.password_hash)
        .bind(u.role.as_str())
        .bind(u.language)
        .bind(now())
        .execute(&self.pool)
        .await
        .map_err(map_unique)?;
        Ok(self.user_by_id(id).await?.expect("usuario recién creado"))
    }

    pub async fn user_by_id(&self, id: Id) -> Result<Option<User>> {
        let row = sqlx::query(&format!("SELECT {USER_COLS} FROM users WHERE id = ?"))
            .bind(id.as_bytes().as_slice())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(user_from_row).transpose()
    }

    pub async fn user_by_username(&self, username: &str) -> Result<Option<User>> {
        let row = sqlx::query(&format!("SELECT {USER_COLS} FROM users WHERE username = ?"))
            .bind(username)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(user_from_row).transpose()
    }

    /// Suma un intento fallido y bloquea la cuenta `lock_secs` al llegar a `max`.
    pub async fn record_failed_login(&self, id: Id, max: i64, lock_secs: i64) -> Result<()> {
        sqlx::query(
            "UPDATE users SET locked_until = CASE WHEN failed_logins + 1 >= ? THEN ? ELSE locked_until END, \
             failed_logins = CASE WHEN failed_logins + 1 >= ? THEN 0 ELSE failed_logins + 1 END \
             WHERE id = ?",
        )
        .bind(max)
        .bind(now() + lock_secs)
        .bind(max)
        .bind(id.as_bytes().as_slice())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn reset_failed_logins(&self, id: Id) -> Result<()> {
        sqlx::query("UPDATE users SET failed_logins = 0, locked_until = NULL WHERE id = ?")
            .bind(id.as_bytes().as_slice())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_totp(&self, id: Id, sealed_secret: Option<&[u8]>, enabled: bool) -> Result<()> {
        sqlx::query("UPDATE users SET totp_secret = ?, totp_enabled = ? WHERE id = ?")
            .bind(sealed_secret)
            .bind(enabled)
            .bind(id.as_bytes().as_slice())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_language(&self, id: Id, language: &str) -> Result<()> {
        sqlx::query("UPDATE users SET language = ? WHERE id = ?")
            .bind(language)
            .bind(id.as_bytes().as_slice())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_session(&self, user: Id, token_hash: &str, user_agent: Option<&str>, ttl: i64) -> Result<()> {
        let t = now();
        sqlx::query(
            "INSERT INTO sessions (id, token_hash, user_id, user_agent, created_at, expires_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(Id::new().as_bytes().as_slice())
        .bind(token_hash)
        .bind(user.as_bytes().as_slice())
        .bind(user_agent)
        .bind(t)
        .bind(t + ttl)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn session_user(&self, token_hash: &str) -> Result<Option<User>> {
        let cols = USER_COLS
            .split(", ")
            .map(|c| format!("u.{}", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let row = sqlx::query(&format!(
            "SELECT {cols} FROM sessions s JOIN users u ON u.id = s.user_id \
             WHERE s.token_hash = ? AND s.expires_at > ?"
        ))
        .bind(token_hash)
        .bind(now())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(user_from_row).transpose()
    }

    pub async fn delete_session(&self, token_hash: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(token_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_invitation(
        &self,
        created_by: Id,
        note: Option<&str>,
        token_hash: &str,
        ttl: i64,
    ) -> Result<Invitation> {
        let id = Id::new();
        let t = now();
        sqlx::query(
            "INSERT INTO invitations (id, token_hash, note, created_by, created_at, expires_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_bytes().as_slice())
        .bind(token_hash)
        .bind(note)
        .bind(created_by.as_bytes().as_slice())
        .bind(t)
        .bind(t + ttl)
        .execute(&self.pool)
        .await?;
        Ok(Invitation {
            id,
            note: note.map(Into::into),
            created_by,
            created_at: t,
            expires_at: t + ttl,
            used_at: None,
        })
    }

    pub async fn list_invitations(&self) -> Result<Vec<Invitation>> {
        let rows = sqlx::query(
            "SELECT id, note, created_by, created_at, expires_at, used_at FROM invitations ORDER BY id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(invitation_from_row).collect()
    }

    pub async fn delete_invitation(&self, id: Id) -> Result<bool> {
        let r = sqlx::query("DELETE FROM invitations WHERE id = ? AND used_at IS NULL")
            .bind(id.as_bytes().as_slice())
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Invitación válida (sin usar y sin caducar) para ese token.
    pub async fn valid_invitation(&self, token_hash: &str) -> Result<Option<Invitation>> {
        let row = sqlx::query(
            "SELECT id, note, created_by, created_at, expires_at, used_at FROM invitations \
             WHERE token_hash = ? AND used_at IS NULL AND expires_at > ?",
        )
        .bind(token_hash)
        .bind(now())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(invitation_from_row).transpose()
    }

    /// Crea el usuario y marca la invitación como usada en una sola transacción.
    /// Devuelve `None` si la invitación ya no es válida.
    pub async fn accept_invitation(&self, token_hash: &str, u: NewUser<'_>) -> Result<Option<User>> {
        let mut tx = self.pool.begin().await?;
        let id = Id::new();
        let t = now();
        let used = sqlx::query(
            "UPDATE invitations SET used_at = ? WHERE token_hash = ? AND used_at IS NULL AND expires_at > ?",
        )
        .bind(t)
        .bind(token_hash)
        .bind(t)
        .execute(&mut *tx)
        .await?;
        if used.rows_affected() == 0 {
            return Ok(None);
        }
        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, role, language, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_bytes().as_slice())
        .bind(u.username)
        .bind(u.email)
        .bind(u.password_hash)
        .bind(u.role.as_str())
        .bind(u.language)
        .bind(t)
        .execute(&mut *tx)
        .await
        .map_err(map_unique)?;
        sqlx::query("UPDATE invitations SET used_by = ? WHERE token_hash = ?")
            .bind(id.as_bytes().as_slice())
            .bind(token_hash)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.user_by_id(id).await
    }

    pub async fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn audit(&self, user: Option<Id>, action: &str, detail: Option<&str>) -> Result<()> {
        sqlx::query("INSERT INTO audit_log (id, user_id, action, detail, created_at) VALUES (?, ?, ?, ?, ?)")
            .bind(Id::new().as_bytes().as_slice())
            .bind(user.map(|u| u.as_bytes().to_vec()))
            .bind(action)
            .bind(detail)
            .bind(now())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> (tempfile::TempDir, AppDb) {
        let dir = tempfile::tempdir().unwrap();
        let db = AppDb::open(&dir.path().join("app.db")).await.unwrap();
        (dir, db)
    }

    fn new_user(name: &str) -> NewUser<'_> {
        NewUser {
            username: name,
            email: None,
            password_hash: "x",
            role: Role::User,
            language: "es",
        }
    }

    #[tokio::test]
    async fn usernames_are_unique_ignoring_case() {
        let (_d, db) = db().await;
        db.create_user(new_user("Javier")).await.unwrap();
        assert!(matches!(
            db.create_user(new_user("javier")).await,
            Err(crate::Error::Duplicate)
        ));
    }

    #[tokio::test]
    async fn invitation_is_single_use() {
        let (_d, db) = db().await;
        let admin = db.create_user(new_user("admin")).await.unwrap();
        db.create_invitation(admin.id, None, "h1", 72 * 3600).await.unwrap();
        assert!(db.accept_invitation("h1", new_user("ana")).await.unwrap().is_some());
        assert!(db.accept_invitation("h1", new_user("otra")).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn expired_invitation_is_rejected() {
        let (_d, db) = db().await;
        let admin = db.create_user(new_user("admin")).await.unwrap();
        db.create_invitation(admin.id, None, "h2", -1).await.unwrap();
        assert!(db.valid_invitation("h2").await.unwrap().is_none());
        assert!(db.accept_invitation("h2", new_user("ana")).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn lockout_after_max_failures() {
        let (_d, db) = db().await;
        let u = db.create_user(new_user("ana")).await.unwrap();
        for _ in 0..5 {
            db.record_failed_login(u.id, 5, 900).await.unwrap();
        }
        let u = db.user_by_id(u.id).await.unwrap().unwrap();
        assert!(u.locked_until.unwrap() > now());
    }
}

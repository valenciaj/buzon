//! Tokens personales de la API: para scripts y otras apps, con permisos limitados.

use sqlx::sqlite::SqliteRow;
use sqlx::{AssertSqlSafe, Row};

use crate::app::{user_from_row, User, USER_COLS};
use crate::{now, AppDb, Id, Result};

/// Permisos que puede tener un token.
pub const SCOPES: &[&str] = &["mail:read", "mail:write", "mail:send", "contacts", "webhooks"];

#[derive(Debug, Clone)]
pub struct ApiToken {
    pub id: Id,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
}

pub struct NewToken<'a> {
    pub user: Id,
    pub name: &'a str,
    pub token_hash: &'a str,
    pub prefix: &'a str,
    pub scopes: &'a [String],
    pub expires_at: Option<i64>,
}

fn token_from_row(r: &SqliteRow) -> Result<ApiToken> {
    Ok(ApiToken {
        id: Id::from_slice(r.try_get::<&[u8], _>("id")?)?,
        name: r.try_get("name")?,
        prefix: r.try_get("prefix")?,
        scopes: r
            .try_get::<&str, _>("scopes")?
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        created_at: r.try_get("created_at")?,
        expires_at: r.try_get("expires_at")?,
        last_used_at: r.try_get("last_used_at")?,
    })
}

const TOKEN_COLS: &str = "id, name, prefix, scopes, created_at, expires_at, last_used_at";

impl AppDb {
    pub async fn create_token(&self, t: NewToken<'_>) -> Result<ApiToken> {
        let id = Id::new();
        sqlx::query(
            "INSERT INTO api_tokens (id, user_id, name, token_hash, prefix, scopes, created_at, expires_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_bytes().as_slice())
        .bind(t.user.as_bytes().as_slice())
        .bind(t.name)
        .bind(t.token_hash)
        .bind(t.prefix)
        .bind(t.scopes.join(" "))
        .bind(now())
        .bind(t.expires_at)
        .execute(&self.pool)
        .await
        .map_err(crate::map_unique)?;
        Ok(self
            .tokens(t.user)
            .await?
            .into_iter()
            .find(|x| x.id == id)
            .expect("token recién creado"))
    }

    /// Tokens vigentes del usuario (los revocados no se listan).
    pub async fn tokens(&self, user: Id) -> Result<Vec<ApiToken>> {
        let rows = sqlx::query(AssertSqlSafe(format!(
            "SELECT {TOKEN_COLS} FROM api_tokens WHERE user_id = ? AND revoked_at IS NULL ORDER BY id DESC"
        )))
        .bind(user.as_bytes().as_slice())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(token_from_row).collect()
    }

    pub async fn revoke_token(&self, user: Id, id: Id) -> Result<bool> {
        let r = sqlx::query("UPDATE api_tokens SET revoked_at = ? WHERE id = ? AND user_id = ? AND revoked_at IS NULL")
            .bind(now())
            .bind(id.as_bytes().as_slice())
            .bind(user.as_bytes().as_slice())
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Usuario y token de un token en vigor. Apunta cuándo se usó (como mucho una vez por minuto).
    pub async fn token_user(&self, token_hash: &str) -> Result<Option<(User, ApiToken)>> {
        let t = now();
        let row = sqlx::query(AssertSqlSafe(format!(
            "SELECT t.user_id, {TOKEN_COLS} FROM api_tokens t \
             WHERE t.token_hash = ? AND t.revoked_at IS NULL AND (t.expires_at IS NULL OR t.expires_at > ?)"
        )))
        .bind(token_hash)
        .bind(t)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else { return Ok(None) };
        let token = token_from_row(&row)?;
        let user_id = Id::from_slice(row.try_get::<&[u8], _>("user_id")?)?;
        let user = sqlx::query(AssertSqlSafe(format!("SELECT {USER_COLS} FROM users WHERE id = ?")))
            .bind(user_id.as_bytes().as_slice())
            .fetch_optional(&self.pool)
            .await?;
        let Some(user) = user.as_ref().map(user_from_row).transpose()? else {
            return Ok(None);
        };
        if token.last_used_at.is_none_or(|u| u < t - 60) {
            sqlx::query("UPDATE api_tokens SET last_used_at = ? WHERE id = ?")
                .bind(t)
                .bind(token.id.as_bytes().as_slice())
                .execute(&self.pool)
                .await?;
        }
        Ok(Some((user, token)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{db, new_user};

    #[tokio::test]
    async fn tokens_expire_and_revoke() {
        let (_d, db) = db().await;
        let u = db.create_user(new_user("ana")).await.unwrap();
        let other = db.create_user(new_user("luis")).await.unwrap();
        let scopes = vec!["mail:read".to_string()];
        let new = |hash, expires_at| NewToken {
            user: u.id,
            name: "script",
            token_hash: hash,
            prefix: "bz_abc",
            scopes: &scopes,
            expires_at,
        };
        let tok = db.create_token(new("h1", None)).await.unwrap();
        assert_eq!(tok.scopes, scopes);
        db.create_token(new("h2", Some(now() - 1))).await.unwrap();

        let (user, t) = db.token_user("h1").await.unwrap().unwrap();
        assert_eq!(user.id, u.id);
        assert_eq!(t.id, tok.id);
        assert!(db.tokens(u.id).await.unwrap()[1].last_used_at.is_some());
        assert!(db.token_user("h2").await.unwrap().is_none(), "caducado");

        assert!(!db.revoke_token(other.id, tok.id).await.unwrap(), "de otro usuario");
        assert!(db.revoke_token(u.id, tok.id).await.unwrap());
        assert!(db.token_user("h1").await.unwrap().is_none());
        assert_eq!(db.tokens(u.id).await.unwrap().len(), 1);
    }
}

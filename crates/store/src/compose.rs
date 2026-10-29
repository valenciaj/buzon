//! Redacción: identidades (remitentes con su firma), borradores y adjuntos subidos.

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::mail::{b, id_of, Addr};
use crate::{now, Id, Result, UserDb};

/// Un remitente de una cuenta: su dirección principal o un alias, con su firma.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub id: Id,
    pub account_id: Id,
    pub name: String,
    pub email: String,
    pub signature: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Draft {
    pub id: Option<Id>,
    pub identity_id: Option<Id>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub bcc: Vec<Addr>,
    pub subject: String,
    pub html: String,
    pub text: String,
    pub reply_to: Option<Id>,
    pub forward_of: Option<Id>,
    pub attachments: Vec<Id>,
    pub updated_at: i64,
    /// Cuándo sale, si está programado. `save_draft` no lo cambia: usa `schedule_draft`.
    pub send_at: Option<i64>,
    /// Por qué falló el último envío programado.
    pub send_error: Option<String>,
    /// Avisar si nadie responde en estos días.
    pub follow_up_days: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    pub id: Id,
    pub name: String,
    pub content_type: String,
    pub size: u64,
}

/// Datos de un mensaje guardado que hacen falta para responderlo.
#[derive(Debug, Clone)]
pub struct MessageRef {
    pub account_id: Id,
    pub mailbox: String,
    pub uid: u32,
    pub message_id: Option<String>,
    pub refs: Vec<String>,
}

/// Dirección conocida, para autocompletar destinatarios.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    pub name: Option<String>,
    pub email: String,
}

fn identity_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Identity> {
    Ok(Identity {
        id: id_of(r, "id")?,
        account_id: id_of(r, "account_id")?,
        name: r.try_get("name")?,
        email: r.try_get("email")?,
        signature: r.try_get("signature")?,
        is_default: r.try_get("is_default")?,
    })
}

fn opt_id(r: &sqlx::sqlite::SqliteRow, col: &str) -> Result<Option<Id>> {
    r.try_get::<Option<&[u8]>, _>(col)?.map(Id::from_slice).transpose()
}

fn draft_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Draft> {
    let json = |col: &str| -> Result<String> { Ok(r.try_get(col)?) };
    Ok(Draft {
        id: Some(id_of(r, "id")?),
        identity_id: opt_id(r, "identity_id")?,
        to: serde_json::from_str(&json("to_json")?).unwrap_or_default(),
        cc: serde_json::from_str(&json("cc_json")?).unwrap_or_default(),
        bcc: serde_json::from_str(&json("bcc_json")?).unwrap_or_default(),
        subject: r.try_get("subject")?,
        html: r.try_get("html")?,
        text: r.try_get("text")?,
        reply_to: opt_id(r, "reply_to")?,
        forward_of: opt_id(r, "forward_of")?,
        attachments: serde_json::from_str(&json("attachments")?).unwrap_or_default(),
        updated_at: r.try_get("updated_at")?,
        send_at: r.try_get("send_at")?,
        send_error: r.try_get("send_error")?,
        follow_up_days: r.try_get("follow_up_days")?,
    })
}

fn to_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

impl UserDb {
    /// Identidades de una cuenta (o de todas). Cada cuenta tiene al menos una, con su dirección.
    pub async fn identities(&self, account: Option<Id>) -> Result<Vec<Identity>> {
        for acc in self.accounts().await? {
            let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM identities WHERE account_id = ?")
                .bind(b(acc.id))
                .fetch_one(&self.pool)
                .await?;
            if n == 0 {
                sqlx::query(
                    "INSERT INTO identities (id, account_id, name, email, is_default, created_at) VALUES (?, ?, '', ?, 1, ?)",
                )
                .bind(b(Id::new()))
                .bind(b(acc.id))
                .bind(&acc.email)
                .bind(now())
                .execute(&self.pool)
                .await?;
            }
        }
        let rows = match account {
            Some(a) => {
                sqlx::query("SELECT * FROM identities WHERE account_id = ? ORDER BY is_default DESC, id")
                    .bind(b(a))
                    .fetch_all(&self.pool)
                    .await?
            }
            None => {
                sqlx::query("SELECT * FROM identities ORDER BY account_id, is_default DESC, id")
                    .fetch_all(&self.pool)
                    .await?
            }
        };
        rows.iter().map(identity_from_row).collect()
    }

    pub async fn identity(&self, id: Id) -> Result<Option<Identity>> {
        let row = sqlx::query("SELECT * FROM identities WHERE id = ?")
            .bind(b(id))
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(identity_from_row).transpose()
    }

    /// Crea o actualiza una identidad. Si queda como predeterminada, las demás de la cuenta dejan de serlo.
    pub async fn save_identity(&self, i: &Identity) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        if i.is_default {
            sqlx::query("UPDATE identities SET is_default = 0 WHERE account_id = ?")
                .bind(b(i.account_id))
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query(
            "INSERT INTO identities (id, account_id, name, email, signature, is_default, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT (id) DO UPDATE SET name = excluded.name, \
             email = excluded.email, signature = excluded.signature, is_default = excluded.is_default",
        )
        .bind(b(i.id))
        .bind(b(i.account_id))
        .bind(&i.name)
        .bind(&i.email)
        .bind(&i.signature)
        .bind(i.is_default)
        .bind(now())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Borra una identidad, salvo que sea la única de su cuenta. Devuelve si se borró.
    pub async fn delete_identity(&self, id: Id) -> Result<bool> {
        let Some(i) = self.identity(id).await? else {
            return Ok(false);
        };
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM identities WHERE account_id = ?")
            .bind(b(i.account_id))
            .fetch_one(&self.pool)
            .await?;
        if n <= 1 {
            return Ok(false);
        }
        sqlx::query("DELETE FROM identities WHERE id = ?")
            .bind(b(id))
            .execute(&self.pool)
            .await?;
        if i.is_default {
            sqlx::query(
                "UPDATE identities SET is_default = 1 WHERE id = (SELECT id FROM identities WHERE account_id = ? ORDER BY id LIMIT 1)",
            )
            .bind(b(i.account_id))
            .execute(&self.pool)
            .await?;
        }
        Ok(true)
    }

    /// Guarda un borrador (nuevo si no tiene id) y lo devuelve con su id.
    pub async fn save_draft(&self, d: &Draft) -> Result<Draft> {
        let id = d.id.unwrap_or_default();
        let t = now();
        sqlx::query(
            "INSERT INTO drafts (id, identity_id, to_json, cc_json, bcc_json, subject, html, text, reply_to, forward_of, \
             attachments, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT (id) DO UPDATE SET \
             identity_id = excluded.identity_id, to_json = excluded.to_json, cc_json = excluded.cc_json, \
             bcc_json = excluded.bcc_json, subject = excluded.subject, html = excluded.html, text = excluded.text, \
             reply_to = excluded.reply_to, forward_of = excluded.forward_of, attachments = excluded.attachments, \
             updated_at = excluded.updated_at",
        )
        .bind(b(id))
        .bind(d.identity_id.map(b))
        .bind(to_json(&d.to))
        .bind(to_json(&d.cc))
        .bind(to_json(&d.bcc))
        .bind(&d.subject)
        .bind(&d.html)
        .bind(&d.text)
        .bind(d.reply_to.map(b))
        .bind(d.forward_of.map(b))
        .bind(to_json(&d.attachments))
        .bind(t)
        .execute(&self.pool)
        .await?;
        Ok(Draft {
            id: Some(id),
            updated_at: t,
            ..d.clone()
        })
    }

    /// Borradores (sin los programados).
    pub async fn drafts(&self) -> Result<Vec<Draft>> {
        let rows = sqlx::query("SELECT * FROM drafts WHERE send_at IS NULL ORDER BY updated_at DESC, id DESC")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(draft_from_row).collect()
    }

    /// Envíos programados, del más próximo al más lejano.
    pub async fn scheduled(&self) -> Result<Vec<Draft>> {
        let rows = sqlx::query("SELECT * FROM drafts WHERE send_at IS NOT NULL ORDER BY send_at, id")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(draft_from_row).collect()
    }

    /// Programa un borrador (o lo devuelve a Borradores con `None`, y con el error si falló).
    pub async fn schedule_draft(
        &self,
        id: Id,
        send_at: Option<i64>,
        follow_up_days: Option<i64>,
        error: Option<&str>,
    ) -> Result<bool> {
        let r = sqlx::query("UPDATE drafts SET send_at = ?, follow_up_days = ?, send_error = ? WHERE id = ?")
            .bind(send_at)
            .bind(follow_up_days)
            .bind(error)
            .bind(b(id))
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Cancela un envío programado que aún no ha empezado a salir.
    pub async fn unschedule_draft(&self, id: Id) -> Result<bool> {
        let r = sqlx::query("UPDATE drafts SET send_at = NULL, send_error = NULL WHERE id = ? AND send_at > 0")
            .bind(b(id))
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Envíos que ya tocan. Los marca como en curso (los saca de la cola) para no enviarlos dos veces.
    pub async fn take_due_drafts(&self, now: i64) -> Result<Vec<Draft>> {
        let rows = sqlx::query(
            "UPDATE drafts SET send_at = -send_at WHERE send_at IS NOT NULL AND send_at > 0 AND send_at <= ? RETURNING *",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(draft_from_row).collect()
    }

    /// Cuándo sale el próximo envío programado.
    pub async fn next_send_at(&self) -> Result<Option<i64>> {
        Ok(sqlx::query_scalar("SELECT MIN(send_at) FROM drafts WHERE send_at > 0")
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn draft(&self, id: Id) -> Result<Option<Draft>> {
        let row = sqlx::query("SELECT * FROM drafts WHERE id = ?")
            .bind(b(id))
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(draft_from_row).transpose()
    }

    pub async fn delete_draft(&self, id: Id) -> Result<bool> {
        let r = sqlx::query("DELETE FROM drafts WHERE id = ?")
            .bind(b(id))
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn add_upload(&self, name: &str, content_type: &str, size: u64) -> Result<Upload> {
        let id = Id::new();
        sqlx::query("INSERT INTO uploads (id, name, content_type, size, created_at) VALUES (?, ?, ?, ?, ?)")
            .bind(b(id))
            .bind(name)
            .bind(content_type)
            .bind(size as i64)
            .bind(now())
            .execute(&self.pool)
            .await?;
        Ok(Upload {
            id,
            name: name.into(),
            content_type: content_type.into(),
            size,
        })
    }

    pub async fn upload(&self, id: Id) -> Result<Option<Upload>> {
        let row = sqlx::query("SELECT * FROM uploads WHERE id = ?")
            .bind(b(id))
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| {
            Ok(Upload {
                id: id_of(&r, "id")?,
                name: r.try_get("name")?,
                content_type: r.try_get("content_type")?,
                size: r.try_get::<i64, _>("size")? as u64,
            })
        })
        .transpose()
    }

    pub async fn delete_upload(&self, id: Id) -> Result<()> {
        sqlx::query("DELETE FROM uploads WHERE id = ?")
            .bind(b(id))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Lo necesario de un mensaje guardado para responderlo o reenviarlo.
    pub async fn message_ref(&self, id: Id) -> Result<Option<MessageRef>> {
        let row = sqlx::query(
            "SELECT m.account_id, mb.name, m.uid, m.message_id, m.refs FROM messages m \
             JOIN mailboxes mb ON mb.id = m.mailbox_id WHERE m.id = ?",
        )
        .bind(b(id))
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| {
            Ok(MessageRef {
                account_id: id_of(&r, "account_id")?,
                mailbox: r.try_get("name")?,
                // 0 si se acaba de mover y el servidor aún no le ha dado UID.
                uid: r.try_get::<i64, _>("uid")?.max(0) as u32,
                message_id: r.try_get("message_id")?,
                refs: r
                    .try_get::<String, _>("refs")?
                    .split_whitespace()
                    .map(String::from)
                    .collect(),
            })
        })
        .transpose()
    }

    pub async fn set_answered(&self, id: Id) -> Result<()> {
        sqlx::query("UPDATE messages SET answered = 1 WHERE id = ?")
            .bind(b(id))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Nombre IMAP de la carpeta de un tipo (por ejemplo `sent`) en una cuenta.
    pub async fn mailbox_name_by_role(&self, account: Id, role: &str) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT name FROM mailboxes WHERE account_id = ? AND role = ? ORDER BY name LIMIT 1")
                .bind(b(account))
                .bind(role)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Direcciones para autocompletar: primero la libreta, luego quien te ha escrito
    /// (los más frecuentes antes).
    pub async fn contacts(&self, q: &str, limit: u32) -> Result<Vec<Contact>> {
        let like = format!("%{}%", q.replace(['%', '_'], ""));
        let rows = sqlx::query(
            "SELECT name, email FROM (SELECT *, ROW_NUMBER() OVER (PARTITION BY email ORDER BY book) AS rn FROM (\
               SELECT NULLIF(name, '') AS name, lower(email) AS email, 0 AS book, 0 AS n FROM contacts \
               WHERE email LIKE ?1 OR name LIKE ?1 \
               UNION ALL \
               SELECT MAX(from_name), lower(from_addr), 1, COUNT(*) FROM messages \
               WHERE from_addr IS NOT NULL AND (from_addr LIKE ?1 OR from_name LIKE ?1) GROUP BY lower(from_addr))) \
             WHERE rn = 1 ORDER BY book, n DESC, email LIMIT ?2",
        )
        .bind(like)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(Contact {
                    name: r.try_get("name")?,
                    email: r.try_get("email")?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user::NewAccount;

    async fn db() -> (tempfile::TempDir, UserDb, Id) {
        let dir = tempfile::tempdir().unwrap();
        let db = UserDb::open(dir.path(), Id::new()).await.unwrap();
        let key = crate::crypto::MasterKey::generate().derive("x");
        let acc = db
            .add_account(
                NewAccount {
                    name: "P",
                    email: "yo@ejemplo.es",
                    imap_host: "h",
                    imap_port: 993,
                    smtp_host: "h",
                    smtp_port: 465,
                    login: "yo",
                    password: "x",
                    imap_security: "tls",
                    smtp_security: "tls",
                },
                &key,
            )
            .await
            .unwrap();
        (dir, db, acc.id)
    }

    #[tokio::test]
    async fn identities_have_one_default_per_account() {
        let (_d, db, acc) = db().await;
        let list = db.identities(None).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].email, "yo@ejemplo.es");
        assert!(list[0].is_default);
        assert!(!db.delete_identity(list[0].id).await.unwrap(), "la única no se borra");

        let alias = Identity {
            id: Id::new(),
            account_id: acc,
            name: "Javier V.".into(),
            email: "javier@ejemplo.es".into(),
            signature: "Un saludo".into(),
            is_default: true,
        };
        db.save_identity(&alias).await.unwrap();
        let list = db.identities(Some(acc)).await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, alias.id, "la predeterminada va primero");
        assert_eq!(list.iter().filter(|i| i.is_default).count(), 1);

        assert!(db.delete_identity(alias.id).await.unwrap());
        let list = db.identities(Some(acc)).await.unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].is_default, "otra pasa a ser la predeterminada");
    }

    #[tokio::test]
    async fn drafts_and_uploads_round_trip() {
        let (_d, db, _) = db().await;
        let up = db.add_upload("plano.pdf", "application/pdf", 1234).await.unwrap();
        let d = db
            .save_draft(&Draft {
                to: vec![Addr {
                    name: Some("Ana".into()),
                    email: "ana@ejemplo.es".into(),
                }],
                subject: "Hola".into(),
                text: "¿Qué tal?".into(),
                attachments: vec![up.id],
                ..Default::default()
            })
            .await
            .unwrap();
        let id = d.id.unwrap();
        let mut again = db.draft(id).await.unwrap().unwrap();
        assert_eq!(again.to[0].email, "ana@ejemplo.es");
        assert_eq!(again.attachments, vec![up.id]);
        again.subject = "Hola de nuevo".into();
        db.save_draft(&again).await.unwrap();
        let all = db.drafts().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].subject, "Hola de nuevo");
        assert_eq!(db.upload(up.id).await.unwrap().unwrap().size, 1234);
        assert!(db.delete_draft(id).await.unwrap());
        assert!(db.drafts().await.unwrap().is_empty());
    }
}

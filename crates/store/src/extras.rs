//! Seguimiento de respuestas: correos enviados que esperan contestación.

use serde::Serialize;
use sqlx::{AssertSqlSafe, Row};

use crate::mail::{b, id_of, Addr};
use crate::{now, Id, Result, UserDb};

#[derive(Debug, Clone, Serialize)]
pub struct FollowUp {
    pub id: Id,
    pub account_id: Id,
    pub subject: String,
    pub to: Vec<Addr>,
    pub sent_at: i64,
    pub due_at: i64,
    /// Hilo de la copia en Enviados (cuando ya se ha sincronizado).
    pub thread: Option<Id>,
}

/// Respuesta de otra persona: un mensaje posterior al envío, que no sale de una
/// dirección propia y que contesta al enviado (o está en su hilo).
const ANSWERED: &str = "EXISTS (SELECT 1 FROM messages r WHERE r.date > f.sent_at \
     AND lower(COALESCE(r.from_addr, '')) NOT IN (SELECT lower(email) FROM identities UNION SELECT lower(email) FROM accounts) \
     AND (r.in_reply_to = f.message_id OR instr(r.refs, f.message_id) > 0 \
          OR r.thread_id = (SELECT s.thread_id FROM messages s WHERE s.message_id = f.message_id LIMIT 1)))";

impl UserDb {
    /// Apunta un correo enviado para avisar si nadie responde en `days` días.
    pub async fn add_followup(
        &self,
        account: Id,
        message_id: &str,
        subject: &str,
        to: &[Addr],
        days: i64,
    ) -> Result<Id> {
        let id = Id::new();
        let t = now();
        sqlx::query(
            "INSERT INTO followups (id, account_id, message_id, subject, to_json, sent_at, due_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(b(id))
        .bind(b(account))
        .bind(message_id)
        .bind(subject)
        .bind(serde_json::to_string(to).unwrap_or_else(|_| "[]".into()))
        .bind(t)
        .bind(t + days * 24 * 3600)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    /// Correos sin respuesta. Con `due_only`, solo los que ya han pasado el plazo.
    /// Los que ya tienen respuesta se cierran solos.
    pub async fn followups(&self, due_only: bool) -> Result<Vec<FollowUp>> {
        let t = now();
        sqlx::query(AssertSqlSafe(format!(
            "UPDATE followups AS f SET dismissed_at = ? WHERE dismissed_at IS NULL AND {ANSWERED}"
        )))
        .bind(t)
        .execute(&self.pool)
        .await?;
        let rows = sqlx::query(
            "SELECT f.*, (SELECT s.thread_id FROM messages s WHERE s.message_id = f.message_id LIMIT 1) AS thread \
             FROM followups f WHERE f.dismissed_at IS NULL AND (? = 0 OR f.due_at <= ?) ORDER BY f.due_at, f.id",
        )
        .bind(due_only)
        .bind(t)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(FollowUp {
                    id: id_of(r, "id")?,
                    account_id: id_of(r, "account_id")?,
                    subject: r.try_get("subject")?,
                    to: serde_json::from_str(r.try_get("to_json")?).unwrap_or_default(),
                    sent_at: r.try_get("sent_at")?,
                    due_at: r.try_get("due_at")?,
                    thread: r
                        .try_get::<Option<Vec<u8>>, _>("thread")?
                        .map(|v| Id::from_slice(&v))
                        .transpose()?,
                })
            })
            .collect()
    }

    /// Deja de esperar respuesta.
    pub async fn dismiss_followup(&self, id: Id) -> Result<bool> {
        let r = sqlx::query("UPDATE followups SET dismissed_at = ? WHERE id = ? AND dismissed_at IS NULL")
            .bind(now())
            .bind(b(id))
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Al arrancar: un envío que quedó a medias (el servidor se paró) vuelve a Borradores
    /// en vez de reintentarse, para no enviarlo dos veces.
    pub async fn recover_interrupted_sends(&self, error: &str) -> Result<u64> {
        let r = sqlx::query("UPDATE drafts SET send_at = NULL, send_error = ? WHERE send_at < 0")
            .bind(error)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::Draft;
    use crate::organize::tests::{db, msg};

    #[tokio::test]
    async fn followups_close_when_someone_answers() {
        let (_d, db, acc) = db().await;
        let inbox = db.upsert_mailbox(acc, "INBOX", Some("/"), Some("inbox")).await.unwrap();
        let sent = db.upsert_mailbox(acc, "Sent", Some("/"), Some("sent")).await.unwrap();
        let ana = vec![Addr {
            name: None,
            email: "ana@ejemplo.es".into(),
        }];
        let f = db
            .add_followup(acc, "s1@ejemplo.es", "Presupuesto", &ana, 0)
            .await
            .unwrap();
        db.add_followup(acc, "s2@ejemplo.es", "Más tarde", &ana, 3)
            .await
            .unwrap();
        assert_eq!(db.followups(true).await.unwrap().len(), 1, "solo el que ya ha vencido");
        assert_eq!(db.followups(false).await.unwrap().len(), 2);

        // La copia en Enviados da el hilo; una respuesta propia no cuenta.
        let mut copy = msg(
            acc,
            sent.id,
            1,
            "s1@ejemplo.es",
            "Presupuesto",
            "yo@ejemplo.es",
            "Te paso",
        );
        copy.date = now() + 1;
        db.insert_message(&copy).await.unwrap();
        let mut mine = msg(
            acc,
            sent.id,
            2,
            "s3@ejemplo.es",
            "Re: Presupuesto",
            "yo@ejemplo.es",
            "¿Lo viste?",
        );
        mine.in_reply_to = Some("s1@ejemplo.es".into());
        mine.date = now() + 2;
        db.insert_message(&mine).await.unwrap();
        let due = db.followups(true).await.unwrap();
        assert_eq!(due[0].id, f);
        assert!(due[0].thread.is_some());

        let mut reply = msg(
            acc,
            inbox.id,
            1,
            "r1@ejemplo.es",
            "Re: Presupuesto",
            "ana@ejemplo.es",
            "Visto",
        );
        reply.in_reply_to = Some("s1@ejemplo.es".into());
        reply.date = now() + 3;
        db.insert_message(&reply).await.unwrap();
        assert!(db.followups(true).await.unwrap().is_empty());

        let rest = db.followups(false).await.unwrap();
        assert!(db.dismiss_followup(rest[0].id).await.unwrap());
        assert!(db.followups(false).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn scheduled_drafts_leave_once() {
        let (_d, db, _acc) = db().await;
        let d = db
            .save_draft(&Draft {
                subject: "Luego".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        let id = d.id.unwrap();
        db.schedule_draft(id, Some(now() + 60), Some(3), None).await.unwrap();
        assert!(
            db.drafts().await.unwrap().is_empty(),
            "los programados no están en Borradores"
        );
        assert_eq!(db.scheduled().await.unwrap()[0].follow_up_days, Some(3));
        assert!(db.take_due_drafts(now()).await.unwrap().is_empty());
        assert_eq!(db.next_send_at().await.unwrap(), Some(now() + 60));

        let due = db.take_due_drafts(now() + 61).await.unwrap();
        assert_eq!(due.len(), 1);
        assert!(
            db.take_due_drafts(now() + 61).await.unwrap().is_empty(),
            "no sale dos veces"
        );
        assert_eq!(db.next_send_at().await.unwrap(), None);

        // Si el servidor se para a mitad, vuelve a Borradores con aviso.
        assert_eq!(db.recover_interrupted_sends("cortado").await.unwrap(), 1);
        let back = db.drafts().await.unwrap();
        assert_eq!(back[0].send_error.as_deref(), Some("cortado"));
    }
}

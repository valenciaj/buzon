//! Webhooks: avisos firmados a otras apps cuando pasa algo en el correo.
//! Los envíos van a una cola persistente y se reintentan durante casi 24 horas.

use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::{now, AppDb, Id, Result};

/// Eventos a los que se puede suscribir un webhook.
pub const EVENTS: &[&str] = &[
    "message.received",
    "message.sent",
    "message.read",
    "message.moved",
    "thread.labeled",
    "contact.screened",
    "account.sync_failed",
];

/// Evento de prueba: solo se envía cuando se pide desde los ajustes.
pub const PING: &str = "ping";

/// Espera antes de cada reintento: 1 min, 5 min, 30 min, 2 h, 6 h y 15 h (unas 23 h en total).
pub const RETRY_DELAYS: &[i64] = &[60, 300, 1800, 7200, 21_600, 54_000];

/// El historial se guarda 30 días.
const KEEP: i64 = 30 * 24 * 3600;

#[derive(Debug, Clone)]
pub struct Webhook {
    pub id: Id,
    pub user_id: Id,
    pub url: String,
    pub description: String,
    pub events: Vec<String>,
    /// Secreto de firma, cifrado.
    pub secret: Vec<u8>,
    pub active: bool,
    pub created_at: i64,
    /// Estado del último envío terminado (`ok` o `failed`).
    pub last_status: Option<String>,
}

impl Webhook {
    pub fn wants(&self, event: &str) -> bool {
        self.active && self.events.iter().any(|e| e == event)
    }
}

#[derive(Debug, Clone)]
pub struct Delivery {
    pub id: Id,
    pub webhook_id: Id,
    pub event: String,
    pub payload: String,
    /// `pending`, `ok` o `failed`.
    pub status: String,
    pub attempts: i64,
    pub next_at: Option<i64>,
    /// Código HTTP de la última respuesta.
    pub response: Option<i64>,
    pub error: Option<String>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

pub struct NewWebhook<'a> {
    pub url: &'a str,
    pub description: &'a str,
    pub events: &'a [String],
    pub secret: &'a [u8],
}

pub struct WebhookChange<'a> {
    pub url: Option<&'a str>,
    pub description: Option<&'a str>,
    pub events: Option<&'a [String]>,
    pub active: Option<bool>,
}

/// Resultado de un intento de envío.
pub struct Attempt<'a> {
    pub ok: bool,
    pub response: Option<i64>,
    pub error: Option<&'a str>,
}

fn webhook_from_row(r: &SqliteRow) -> Result<Webhook> {
    Ok(Webhook {
        id: Id::from_slice(r.try_get::<&[u8], _>("id")?)?,
        user_id: Id::from_slice(r.try_get::<&[u8], _>("user_id")?)?,
        url: r.try_get("url")?,
        description: r.try_get("description")?,
        events: r
            .try_get::<&str, _>("events")?
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        secret: r.try_get("secret")?,
        active: r.try_get("active")?,
        created_at: r.try_get("created_at")?,
        last_status: r.try_get("last_status")?,
    })
}

fn delivery_from_row(r: &SqliteRow) -> Result<Delivery> {
    Ok(Delivery {
        id: Id::from_slice(r.try_get::<&[u8], _>("id")?)?,
        webhook_id: Id::from_slice(r.try_get::<&[u8], _>("webhook_id")?)?,
        event: r.try_get("event")?,
        payload: r.try_get("payload")?,
        status: r.try_get("status")?,
        attempts: r.try_get("attempts")?,
        next_at: r.try_get("next_at")?,
        response: r.try_get("response")?,
        error: r.try_get("error")?,
        created_at: r.try_get("created_at")?,
        finished_at: r.try_get("finished_at")?,
    })
}

const HOOK_SELECT: &str = "SELECT w.*, (SELECT d.status FROM webhook_deliveries d \
     WHERE d.webhook_id = w.id AND d.status <> 'pending' ORDER BY d.finished_at DESC, d.id DESC LIMIT 1) \
     AS last_status FROM webhooks w";

impl AppDb {
    pub async fn create_webhook(&self, user: Id, w: NewWebhook<'_>) -> Result<Webhook> {
        let id = Id::new();
        sqlx::query(
            "INSERT INTO webhooks (id, user_id, url, description, events, secret, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_bytes().as_slice())
        .bind(user.as_bytes().as_slice())
        .bind(w.url)
        .bind(w.description)
        .bind(w.events.join(" "))
        .bind(w.secret)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(self.webhook(user, id).await?.expect("webhook recién creado"))
    }

    pub async fn webhooks(&self, user: Id) -> Result<Vec<Webhook>> {
        let rows = sqlx::query(&format!("{HOOK_SELECT} WHERE w.user_id = ? ORDER BY w.id"))
            .bind(user.as_bytes().as_slice())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(webhook_from_row).collect()
    }

    pub async fn webhook(&self, user: Id, id: Id) -> Result<Option<Webhook>> {
        let row = sqlx::query(&format!("{HOOK_SELECT} WHERE w.id = ? AND w.user_id = ?"))
            .bind(id.as_bytes().as_slice())
            .bind(user.as_bytes().as_slice())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(webhook_from_row).transpose()
    }

    pub async fn update_webhook(&self, user: Id, id: Id, c: WebhookChange<'_>) -> Result<Option<Webhook>> {
        sqlx::query(
            "UPDATE webhooks SET url = COALESCE(?, url), description = COALESCE(?, description), \
             events = COALESCE(?, events), active = COALESCE(?, active) WHERE id = ? AND user_id = ?",
        )
        .bind(c.url)
        .bind(c.description)
        .bind(c.events.map(|e| e.join(" ")))
        .bind(c.active)
        .bind(id.as_bytes().as_slice())
        .bind(user.as_bytes().as_slice())
        .execute(&self.pool)
        .await?;
        self.webhook(user, id).await
    }

    pub async fn delete_webhook(&self, user: Id, id: Id) -> Result<bool> {
        let r = sqlx::query("DELETE FROM webhooks WHERE id = ? AND user_id = ?")
            .bind(id.as_bytes().as_slice())
            .bind(user.as_bytes().as_slice())
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Pone en cola un evento para cada webhook activo del usuario que lo quiera.
    /// `data` es JSON. Con `only`, solo para ese webhook (y aunque no lo quiera: es una prueba).
    /// Devuelve cuántos envíos quedaron en cola.
    pub async fn enqueue_event(&self, user: Id, event: &str, data: &str, only: Option<Id>) -> Result<usize> {
        let hooks: Vec<Webhook> = match only {
            Some(id) => self.webhook(user, id).await?.into_iter().collect(),
            None => self
                .webhooks(user)
                .await?
                .into_iter()
                .filter(|w| w.wants(event))
                .collect(),
        };
        if hooks.is_empty() {
            return Ok(0);
        }
        let t = now();
        let payload = format!(
            r#"{{"id":"{}","event":{},"created_at":{t},"data":{data}}}"#,
            Id::new(),
            serde_json::to_string(event).expect("texto"),
        );
        let mut tx = self.pool.begin().await?;
        for w in &hooks {
            sqlx::query(
                "INSERT INTO webhook_deliveries (id, webhook_id, event, payload, next_at, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(Id::new().as_bytes().as_slice())
            .bind(w.id.as_bytes().as_slice())
            .bind(event)
            .bind(&payload)
            .bind(t)
            .bind(t)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(hooks.len())
    }

    /// Envíos que toca hacer ya, con su webhook.
    pub async fn due_deliveries(&self, limit: i64) -> Result<Vec<(Delivery, Webhook)>> {
        let rows = sqlx::query(
            "SELECT d.* FROM webhook_deliveries d JOIN webhooks w ON w.id = d.webhook_id \
             WHERE d.status = 'pending' AND d.next_at <= ? AND w.active = 1 ORDER BY d.next_at, d.id LIMIT ?",
        )
        .bind(now())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let d = delivery_from_row(r)?;
            let row = sqlx::query(&format!("{HOOK_SELECT} WHERE w.id = ?"))
                .bind(d.webhook_id.as_bytes().as_slice())
                .fetch_one(&self.pool)
                .await?;
            out.push((d, webhook_from_row(&row)?));
        }
        Ok(out)
    }

    /// Cuándo toca el próximo envío pendiente.
    pub async fn next_delivery_at(&self) -> Result<Option<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT MIN(d.next_at) FROM webhook_deliveries d JOIN webhooks w ON w.id = d.webhook_id \
             WHERE d.status = 'pending' AND w.active = 1",
        )
        .fetch_one(&self.pool)
        .await?)
    }

    /// Avisos que aún esperan a entregarse (de webhooks activos).
    pub async fn pending_deliveries(&self) -> Result<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM webhook_deliveries d JOIN webhooks w ON w.id = d.webhook_id \
             WHERE d.status = 'pending' AND w.active = 1",
        )
        .fetch_one(&self.pool)
        .await?)
    }

    /// Apunta un intento. Si falla, programa el siguiente o lo da por perdido.
    pub async fn record_attempt(&self, id: Id, a: Attempt<'_>) -> Result<()> {
        let t = now();
        let attempts: i64 = sqlx::query_scalar("SELECT attempts FROM webhook_deliveries WHERE id = ?")
            .bind(id.as_bytes().as_slice())
            .fetch_optional(&self.pool)
            .await?
            .unwrap_or(0);
        let (status, next_at, finished_at) = if a.ok {
            ("ok", None, Some(t))
        } else {
            match RETRY_DELAYS.get(attempts as usize) {
                Some(d) => ("pending", Some(t + d), None),
                None => ("failed", None, Some(t)),
            }
        };
        sqlx::query(
            "UPDATE webhook_deliveries SET status = ?, attempts = attempts + 1, next_at = ?, \
             response = ?, error = ?, finished_at = ? WHERE id = ?",
        )
        .bind(status)
        .bind(next_at)
        .bind(a.response)
        .bind(a.error)
        .bind(finished_at)
        .bind(id.as_bytes().as_slice())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Historial de envíos de un webhook, del más reciente al más antiguo.
    pub async fn deliveries(&self, user: Id, webhook: Id, limit: i64) -> Result<Vec<Delivery>> {
        let rows = sqlx::query(
            "SELECT d.* FROM webhook_deliveries d JOIN webhooks w ON w.id = d.webhook_id \
             WHERE d.webhook_id = ? AND w.user_id = ? ORDER BY d.id DESC LIMIT ?",
        )
        .bind(webhook.as_bytes().as_slice())
        .bind(user.as_bytes().as_slice())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(delivery_from_row).collect()
    }

    /// Vuelve a enviar un evento ya enviado (o perdido): crea un envío nuevo con el mismo contenido.
    pub async fn redeliver(&self, user: Id, delivery: Id) -> Result<Option<Delivery>> {
        let t = now();
        let id = Id::new();
        let r = sqlx::query(
            "INSERT INTO webhook_deliveries (id, webhook_id, event, payload, next_at, created_at) \
             SELECT ?, d.webhook_id, d.event, d.payload, ?, ? FROM webhook_deliveries d \
             JOIN webhooks w ON w.id = d.webhook_id WHERE d.id = ? AND w.user_id = ?",
        )
        .bind(id.as_bytes().as_slice())
        .bind(t)
        .bind(t)
        .bind(delivery.as_bytes().as_slice())
        .bind(user.as_bytes().as_slice())
        .execute(&self.pool)
        .await?;
        if r.rows_affected() == 0 {
            return Ok(None);
        }
        let row = sqlx::query("SELECT * FROM webhook_deliveries WHERE id = ?")
            .bind(id.as_bytes().as_slice())
            .fetch_one(&self.pool)
            .await?;
        delivery_from_row(&row).map(Some)
    }

    /// Borra el historial de más de 30 días.
    pub async fn prune_deliveries(&self) -> Result<u64> {
        let r = sqlx::query("DELETE FROM webhook_deliveries WHERE status <> 'pending' AND finished_at < ?")
            .bind(now() - KEEP)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{db, new_user};

    #[tokio::test]
    async fn queue_retries_and_redelivers() {
        let (_d, db) = db().await;
        let u = db.create_user(new_user("ana")).await.unwrap();
        let other = db.create_user(new_user("luis")).await.unwrap();
        let events = vec!["message.received".to_string()];
        let hook = |url| NewWebhook {
            url,
            description: "",
            events: &events,
            secret: b"x",
        };
        let w = db.create_webhook(u.id, hook("https://a.example/")).await.unwrap();
        let off = db.create_webhook(u.id, hook("https://b.example/")).await.unwrap();
        db.update_webhook(
            u.id,
            off.id,
            WebhookChange {
                url: None,
                description: None,
                events: None,
                active: Some(false),
            },
        )
        .await
        .unwrap();
        assert!(db.webhooks(other.id).await.unwrap().is_empty());

        assert_eq!(db.enqueue_event(u.id, "message.sent", "{}", None).await.unwrap(), 0);
        assert_eq!(
            db.enqueue_event(u.id, "message.received", r#"{"n":1}"#, None)
                .await
                .unwrap(),
            1
        );
        let due = db.due_deliveries(10).await.unwrap();
        assert_eq!(due.len(), 1);
        let (d, hw) = &due[0];
        assert_eq!(hw.id, w.id);
        let p: serde_json::Value = serde_json::from_str(&d.payload).unwrap();
        assert_eq!(p["event"], "message.received");
        assert_eq!(p["data"]["n"], 1);

        // Cada fallo programa el siguiente intento; al final, se da por perdido.
        for _ in 0..RETRY_DELAYS.len() {
            db.record_attempt(
                d.id,
                Attempt {
                    ok: false,
                    response: Some(500),
                    error: None,
                },
            )
            .await
            .unwrap();
            assert!(db.due_deliveries(10).await.unwrap().is_empty(), "espera al reintento");
        }
        db.record_attempt(
            d.id,
            Attempt {
                ok: false,
                response: None,
                error: Some("timeout"),
            },
        )
        .await
        .unwrap();
        let list = db.deliveries(u.id, w.id, 10).await.unwrap();
        assert_eq!((list[0].status.as_str(), list[0].attempts), ("failed", 7));
        assert_eq!(
            db.webhook(u.id, w.id).await.unwrap().unwrap().last_status.as_deref(),
            Some("failed")
        );
        assert!(db.next_delivery_at().await.unwrap().is_none());

        // Reenviar crea un envío nuevo con el mismo contenido.
        assert!(db.redeliver(other.id, d.id).await.unwrap().is_none(), "de otro usuario");
        let again = db.redeliver(u.id, d.id).await.unwrap().unwrap();
        assert_eq!(again.payload, d.payload);
        db.record_attempt(
            again.id,
            Attempt {
                ok: true,
                response: Some(200),
                error: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(db.deliveries(u.id, w.id, 10).await.unwrap()[0].status, "ok");

        // La prueba llega aunque el webhook no esté suscrito al evento.
        assert_eq!(db.enqueue_event(u.id, PING, "{}", Some(w.id)).await.unwrap(), 1);
        assert!(db.delete_webhook(u.id, w.id).await.unwrap());
        assert!(db.due_deliveries(10).await.unwrap().is_empty());
    }
}

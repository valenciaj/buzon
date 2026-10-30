//! El flujo del modo Enfoque, inspirado en Hey: filtro de remitentes, bandejas por
//! intención (Principal, Novedades y Recibos), pilas (Responder luego y Apartados),
//! hilos pospuestos que reaparecen y notas privadas.
//!
//! Todo vive en Buzón; el servidor IMAP no se entera.

use sqlx::{QueryBuilder, Row, Sqlite};

use crate::mail::{b, Addr};
use crate::{now, Id, Result, UserDb};

/// Dónde va el correo aprobado de un remitente.
pub const BUCKETS: [&str; 3] = ["principal", "news", "receipts"];

/// Lo que se decide sobre un remitente.
pub const STATUSES: [&str; 3] = ["pending", "approved", "blocked"];

/// Bandeja o pila que se lista.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Principal,
    News,
    Receipts,
    /// Correo de remitentes nuevos, por aprobar.
    Screener,
    /// Correo de remitentes rechazados.
    Blocked,
    ReplyLater,
    Aside,
    /// Pospuesto, aún sin reaparecer.
    Snoozed,
    /// Pospuesto que ya ha reaparecido y sigue en la entrada.
    Bubbled,
}

impl View {
    pub fn parse(s: &str) -> Option<View> {
        Some(match s {
            "principal" => View::Principal,
            "news" => View::News,
            "receipts" => View::Receipts,
            "screener" => View::Screener,
            "blocked" => View::Blocked,
            "reply_later" => View::ReplyLater,
            "aside" => View::Aside,
            "snoozed" => View::Snoozed,
            "bubbled" => View::Bubbled,
            _ => return None,
        })
    }

    /// Añade a la consulta de hilos (alias `t`) las condiciones de esta vista.
    pub(crate) fn push_conditions(self, s: &mut QueryBuilder<Sqlite>, now: i64) {
        // Dónde va el hilo según quién escribió lo último que hay en la entrada:
        // su bandeja si está aprobado (o no se sabe nada de él), o `pending`/`blocked`.
        // Sin nada en la entrada, NULL.
        const PLACE: &str = "(SELECT CASE WHEN s.status IS NULL OR s.status = 'approved' \
             THEN COALESCE(s.bucket, 'principal') ELSE s.status END \
             FROM messages m2 JOIN mailboxes mb2 ON mb2.id = m2.mailbox_id \
             LEFT JOIN senders s ON s.email = m2.from_addr \
             WHERE m2.thread_id = t.id AND mb2.role = 'inbox' ORDER BY m2.date DESC LIMIT 1)";
        // Las pilas no muestran lo que ya está en la papelera o en spam.
        const ALIVE: &str = " AND EXISTS (SELECT 1 FROM messages m3 JOIN mailboxes mb3 ON mb3.id = m3.mailbox_id \
             WHERE m3.thread_id = t.id AND COALESCE(mb3.role, '') NOT IN ('trash', 'junk'))";
        match self {
            View::Principal | View::News | View::Receipts => {
                let bucket = match self {
                    View::Principal => "principal",
                    View::News => "news",
                    _ => "receipts",
                };
                s.push(format!(" AND {PLACE} = "))
                    .push_bind(bucket)
                    .push(" AND t.reply_later IS NULL AND t.aside IS NULL AND t.snooze_until IS NULL");
            }
            View::Screener => {
                s.push(format!(" AND {PLACE} = 'pending'"));
            }
            View::Blocked => {
                s.push(format!(" AND {PLACE} = 'blocked'"));
            }
            View::ReplyLater | View::Aside => {
                let col = if self == View::ReplyLater {
                    "reply_later"
                } else {
                    "aside"
                };
                s.push(format!(
                    " AND t.{col} IS NOT NULL AND (t.snooze_until IS NULL OR t.snooze_until <= "
                ))
                .push_bind(now)
                .push(")")
                .push(ALIVE);
            }
            View::Snoozed => {
                s.push(" AND t.snooze_until > ").push_bind(now).push(ALIVE);
            }
            View::Bubbled => {
                s.push(" AND t.snooze_until <= ")
                    .push_bind(now)
                    .push(format!(" AND {PLACE} NOT IN ('pending', 'blocked')"));
            }
        }
    }
}

/// Estado de un hilo en el flujo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadFlow {
    pub reply_later: bool,
    pub aside: bool,
    pub snooze_until: Option<i64>,
    pub has_note: bool,
    /// Algún mensaje traía píxeles espía (ya quitados).
    pub tracked: bool,
}

/// Cambios en las pilas o en lo pospuesto. `None` deja las cosas como están.
#[derive(Debug, Clone, Copy, Default)]
pub struct FlowChange {
    pub reply_later: Option<bool>,
    pub aside: Option<bool>,
    /// `Some(None)` quita lo pospuesto.
    pub snooze: Option<Option<i64>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sender {
    pub email: String,
    pub name: String,
    pub status: String,
    pub bucket: String,
}

/// Un remitente por aprobar (o ya decidido) con lo último que ha escrito.
#[derive(Debug, Clone)]
pub struct SenderSummary {
    pub sender: Sender,
    pub threads: i64,
    pub last_thread: Option<Id>,
    pub last_subject: String,
    pub last_date: i64,
}

/// Cuántos hilos hay en cada sitio, para la barra lateral.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlowCounts {
    pub screener: i64,
    pub reply_later: i64,
    pub aside: i64,
    pub snoozed: i64,
    pub bubbled: i64,
    /// Hilos con algo sin leer en cada bandeja.
    pub principal_unread: i64,
    pub news_unread: i64,
    pub receipts_unread: i64,
}

/// Qué parece un mensaje por sus cabeceras y su asunto.
pub fn guess_kind(list_header: bool, bulk: bool, from: &str, subject: &str) -> &'static str {
    let subject = subject.to_lowercase();
    let from = from.to_lowercase();
    const RECEIPT_WORDS: [&str; 16] = [
        "factura",
        "recibo",
        "pedido",
        "tu compra",
        "confirmación de compra",
        "envío",
        "enviado",
        "invoice",
        "receipt",
        "your order",
        "order confirmation",
        "has shipped",
        "shipping",
        "payment",
        "pago",
        "billing",
    ];
    const RECEIPT_SENDERS: [&str; 6] = ["billing@", "invoice", "facturacion", "receipts@", "orders@", "pedidos@"];
    if RECEIPT_WORDS.iter().any(|w| subject.contains(w)) || RECEIPT_SENDERS.iter().any(|w| from.contains(w)) {
        "receipts"
    } else if list_header || bulk {
        "news"
    } else {
        "principal"
    }
}

impl UserDb {
    /// Apunta al remitente de un correo recién llegado a la entrada, si es nuevo.
    /// Lo que llega en la primera descarga, lo de los contactos y lo de quien ya ha
    /// recibido algo nuestro queda aprobado; el resto espera en «Por aprobar».
    /// Devuelve si el remitente queda por aprobar.
    pub async fn screen_sender(&self, from: &Addr, kind: &str, fresh: bool) -> Result<bool> {
        let email = from.email.trim().to_lowercase();
        if email.is_empty() {
            return Ok(false);
        }
        let own: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM identities WHERE email = ?1 COLLATE NOCASE) \
             OR EXISTS (SELECT 1 FROM accounts WHERE email = ?1 COLLATE NOCASE)",
        )
        .bind(&email)
        .fetch_one(&self.pool)
        .await?;
        if own {
            return Ok(false);
        }
        let known: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM contacts WHERE email = ? COLLATE NOCASE)")
            .bind(&email)
            .fetch_one(&self.pool)
            .await?;
        let status = if fresh && !known { "pending" } else { "approved" };
        let bucket = if BUCKETS.contains(&kind) { kind } else { "principal" };
        sqlx::query(
            "INSERT INTO senders (email, name, status, bucket, created_at) VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT (email) DO NOTHING",
        )
        .bind(&email)
        .bind(from.name.as_deref().unwrap_or("").trim())
        .bind(status)
        .bind(bucket)
        .bind(now())
        .execute(&self.pool)
        .await?;
        let status: String = sqlx::query_scalar("SELECT status FROM senders WHERE email = ?")
            .bind(&email)
            .fetch_one(&self.pool)
            .await?;
        Ok(status == "pending")
    }

    /// Aprueba, rechaza o cambia de bandeja a un remitente (lo crea si no estaba).
    /// `bucket` vacío deja la que tenía.
    pub async fn decide_sender(&self, email: &str, status: &str, bucket: &str) -> Result<Sender> {
        let email = email.trim().to_lowercase();
        let name: Option<String> = sqlx::query_scalar(
            "SELECT from_name FROM messages WHERE from_addr = ? COLLATE NOCASE AND from_name IS NOT NULL \
             ORDER BY date DESC LIMIT 1",
        )
        .bind(&email)
        .fetch_optional(&self.pool)
        .await?
        .flatten();
        sqlx::query(
            "INSERT INTO senders (email, name, status, bucket, created_at, decided_at) \
             VALUES (?1, ?2, ?3, CASE WHEN ?4 = '' THEN 'principal' ELSE ?4 END, ?5, ?5) \
             ON CONFLICT (email) DO UPDATE SET status = excluded.status, decided_at = excluded.decided_at, \
             bucket = CASE WHEN ?4 = '' THEN senders.bucket ELSE ?4 END",
        )
        .bind(&email)
        .bind(name.unwrap_or_default())
        .bind(status)
        .bind(bucket)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(self.sender(&email).await?.expect("recién guardado"))
    }

    pub async fn sender(&self, email: &str) -> Result<Option<Sender>> {
        let r = sqlx::query("SELECT email, name, status, bucket FROM senders WHERE email = ?")
            .bind(email.trim())
            .fetch_optional(&self.pool)
            .await?;
        r.map(|r| sender_from_row(&r)).transpose()
    }

    /// Quien escribió lo último que hay del hilo en la entrada (o, si no hay nada en
    /// la entrada, lo último recibido), con lo decidido sobre él.
    pub async fn thread_sender(&self, thread: Id) -> Result<Option<Sender>> {
        let r = sqlx::query(
            "SELECT m.from_addr, COALESCE(m.from_name, '') AS from_name, s.status, s.bucket \
             FROM messages m JOIN mailboxes mb ON mb.id = m.mailbox_id LEFT JOIN senders s ON s.email = m.from_addr \
             WHERE m.thread_id = ? AND m.from_addr IS NOT NULL AND COALESCE(mb.role, '') NOT IN ('sent', 'drafts') \
             ORDER BY (mb.role = 'inbox') DESC, m.date DESC LIMIT 1",
        )
        .bind(b(thread))
        .fetch_optional(&self.pool)
        .await?;
        let Some(r) = r else { return Ok(None) };
        let status: Option<String> = r.try_get("status")?;
        let Some(status) = status else { return Ok(None) };
        Ok(Some(Sender {
            email: r.try_get("from_addr")?,
            name: r.try_get("from_name")?,
            status,
            bucket: r.try_get("bucket")?,
        }))
    }

    /// Remitentes con correo en la entrada, con lo último que han escrito.
    /// Sin `status`, todos.
    pub async fn senders(&self, status: Option<&str>) -> Result<Vec<SenderSummary>> {
        let rows = sqlx::query(
            "SELECT s.email, s.name, s.status, s.bucket, COUNT(DISTINCT m.thread_id) AS threads, MAX(m.date) AS last_date, \
             (SELECT m4.thread_id FROM messages m4 WHERE m4.from_addr = s.email ORDER BY m4.date DESC LIMIT 1) AS last_thread, \
             (SELECT m4.subject FROM messages m4 WHERE m4.from_addr = s.email ORDER BY m4.date DESC LIMIT 1) AS last_subject \
             FROM senders s JOIN messages m ON m.from_addr = s.email JOIN mailboxes mb ON mb.id = m.mailbox_id \
             WHERE mb.role = 'inbox' AND (?1 IS NULL OR s.status = ?1) \
             GROUP BY s.email ORDER BY last_date DESC",
        )
        .bind(status)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                let last: Option<Vec<u8>> = r.try_get("last_thread")?;
                Ok(SenderSummary {
                    sender: sender_from_row(r)?,
                    threads: r.try_get("threads")?,
                    last_thread: last.map(|v| Id::from_slice(&v)).transpose()?,
                    last_subject: r.try_get::<Option<String>, _>("last_subject")?.unwrap_or_default(),
                    last_date: r.try_get::<Option<i64>, _>("last_date")?.unwrap_or(0),
                })
            })
            .collect()
    }

    /// Mete o saca hilos de las pilas y los pospone. Devuelve cuántos existían.
    pub async fn set_flow(&self, threads: &[Id], change: FlowChange) -> Result<u64> {
        let t = now();
        let mut tx = self.pool.begin().await?;
        let mut found = 0;
        for id in threads {
            let mut q: QueryBuilder<Sqlite> = QueryBuilder::new("UPDATE threads SET id = id");
            if let Some(v) = change.reply_later {
                q.push(", reply_later = ").push_bind(v.then_some(t));
            }
            if let Some(v) = change.aside {
                q.push(", aside = ").push_bind(v.then_some(t));
            }
            if let Some(v) = change.snooze {
                q.push(", snooze_until = ").push_bind(v);
            }
            q.push(" WHERE id = ").push_bind(b(*id));
            found += q.build().execute(&mut *tx).await?.rows_affected();
        }
        tx.commit().await?;
        Ok(found)
    }

    /// Lo que ha reaparecido deja de estar arriba al moverlo.
    pub async fn clear_bubbled(&self, threads: &[Id]) -> Result<()> {
        for id in threads {
            sqlx::query("UPDATE threads SET snooze_until = NULL WHERE id = ? AND snooze_until <= ?")
                .bind(b(*id))
                .bind(now())
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    /// Al responder, el hilo sale de «Responder luego».
    pub async fn replied(&self, message: Id) -> Result<()> {
        sqlx::query("UPDATE threads SET reply_later = NULL WHERE id = (SELECT thread_id FROM messages WHERE id = ?)")
            .bind(b(message))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Guarda la nota privada de un hilo. Devuelve si el hilo existe.
    pub async fn set_note(&self, thread: Id, note: &str) -> Result<bool> {
        let r = sqlx::query("UPDATE threads SET note = ? WHERE id = ?")
            .bind(note.trim())
            .bind(b(thread))
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn flow_counts(&self, account: Option<Id>) -> Result<FlowCounts> {
        let t = now();
        let count = |view: View, unread: bool| {
            let mut s: QueryBuilder<Sqlite> = QueryBuilder::new("SELECT COUNT(*) FROM threads t WHERE 1");
            view.push_conditions(&mut s, t);
            if unread {
                s.push(" AND EXISTS (SELECT 1 FROM messages mu WHERE mu.thread_id = t.id AND mu.seen = 0)");
            }
            if let Some(a) = account {
                s.push(" AND t.account_id = ").push_bind(b(a));
            }
            s
        };
        let mut out = FlowCounts::default();
        for (view, unread, slot) in [
            (View::Screener, false, &mut out.screener),
            (View::ReplyLater, false, &mut out.reply_later),
            (View::Aside, false, &mut out.aside),
            (View::Snoozed, false, &mut out.snoozed),
            (View::Bubbled, false, &mut out.bubbled),
            (View::Principal, true, &mut out.principal_unread),
            (View::News, true, &mut out.news_unread),
            (View::Receipts, true, &mut out.receipts_unread),
        ] {
            *slot = count(view, unread).build_query_scalar().fetch_one(&self.pool).await?;
        }
        Ok(out)
    }
}

fn sender_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Sender> {
    Ok(Sender {
        email: r.try_get("email")?,
        name: r.try_get("name")?,
        status: r.try_get("status")?,
        bucket: r.try_get("bucket")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::ThreadQuery;
    use crate::organize::tests::{db, msg};

    fn view(v: View) -> ThreadQuery {
        ThreadQuery {
            view: Some(v),
            now: now(),
            limit: 50,
            ..Default::default()
        }
    }

    async fn subjects(db: &UserDb, v: View) -> Vec<String> {
        db.threads(&view(v))
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.subject)
            .collect()
    }

    #[test]
    fn guesses_kind() {
        assert_eq!(guess_kind(false, false, "hola@tienda.es", "Tu pedido 4815"), "receipts");
        assert_eq!(guess_kind(true, false, "billing@saas.example", "Novedades"), "receipts");
        assert_eq!(guess_kind(true, false, "news@lista.es", "Boletín de octubre"), "news");
        assert_eq!(guess_kind(false, true, "x@y.es", "Hola"), "news");
        assert_eq!(guess_kind(false, false, "ana@ejemplo.es", "Cena"), "principal");
    }

    #[tokio::test]
    async fn screener_buckets_piles_and_notes() {
        let (_d, db, acc) = db().await;
        let inbox = db.upsert_mailbox(acc, "INBOX", Some("/"), Some("inbox")).await.unwrap();
        let trash = db.upsert_mailbox(acc, "Trash", Some("/"), Some("trash")).await.unwrap();
        let add = |uid: u32, subject: &str, from: &str, kind: &str| {
            let mut m = msg(acc, inbox.id, uid, &format!("m{uid}@x"), subject, from, "texto");
            m.kind = kind.into();
            m
        };

        // Primera descarga: todo aprobado, cada uno en su bandeja.
        for (m, fresh) in [
            (add(1, "Cena", "ana@ejemplo.es", "principal"), false),
            (add(2, "Boletín", "news@lista.es", "news"), false),
            (add(3, "Tu pedido", "hola@tienda.es", "receipts"), false),
        ] {
            db.insert_message(&m).await.unwrap();
            assert!(!db
                .screen_sender(m.from.as_ref().unwrap(), &m.kind, fresh)
                .await
                .unwrap());
        }
        // Después: el remitente nuevo espera; el conocido no.
        db.save_book_entry(&crate::organize::BookEntry {
            id: None,
            name: "Luis".into(),
            email: "luis@estudio.es".into(),
            phone: String::new(),
            notes: String::new(),
            source: "manual".into(),
        })
        .await
        .unwrap();
        for (uid, subject, from) in [(4, "Hola", "nuevo@desconocido.es"), (5, "Planos", "Luis@Estudio.es")] {
            let m = add(uid, subject, from, "principal");
            db.insert_message(&m).await.unwrap();
            let pending = db
                .screen_sender(m.from.as_ref().unwrap(), "principal", true)
                .await
                .unwrap();
            assert_eq!(pending, uid == 4);
        }
        assert_eq!(subjects(&db, View::Principal).await, ["Planos", "Cena"]);
        assert_eq!(subjects(&db, View::News).await, ["Boletín"]);
        assert_eq!(subjects(&db, View::Receipts).await, ["Tu pedido"]);
        assert_eq!(subjects(&db, View::Screener).await, ["Hola"]);
        let pending = db.senders(Some("pending")).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].sender.email, "nuevo@desconocido.es");
        assert_eq!(pending[0].last_subject, "Hola");
        let c = db.flow_counts(None).await.unwrap();
        assert_eq!((c.screener, c.principal_unread, c.news_unread), (1, 2, 1));

        // Aprobar a Novedades; rechazar; responder aprueba.
        db.decide_sender("nuevo@desconocido.es", "approved", "news")
            .await
            .unwrap();
        assert_eq!(subjects(&db, View::News).await, ["Hola", "Boletín"]);
        db.decide_sender("news@lista.es", "blocked", "").await.unwrap();
        assert_eq!(subjects(&db, View::News).await, ["Hola"]);
        assert_eq!(subjects(&db, View::Blocked).await, ["Boletín"]);
        assert_eq!(db.sender("news@lista.es").await.unwrap().unwrap().bucket, "news");
        let m = add(6, "Oferta", "otro@nuevo.es", "principal");
        db.insert_message(&m).await.unwrap();
        assert!(db
            .screen_sender(m.from.as_ref().unwrap(), "principal", true)
            .await
            .unwrap());
        db.remember_recipients(&[m.from.clone().unwrap()]).await.unwrap();
        assert_eq!(db.sender("otro@nuevo.es").await.unwrap().unwrap().status, "approved");
        let oferta = db.threads(&view(View::Principal)).await.unwrap()[0].id;
        assert_eq!(db.thread_sender(oferta).await.unwrap().unwrap().email, "otro@nuevo.es");

        // Pilas: salen de la bandeja y vuelven al quitarlas.
        let cena = db
            .threads(&view(View::Principal))
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.subject == "Cena")
            .unwrap();
        db.set_flow(
            &[cena.id],
            FlowChange {
                reply_later: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(!subjects(&db, View::Principal).await.contains(&"Cena".to_string()));
        let rl = db.threads(&view(View::ReplyLater)).await.unwrap();
        assert_eq!(rl.len(), 1);
        assert!(rl[0].flow.reply_later && !rl[0].flow.aside);
        // Responder lo saca de la pila.
        let first = db.thread(cena.id).await.unwrap().unwrap().messages[0].id;
        db.replied(first).await.unwrap();
        assert!(subjects(&db, View::ReplyLater).await.is_empty());
        assert!(subjects(&db, View::Principal).await.contains(&"Cena".to_string()));

        db.set_flow(
            &[cena.id],
            FlowChange {
                aside: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(subjects(&db, View::Aside).await, ["Cena"]);
        // Lo que está en la papelera no sale en las pilas.
        let planos = db
            .threads(&view(View::Principal))
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.subject == "Planos")
            .unwrap();
        db.set_flow(
            &[planos.id],
            FlowChange {
                aside: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        db.move_messages(&[planos.id], &[], None, &crate::organize::Place::Mailbox(trash.id))
            .await
            .unwrap();
        assert_eq!(subjects(&db, View::Aside).await, ["Cena"]);
        db.set_flow(
            &[cena.id],
            FlowChange {
                aside: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Pospuesto: desaparece y, pasada la fecha, reaparece arriba.
        db.set_flow(
            &[cena.id],
            FlowChange {
                snooze: Some(Some(now() + 3600)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(subjects(&db, View::Snoozed).await, ["Cena"]);
        assert!(!subjects(&db, View::Principal).await.contains(&"Cena".to_string()));
        db.set_flow(
            &[cena.id],
            FlowChange {
                snooze: Some(Some(now() - 1)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(subjects(&db, View::Snoozed).await.is_empty());
        assert_eq!(subjects(&db, View::Bubbled).await, ["Cena"]);
        assert_eq!(db.flow_counts(None).await.unwrap().bubbled, 1);
        db.clear_bubbled(&[cena.id]).await.unwrap();
        assert!(subjects(&db, View::Bubbled).await.is_empty());
        assert!(subjects(&db, View::Principal).await.contains(&"Cena".to_string()));

        // Nota privada; se conserva al fusionar hilos.
        assert!(db.set_note(cena.id, "  Llevar vino  ").await.unwrap());
        let t = db.thread(cena.id).await.unwrap().unwrap();
        assert_eq!(t.note, "Llevar vino");
        assert!(t.flow.has_note);
        assert!(!db.set_note(Id::new(), "x").await.unwrap());
    }
}

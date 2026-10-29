//! Caché local del correo: carpetas, hilos, mensajes y cuerpos saneados.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Row, Sqlite, SqliteConnection};

use crate::{Id, Result, UserDb};

/// Una dirección de correo con su nombre visible.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Addr {
    pub name: Option<String>,
    pub email: String,
}

/// Adjunto de un mensaje. `index` es su posición entre los adjuntos del original.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Attachment {
    pub index: u32,
    pub name: String,
    pub content_type: String,
    pub size: u64,
    pub content_id: Option<String>,
    /// Imagen incrustada en el HTML (no se lista como adjunto).
    pub inline: bool,
}

#[derive(Debug, Clone)]
pub struct Mailbox {
    pub id: Id,
    pub account_id: Id,
    /// Nombre IMAP tal cual (UTF-7 modificado).
    pub name: String,
    pub delimiter: Option<String>,
    pub role: Option<String>,
    pub uidvalidity: Option<u32>,
    pub last_uid: u32,
    /// UID más antiguo descargado; `Some(0)` cuando ya está todo el historial.
    pub oldest_uid: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct MailboxCounts {
    pub mailbox: Mailbox,
    pub total: i64,
    pub unread: i64,
}

/// Mensaje recién descargado, ya analizado y saneado.
#[derive(Debug, Clone)]
pub struct NewMessage {
    pub id: Id,
    pub account_id: Id,
    pub mailbox_id: Id,
    pub uid: u32,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub refs: Vec<String>,
    pub subject: String,
    pub from: Option<Addr>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub date: i64,
    pub snippet: String,
    pub size: u32,
    pub flags: Flags,
    pub text: Option<String>,
    pub html: Option<String>,
    pub remote_images: u32,
    pub attachments: Vec<Attachment>,
    /// Qué parece por sus cabeceras: `principal`, `news` o `receipts`.
    pub kind: String,
    /// Dominios de los píxeles espía quitados del HTML.
    pub trackers: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
}

/// Qué hilos listar: una carpeta concreta o un tipo de carpeta (por ejemplo
/// `inbox`) en una cuenta o en todas.
#[derive(Debug, Clone, Default)]
pub struct ThreadQuery {
    pub account: Option<Id>,
    pub mailbox: Option<Id>,
    pub role: Option<String>,
    /// Cursor: hilos anteriores a esta fecha e id.
    pub before: Option<(i64, Id)>,
    pub limit: u32,
    /// Solo hilos con esta etiqueta.
    pub label: Option<Id>,
    /// Búsqueda: algún mensaje del hilo debe cumplirla entera.
    pub search: Option<crate::organize::Search>,
    /// Bandeja o pila del modo Enfoque.
    pub view: Option<crate::flow::View>,
    /// Ahora, para lo pospuesto.
    pub now: i64,
}

#[derive(Debug, Clone)]
pub struct ThreadSummary {
    pub id: Id,
    pub account_id: Id,
    pub subject: String,
    pub last_date: i64,
    pub participants: Vec<String>,
    pub snippet: String,
    pub messages: i64,
    pub unread: i64,
    pub flagged: bool,
    pub has_attachments: bool,
    pub labels: Vec<Id>,
    pub flow: crate::flow::ThreadFlow,
}

#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub id: Id,
    pub mailbox_id: Id,
    pub uid: u32,
    pub message_id: Option<String>,
    pub subject: String,
    pub from: Option<Addr>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub date: i64,
    pub flags: Flags,
    pub text: Option<String>,
    pub html: Option<String>,
    pub remote_images: u32,
    pub attachments: Vec<Attachment>,
    pub trackers: Vec<String>,
}

/// Resumen de un mensaje para los webhooks.
#[derive(Debug, Clone, Serialize)]
pub struct MessageBrief {
    pub id: Id,
    pub thread: Id,
    pub account: Id,
    pub mailbox: String,
    pub subject: String,
    pub from: Option<Addr>,
    pub to: Vec<Addr>,
    pub date: i64,
    pub snippet: String,
    /// `principal`, `news` o `receipts`.
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct Thread {
    pub id: Id,
    pub account_id: Id,
    pub subject: String,
    pub messages: Vec<StoredMessage>,
    pub flow: crate::flow::ThreadFlow,
    pub note: String,
}

/// Mensaje que hay que cambiar también en el servidor IMAP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapTarget {
    pub account_id: Id,
    pub mailbox: String,
    pub uid: u32,
}

pub(crate) fn b(id: Id) -> Vec<u8> {
    id.as_bytes().to_vec()
}

pub(crate) fn id_of(r: &sqlx::sqlite::SqliteRow, col: &str) -> Result<Id> {
    Id::from_slice(r.try_get::<&[u8], _>(col)?)
}

fn mailbox_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Mailbox> {
    Ok(Mailbox {
        id: id_of(r, "id")?,
        account_id: id_of(r, "account_id")?,
        name: r.try_get("name")?,
        delimiter: r.try_get("delimiter")?,
        role: r.try_get("role")?,
        uidvalidity: r.try_get::<Option<i64>, _>("uidvalidity")?.map(|v| v as u32),
        last_uid: r.try_get::<i64, _>("last_uid")? as u32,
        oldest_uid: r.try_get::<Option<i64>, _>("oldest_uid")?.map(|v| v as u32),
    })
}

/// Quita los «Re:», «Fwd:», «RV:»… del principio del asunto.
pub fn base_subject(s: &str) -> &str {
    let mut s = s.trim();
    loop {
        let lower = s.to_ascii_lowercase();
        let Some(p) = ["re:", "fw:", "fwd:", "rv:", "aw:", "wg:", "tr:", "res:", "enc:"]
            .iter()
            .find(|p| lower.starts_with(*p))
        else {
            return s;
        };
        s = s[p.len()..].trim_start();
    }
}

/// Decodifica un nombre de carpeta IMAP (UTF-7 modificado, RFC 3501 §5.1.3).
pub fn decode_mailbox_name(name: &str) -> String {
    let mut out = String::new();
    let mut rest = name;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('-') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let chunk = &after[..end];
        if chunk.is_empty() {
            out.push('&');
        } else {
            match decode_b64(chunk) {
                Some(bytes) => {
                    let units: Vec<u16> = bytes
                        .chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    out.push_str(&String::from_utf16_lossy(&units));
                }
                None => out.push_str(&rest[start..start + end + 2]),
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn decode_b64(s: &str) -> Option<Vec<u8>> {
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = Vec::new();
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b',' => 63,
            _ => return None,
        } as u32;
        bits = (bits << 6) | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
            bits &= (1 << n) - 1;
        }
    }
    Some(out)
}

pub(crate) async fn delete_messages_tx(tx: &mut SqliteConnection, ids: &[Id]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut threads: HashSet<Vec<u8>> = HashSet::new();
    for chunk in ids.chunks(500) {
        let mut q: QueryBuilder<Sqlite> = QueryBuilder::new("SELECT DISTINCT thread_id FROM messages WHERE id IN (");
        let mut sep = q.separated(", ");
        for id in chunk {
            sep.push_bind(b(*id));
        }
        q.push(")");
        for t in q.build_query_scalar::<Vec<u8>>().fetch_all(&mut *tx).await? {
            threads.insert(t);
        }
        let mut q: QueryBuilder<Sqlite> = QueryBuilder::new("DELETE FROM messages WHERE id IN (");
        let mut sep = q.separated(", ");
        for id in chunk {
            sep.push_bind(b(*id));
        }
        q.push(")");
        q.build().execute(&mut *tx).await?;
    }
    for t in threads {
        let last: Option<i64> = sqlx::query_scalar("SELECT MAX(date) FROM messages WHERE thread_id = ?")
            .bind(&t)
            .fetch_one(&mut *tx)
            .await?;
        match last {
            Some(d) => sqlx::query("UPDATE threads SET last_date = ? WHERE id = ?").bind(d),
            None => sqlx::query("DELETE FROM threads WHERE id = ?"),
        }
        .bind(&t)
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

impl UserDb {
    /// Crea o actualiza una carpeta y la devuelve con su estado de sincronización.
    pub async fn upsert_mailbox(
        &self,
        account: Id,
        name: &str,
        delimiter: Option<&str>,
        role: Option<&str>,
    ) -> Result<Mailbox> {
        sqlx::query(
            "INSERT INTO mailboxes (id, account_id, name, delimiter, role) VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT (account_id, name) DO UPDATE SET delimiter = excluded.delimiter, role = excluded.role",
        )
        .bind(b(Id::new()))
        .bind(b(account))
        .bind(name)
        .bind(delimiter)
        .bind(role)
        .execute(&self.pool)
        .await?;
        let row = sqlx::query("SELECT * FROM mailboxes WHERE account_id = ? AND name = ?")
            .bind(b(account))
            .bind(name)
            .fetch_one(&self.pool)
            .await?;
        mailbox_from_row(&row)
    }

    pub async fn mailbox(&self, id: Id) -> Result<Option<Mailbox>> {
        let row = sqlx::query("SELECT * FROM mailboxes WHERE id = ?")
            .bind(b(id))
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(mailbox_from_row).transpose()
    }

    /// Borra las carpetas que ya no existen en el servidor. Devuelve los mensajes borrados.
    pub async fn retain_mailboxes(&self, account: Id, keep: &[String]) -> Result<Vec<Id>> {
        let mut deleted = Vec::new();
        for mb in self.mailboxes(Some(account)).await? {
            if !keep.contains(&mb.mailbox.name) {
                deleted.extend(self.clear_mailbox(mb.mailbox.id).await?);
                sqlx::query("DELETE FROM mailboxes WHERE id = ?")
                    .bind(b(mb.mailbox.id))
                    .execute(&self.pool)
                    .await?;
            }
        }
        Ok(deleted)
    }

    /// Carpetas con su número de mensajes y no leídos.
    pub async fn mailboxes(&self, account: Option<Id>) -> Result<Vec<MailboxCounts>> {
        let mut q: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT b.*, COUNT(m.id) AS total, COALESCE(SUM(m.seen = 0), 0) AS unread \
             FROM mailboxes b LEFT JOIN messages m ON m.mailbox_id = b.id",
        );
        if let Some(a) = account {
            q.push(" WHERE b.account_id = ").push_bind(b(a));
        }
        q.push(" GROUP BY b.id ORDER BY b.account_id, b.name");
        let rows = q.build().fetch_all(&self.pool).await?;
        rows.iter()
            .map(|r| {
                Ok(MailboxCounts {
                    mailbox: mailbox_from_row(r)?,
                    total: r.try_get("total")?,
                    unread: r.try_get("unread")?,
                })
            })
            .collect()
    }

    /// Vacía la caché de una carpeta (por ejemplo, si cambia su UIDVALIDITY).
    pub async fn clear_mailbox(&self, mailbox: Id) -> Result<Vec<Id>> {
        let ids: Vec<Vec<u8>> = sqlx::query_scalar("SELECT id FROM messages WHERE mailbox_id = ?")
            .bind(b(mailbox))
            .fetch_all(&self.pool)
            .await?;
        let ids = ids.iter().map(|v| Id::from_slice(v)).collect::<Result<Vec<_>>>()?;
        self.delete_messages(&ids).await?;
        sqlx::query("UPDATE mailboxes SET last_uid = 0, oldest_uid = NULL, uidvalidity = NULL WHERE id = ?")
            .bind(b(mailbox))
            .execute(&self.pool)
            .await?;
        Ok(ids)
    }

    pub async fn set_mailbox_state(
        &self,
        mailbox: Id,
        uidvalidity: u32,
        last_uid: u32,
        oldest_uid: Option<u32>,
    ) -> Result<()> {
        sqlx::query("UPDATE mailboxes SET uidvalidity = ?, last_uid = ?, oldest_uid = ? WHERE id = ?")
            .bind(uidvalidity as i64)
            .bind(last_uid as i64)
            .bind(oldest_uid.map(|v| v as i64))
            .bind(b(mailbox))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// UIDs ya guardados de una carpeta, con sus flags.
    pub async fn known_uids(&self, mailbox: Id) -> Result<Vec<(u32, Id, Flags)>> {
        let rows = sqlx::query(
            "SELECT uid, id, seen, flagged, answered FROM messages WHERE mailbox_id = ? AND uid > 0 ORDER BY uid",
        )
        .bind(b(mailbox))
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok((
                    r.try_get::<i64, _>("uid")? as u32,
                    id_of(r, "id")?,
                    Flags {
                        seen: r.try_get("seen")?,
                        flagged: r.try_get("flagged")?,
                        answered: r.try_get("answered")?,
                    },
                ))
            })
            .collect()
    }

    /// Guarda un mensaje y lo agrupa en su hilo por `Message-ID`, `In-Reply-To` y `References`.
    pub async fn insert_message(&self, m: &NewMessage) -> Result<Id> {
        let mut tx = self.pool.begin().await?;
        let existing: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT thread_id FROM messages WHERE mailbox_id = ? AND uid = ?")
                .bind(b(m.mailbox_id))
                .bind(m.uid as i64)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(t) = existing {
            return Id::from_slice(&t);
        }
        let mut related: Vec<&str> = m.refs.iter().map(String::as_str).collect();
        related.extend(m.in_reply_to.as_deref());
        related.extend(m.message_id.as_deref());
        related.retain(|s| !s.is_empty());

        let mut threads: Vec<Vec<u8>> = Vec::new();
        if !related.is_empty() {
            let mut q: QueryBuilder<Sqlite> =
                QueryBuilder::new("SELECT DISTINCT thread_id FROM messages WHERE account_id = ");
            q.push_bind(b(m.account_id)).push(" AND (message_id IN (");
            let mut sep = q.separated(", ");
            for r in &related {
                sep.push_bind(*r);
            }
            q.push(")");
            if let Some(mid) = m.message_id.as_deref().filter(|s| !s.is_empty()) {
                // Respuestas que llegaron antes que este mensaje.
                q.push(" OR in_reply_to = ").push_bind(mid);
            }
            q.push(") ORDER BY thread_id");
            threads = q.build_query_scalar().fetch_all(&mut *tx).await?;
        }

        let thread = match threads.split_first() {
            Some((keep, others)) => {
                for o in others {
                    // Las etiquetas del hilo que desaparece pasan al que queda.
                    sqlx::query("UPDATE OR IGNORE thread_labels SET thread_id = ? WHERE thread_id = ?")
                        .bind(keep)
                        .bind(o)
                        .execute(&mut *tx)
                        .await?;
                    sqlx::query("UPDATE messages SET thread_id = ? WHERE thread_id = ?")
                        .bind(keep)
                        .bind(o)
                        .execute(&mut *tx)
                        .await?;
                    // También las pilas, lo pospuesto y la nota.
                    sqlx::query(
                        "UPDATE threads SET last_date = MAX(threads.last_date, o.last_date), \
                         reply_later = COALESCE(threads.reply_later, o.reply_later), aside = COALESCE(threads.aside, o.aside), \
                         snooze_until = COALESCE(threads.snooze_until, o.snooze_until), \
                         note = CASE WHEN threads.note = '' THEN o.note WHEN o.note = '' THEN threads.note \
                                ELSE threads.note || char(10) || char(10) || o.note END \
                         FROM (SELECT * FROM threads WHERE id = ?) AS o WHERE threads.id = ?",
                    )
                    .bind(o)
                    .bind(keep)
                        .execute(&mut *tx)
                        .await?;
                    sqlx::query("DELETE FROM threads WHERE id = ?")
                        .bind(o)
                        .execute(&mut *tx)
                        .await?;
                }
                sqlx::query("UPDATE threads SET last_date = MAX(last_date, ?) WHERE id = ?")
                    .bind(m.date)
                    .bind(keep)
                    .execute(&mut *tx)
                    .await?;
                keep.clone()
            }
            None => {
                let t = b(Id::new());
                sqlx::query("INSERT INTO threads (id, account_id, subject, last_date) VALUES (?, ?, ?, ?)")
                    .bind(&t)
                    .bind(b(m.account_id))
                    .bind(base_subject(&m.subject))
                    .bind(m.date)
                    .execute(&mut *tx)
                    .await?;
                t
            }
        };

        sqlx::query(
            "INSERT INTO messages (id, account_id, mailbox_id, uid, thread_id, message_id, in_reply_to, refs, subject, \
             from_name, from_addr, to_json, cc_json, date, snippet, size, seen, flagged, answered, has_attachments, kind) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(b(m.id))
        .bind(b(m.account_id))
        .bind(b(m.mailbox_id))
        .bind(m.uid as i64)
        .bind(&thread)
        .bind(&m.message_id)
        .bind(&m.in_reply_to)
        .bind(m.refs.join(" "))
        .bind(&m.subject)
        .bind(m.from.as_ref().and_then(|a| a.name.clone()))
        .bind(m.from.as_ref().map(|a| a.email.clone()))
        .bind(serde_json::to_string(&m.to).unwrap_or_else(|_| "[]".into()))
        .bind(serde_json::to_string(&m.cc).unwrap_or_else(|_| "[]".into()))
        .bind(m.date)
        .bind(&m.snippet)
        .bind(m.size as i64)
        .bind(m.flags.seen)
        .bind(m.flags.flagged)
        .bind(m.flags.answered)
        .bind(m.attachments.iter().any(|a| !a.inline))
        .bind(&m.kind)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO message_bodies (message_id, text, html, remote_images, attachments, trackers) \
             VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (message_id) DO NOTHING",
        )
        .bind(b(m.id))
        .bind(&m.text)
        .bind(&m.html)
        .bind(m.remote_images as i64)
        .bind(serde_json::to_string(&m.attachments).unwrap_or_else(|_| "[]".into()))
        .bind(serde_json::to_string(&m.trackers).unwrap_or_else(|_| "[]".into()))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Id::from_slice(&thread)
    }

    pub async fn set_flags(&self, message: Id, f: Flags) -> Result<()> {
        sqlx::query("UPDATE messages SET seen = ?, flagged = ?, answered = ? WHERE id = ?")
            .bind(f.seen)
            .bind(f.flagged)
            .bind(f.answered)
            .bind(b(message))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_messages(&self, ids: &[Id]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        delete_messages_tx(&mut tx, ids).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Página de hilos, del más reciente al más antiguo.
    pub async fn threads(&self, q: &ThreadQuery) -> Result<Vec<ThreadSummary>> {
        let mut s: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT t.id, t.account_id, t.subject, t.last_date, \
             (SELECT COUNT(DISTINCT COALESCE(message_id, hex(id))) FROM messages WHERE thread_id = t.id) AS n, \
             (SELECT COUNT(*) FROM messages WHERE thread_id = t.id AND seen = 0) AS unread, \
             (SELECT MAX(flagged) FROM messages WHERE thread_id = t.id) AS flagged, \
             (SELECT MAX(has_attachments) FROM messages WHERE thread_id = t.id) AS att, \
             (SELECT snippet FROM messages WHERE thread_id = t.id ORDER BY date DESC LIMIT 1) AS snippet, \
             (SELECT group_concat(who, char(31)) FROM (SELECT COALESCE(from_name, from_addr, '') AS who, MAX(date) AS d \
                FROM messages WHERE thread_id = t.id GROUP BY who ORDER BY d)) AS people, \
             (SELECT group_concat(hex(label_id)) FROM thread_labels WHERE thread_id = t.id) AS labels, \
             t.reply_later, t.aside, t.snooze_until, t.note <> '' AS has_note, \
             (SELECT MAX(json_array_length(bd.trackers)) FROM messages mt JOIN message_bodies bd ON bd.message_id = mt.id \
                WHERE mt.thread_id = t.id) > 0 AS tracked \
             FROM threads t WHERE EXISTS (SELECT 1 FROM messages m JOIN mailboxes mb ON mb.id = m.mailbox_id \
             WHERE m.thread_id = t.id",
        );
        if let Some(mb) = q.mailbox {
            s.push(" AND mb.id = ").push_bind(b(mb));
        }
        if let Some(role) = &q.role {
            s.push(" AND mb.role = ").push_bind(role.clone());
        }
        if let Some(search) = &q.search {
            search.push_conditions(&mut s);
        }
        s.push(")");
        if let Some(l) = q.label {
            s.push(" AND EXISTS (SELECT 1 FROM thread_labels tl WHERE tl.thread_id = t.id AND tl.label_id = ")
                .push_bind(b(l))
                .push(")");
        }
        if let Some(a) = q.account {
            s.push(" AND t.account_id = ").push_bind(b(a));
        }
        if let Some(v) = q.view {
            v.push_conditions(&mut s, q.now);
        }
        if let Some((date, id)) = q.before {
            s.push(" AND (t.last_date, t.id) < (")
                .push_bind(date)
                .push(", ")
                .push_bind(b(id))
                .push(")");
        }
        s.push(" ORDER BY t.last_date DESC, t.id DESC LIMIT ")
            .push_bind(q.limit.clamp(1, 200) as i64);
        let rows = s.build().fetch_all(&self.pool).await?;
        rows.iter()
            .map(|r| {
                let people: Option<String> = r.try_get("people")?;
                Ok(ThreadSummary {
                    id: id_of(r, "id")?,
                    account_id: id_of(r, "account_id")?,
                    subject: r.try_get("subject")?,
                    last_date: r.try_get("last_date")?,
                    participants: people
                        .unwrap_or_default()
                        .split('\u{1f}')
                        .filter(|p| !p.is_empty())
                        .map(String::from)
                        .collect(),
                    snippet: r.try_get::<Option<String>, _>("snippet")?.unwrap_or_default(),
                    messages: r.try_get("n")?,
                    unread: r.try_get("unread")?,
                    flagged: r.try_get::<Option<bool>, _>("flagged")?.unwrap_or(false),
                    has_attachments: r.try_get::<Option<bool>, _>("att")?.unwrap_or(false),
                    labels: r
                        .try_get::<Option<String>, _>("labels")?
                        .unwrap_or_default()
                        .split(',')
                        .filter_map(crate::organize::id_from_hex)
                        .collect(),
                    flow: crate::flow::ThreadFlow {
                        reply_later: r.try_get::<Option<i64>, _>("reply_later")?.is_some(),
                        aside: r.try_get::<Option<i64>, _>("aside")?.is_some(),
                        snooze_until: r.try_get("snooze_until")?,
                        has_note: r.try_get("has_note")?,
                        tracked: r.try_get::<Option<bool>, _>("tracked")?.unwrap_or(false),
                    },
                })
            })
            .collect()
    }

    /// Hilo completo, con los mensajes repetidos en varias carpetas una sola vez.
    pub async fn thread(&self, id: Id) -> Result<Option<Thread>> {
        let Some(t) = sqlx::query(
            "SELECT id, account_id, subject, reply_later, aside, snooze_until, note FROM threads WHERE id = ?",
        )
        .bind(b(id))
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let rows = sqlx::query(
            "SELECT m.*, mb.role, bd.text, bd.html, bd.remote_images, bd.attachments, bd.trackers FROM messages m \
             JOIN mailboxes mb ON mb.id = m.mailbox_id LEFT JOIN message_bodies bd ON bd.message_id = m.id \
             WHERE m.thread_id = ? ORDER BY m.date, (mb.role = 'inbox') DESC, m.id",
        )
        .bind(b(id))
        .fetch_all(&self.pool)
        .await?;
        let mut seen_ids = HashSet::new();
        let mut messages = Vec::new();
        for r in &rows {
            let message_id: Option<String> = r.try_get("message_id")?;
            if let Some(mid) = &message_id {
                if !seen_ids.insert(mid.clone()) {
                    continue;
                }
            }
            let from_addr: Option<String> = r.try_get("from_addr")?;
            messages.push(StoredMessage {
                id: id_of(r, "id")?,
                mailbox_id: id_of(r, "mailbox_id")?,
                uid: r.try_get::<i64, _>("uid")?.max(0) as u32,
                message_id,
                subject: r.try_get("subject")?,
                from: from_addr.map(|email| Addr {
                    name: r.try_get("from_name").ok().flatten(),
                    email,
                }),
                to: serde_json::from_str(r.try_get("to_json")?).unwrap_or_default(),
                cc: serde_json::from_str(r.try_get("cc_json")?).unwrap_or_default(),
                date: r.try_get("date")?,
                flags: Flags {
                    seen: r.try_get("seen")?,
                    flagged: r.try_get("flagged")?,
                    answered: r.try_get("answered")?,
                },
                text: r.try_get("text")?,
                html: r.try_get("html")?,
                remote_images: r.try_get::<Option<i64>, _>("remote_images")?.unwrap_or(0) as u32,
                attachments: r
                    .try_get::<Option<&str>, _>("attachments")?
                    .and_then(|j| serde_json::from_str(j).ok())
                    .unwrap_or_default(),
                trackers: r
                    .try_get::<Option<&str>, _>("trackers")?
                    .and_then(|j| serde_json::from_str(j).ok())
                    .unwrap_or_default(),
            });
        }
        let note: String = t.try_get("note")?;
        Ok(Some(Thread {
            id: id_of(&t, "id")?,
            account_id: id_of(&t, "account_id")?,
            subject: t.try_get("subject")?,
            flow: crate::flow::ThreadFlow {
                reply_later: t.try_get::<Option<i64>, _>("reply_later")?.is_some(),
                aside: t.try_get::<Option<i64>, _>("aside")?.is_some(),
                snooze_until: t.try_get("snooze_until")?,
                has_note: !note.is_empty(),
                tracked: messages.iter().any(|m| !m.trackers.is_empty()),
            },
            note,
            messages,
        }))
    }

    /// Marca un hilo como leído o no leído (`seen`) o destacado (`flagged`).
    /// Devuelve los mensajes que han cambiado, para aplicarlo también en IMAP.
    /// Al destacar solo se marca el último mensaje; al quitarlo, todos.
    pub async fn message_brief(&self, id: Id) -> Result<Option<MessageBrief>> {
        let row = sqlx::query(
            "SELECT m.thread_id, m.account_id, mb.name, m.subject, m.from_name, m.from_addr, m.to_json, \
             m.date, m.snippet, m.kind FROM messages m JOIN mailboxes mb ON mb.id = m.mailbox_id WHERE m.id = ?",
        )
        .bind(id.as_bytes().as_slice())
        .fetch_optional(&self.pool)
        .await?;
        let Some(r) = row else { return Ok(None) };
        Ok(Some(MessageBrief {
            id,
            thread: Id::from_slice(r.try_get::<&[u8], _>("thread_id")?)?,
            account: Id::from_slice(r.try_get::<&[u8], _>("account_id")?)?,
            mailbox: r.try_get("name")?,
            subject: r.try_get("subject")?,
            from: r.try_get::<Option<String>, _>("from_addr")?.map(|email| Addr {
                name: r.try_get("from_name").ok().flatten(),
                email,
            }),
            to: serde_json::from_str(r.try_get("to_json")?).unwrap_or_default(),
            date: r.try_get("date")?,
            snippet: r.try_get("snippet")?,
            kind: r.try_get("kind")?,
        }))
    }

    pub async fn update_thread(&self, id: Id, seen: Option<bool>, flagged: Option<bool>) -> Result<ThreadChanges> {
        let mut tx = self.pool.begin().await?;
        let mut changes = ThreadChanges::default();
        if let Some(v) = seen {
            changes.seen = targets(
                &mut tx,
                "UPDATE messages SET seen = ? WHERE thread_id = ? AND seen <> ? \
                 RETURNING account_id, mailbox_id, uid",
                v,
                id,
            )
            .await?;
        }
        if let Some(v) = flagged {
            let sql = if v {
                "UPDATE messages SET flagged = ? WHERE id = (SELECT id FROM messages WHERE thread_id = ? \
                 ORDER BY date DESC LIMIT 1) AND flagged <> ? RETURNING account_id, mailbox_id, uid"
            } else {
                "UPDATE messages SET flagged = ? WHERE thread_id = ? AND flagged <> ? RETURNING account_id, mailbox_id, uid"
            };
            changes.flagged = targets(&mut tx, sql, v, id).await?;
        }
        tx.commit().await?;
        Ok(changes)
    }

    /// Adjuntos guardados de un mensaje.
    pub async fn attachments(&self, message: Id) -> Result<Option<(Id, Vec<Attachment>)>> {
        let row = sqlx::query(
            "SELECT m.account_id, bd.attachments FROM messages m JOIN message_bodies bd ON bd.message_id = m.id \
             WHERE m.id = ?",
        )
        .bind(b(message))
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| {
            Ok((
                id_of(&r, "account_id")?,
                serde_json::from_str(r.try_get("attachments")?).unwrap_or_default(),
            ))
        })
        .transpose()
    }
}

#[derive(Debug, Default)]
pub struct ThreadChanges {
    pub seen: Vec<ImapTarget>,
    pub flagged: Vec<ImapTarget>,
}

async fn targets(tx: &mut SqliteConnection, sql: &str, v: bool, thread: Id) -> Result<Vec<ImapTarget>> {
    let rows = sqlx::query(sql)
        .bind(v)
        .bind(b(thread))
        .bind(v)
        .fetch_all(&mut *tx)
        .await?;
    let mut out = Vec::new();
    for r in &rows {
        // Los mensajes recién movidos aún no tienen UID en el servidor.
        if r.try_get::<i64, _>("uid")? <= 0 {
            continue;
        }
        let mailbox: String = sqlx::query_scalar("SELECT name FROM mailboxes WHERE id = ?")
            .bind(r.try_get::<&[u8], _>("mailbox_id")?)
            .fetch_one(&mut *tx)
            .await?;
        out.push(ImapTarget {
            account_id: id_of(r, "account_id")?,
            mailbox,
            uid: r.try_get::<i64, _>("uid")? as u32,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(account: Id, mailbox: Id, uid: u32, mid: &str, reply_to: Option<&str>, date: i64) -> NewMessage {
        NewMessage {
            id: Id::new(),
            account_id: account,
            mailbox_id: mailbox,
            uid,
            message_id: Some(mid.into()),
            in_reply_to: reply_to.map(Into::into),
            refs: reply_to.map(|r| vec![r.to_string()]).unwrap_or_default(),
            subject: if reply_to.is_some() { "Re: Cena" } else { "Cena" }.into(),
            from: Some(Addr {
                name: Some(format!("Persona {uid}")),
                email: format!("p{uid}@ejemplo.es"),
            }),
            to: vec![],
            cc: vec![],
            date,
            snippet: format!("texto {uid}"),
            size: 100,
            flags: Flags::default(),
            text: Some("hola".into()),
            html: None,
            remote_images: 0,
            attachments: vec![],
            kind: "principal".into(),
            trackers: vec![],
        }
    }

    async fn db_with_account() -> (tempfile::TempDir, UserDb, Id) {
        let dir = tempfile::tempdir().unwrap();
        let db = UserDb::open(dir.path(), Id::new()).await.unwrap();
        let key = crate::crypto::MasterKey::generate().derive("x");
        let acc = db
            .add_account(
                crate::user::NewAccount {
                    name: "P",
                    email: "yo@ejemplo.es",
                    imap_host: "h",
                    imap_port: 993,
                    smtp_host: "h",
                    smtp_port: 465,
                    login: "yo",
                    password: "x",
                    auth: "password",
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
    async fn threads_group_replies_and_merge_out_of_order() {
        let (_d, db, acc) = db_with_account().await;
        let inbox = db.upsert_mailbox(acc, "INBOX", Some("/"), Some("inbox")).await.unwrap();
        let sent = db.upsert_mailbox(acc, "Sent", Some("/"), Some("sent")).await.unwrap();

        // La respuesta llega antes que el original, y luego otra respuesta desde Enviados.
        let t1 = db
            .insert_message(&msg(acc, inbox.id, 2, "b@x", Some("a@x"), 200))
            .await
            .unwrap();
        let t2 = db
            .insert_message(&msg(acc, inbox.id, 1, "a@x", None, 100))
            .await
            .unwrap();
        let t3 = db
            .insert_message(&msg(acc, sent.id, 1, "c@x", Some("b@x"), 300))
            .await
            .unwrap();
        assert_eq!(t1, t2);
        assert_eq!(t2, t3);
        db.insert_message(&msg(acc, inbox.id, 3, "otro@x", None, 50))
            .await
            .unwrap();

        let page = db
            .threads(&ThreadQuery {
                role: Some("inbox".into()),
                limit: 50,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].id, t1);
        assert_eq!(page[0].subject, "Cena");
        assert_eq!(page[0].messages, 3);
        assert_eq!(page[0].last_date, 300);
        assert_eq!(page[0].snippet, "texto 1");

        let next = db
            .threads(&ThreadQuery {
                role: Some("inbox".into()),
                before: Some((page[0].last_date, page[0].id)),
                limit: 50,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(next.len(), 1);

        let thread = db.thread(t1).await.unwrap().unwrap();
        assert_eq!(thread.messages.len(), 3);
        assert_eq!(thread.messages[0].message_id.as_deref(), Some("a@x"));

        let changes = db.update_thread(t1, Some(true), Some(true)).await.unwrap();
        assert_eq!(changes.seen.len(), 3);
        assert_eq!(changes.flagged.len(), 1);
        assert_eq!(changes.flagged[0].mailbox, "Sent");
        let again = db.update_thread(t1, Some(true), None).await.unwrap();
        assert!(again.seen.is_empty());

        // Al borrar todo el hilo, desaparece; al borrar parte, se recalcula la fecha.
        let ids: Vec<Id> = thread.messages.iter().map(|m| m.id).collect();
        db.delete_messages(&ids[2..]).await.unwrap();
        let t = db
            .threads(&ThreadQuery {
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(t.iter().find(|x| x.id == t1).unwrap().last_date, 200);
        db.delete_messages(&ids[..2]).await.unwrap();
        assert!(db.thread(t1).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_message_that_links_two_threads_merges_them() {
        let (_d, db, acc) = db_with_account().await;
        let inbox = db.upsert_mailbox(acc, "INBOX", None, Some("inbox")).await.unwrap();
        let tx = db
            .insert_message(&msg(acc, inbox.id, 1, "x@x", None, 100))
            .await
            .unwrap();
        let ty = db
            .insert_message(&msg(acc, inbox.id, 2, "y@x", None, 300))
            .await
            .unwrap();
        assert_ne!(tx, ty);
        // Responde a uno y cita al otro en References: los dos hilos pasan a ser uno.
        let mut both = msg(acc, inbox.id, 3, "z@x", Some("x@x"), 200);
        both.refs = vec!["y@x".into(), "x@x".into()];
        let t = db.insert_message(&both).await.unwrap();
        let page = db
            .threads(&ThreadQuery {
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.len(), 1, "{page:?}");
        assert_eq!((page[0].id, page[0].messages, page[0].last_date), (t, 3, 300));
    }

    #[tokio::test]
    async fn duplicated_message_in_two_folders_shows_once() {
        let (_d, db, acc) = db_with_account().await;
        let inbox = db.upsert_mailbox(acc, "INBOX", None, Some("inbox")).await.unwrap();
        let other = db.upsert_mailbox(acc, "Proyectos", None, None).await.unwrap();
        let t = db
            .insert_message(&msg(acc, other.id, 9, "a@x", None, 100))
            .await
            .unwrap();
        db.insert_message(&msg(acc, inbox.id, 1, "a@x", None, 100))
            .await
            .unwrap();
        let thread = db.thread(t).await.unwrap().unwrap();
        assert_eq!(thread.messages.len(), 1);
        assert_eq!(thread.messages[0].mailbox_id, inbox.id);
        let counts = db.mailboxes(Some(acc)).await.unwrap();
        assert_eq!(counts.iter().map(|c| c.unread).sum::<i64>(), 2);

        let deleted = db.retain_mailboxes(acc, &["INBOX".into()]).await.unwrap();
        assert_eq!(deleted.len(), 1);
        assert_eq!(db.mailboxes(Some(acc)).await.unwrap().len(), 1);
    }

    #[test]
    fn subjects_and_mailbox_names() {
        assert_eq!(base_subject("Re: RE: Fwd: Hola"), "Hola");
        assert_eq!(base_subject("RV: Presupuesto"), "Presupuesto");
        assert_eq!(base_subject("Reunión"), "Reunión");
        assert_eq!(decode_mailbox_name("INBOX"), "INBOX");
        assert_eq!(decode_mailbox_name("Elementos enviados"), "Elementos enviados");
        assert_eq!(decode_mailbox_name("&AMk-l&AOk-ments"), "Éléments");
        assert_eq!(decode_mailbox_name("Pap&AOk-lera"), "Papélera");
        assert_eq!(decode_mailbox_name("A&-B"), "A&B");
        assert_eq!(decode_mailbox_name("&ZeVnLIqe-"), "日本語");
    }
}

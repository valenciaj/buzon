//! Organizar el correo: mover hilos, buscar, etiquetas, filtros automáticos y contactos.

use std::collections::BTreeMap;

use sqlx::{QueryBuilder, Row, Sqlite};

use crate::mail::{b, delete_messages_tx, id_of, Addr};
use crate::{map_unique, now, Id, Result, UserDb};

/// Tras este tiempo, un mensaje movido que el servidor no ha confirmado deja de mostrarse:
/// la siguiente sincronización traerá lo que haya de verdad.
const MOVE_GRACE_SECS: i64 = 15 * 60;

pub(crate) fn id_from_hex(h: &str) -> Option<Id> {
    if h.len() != 32 {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..16)
        .map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok())
        .collect();
    Id::from_slice(&bytes?).ok()
}

/// UID provisional (negativo y distinto para cada mensaje) hasta que el servidor confirme.
fn pending_uid(id: Id) -> i64 {
    let r = id.as_bytes();
    -1 - (u32::from_be_bytes([r[12], r[13], r[14], r[15]]) & 0x7fff_ffff) as i64
}

// ---------- Mover ----------

/// Dónde están o adónde van los mensajes: un tipo de carpeta (`inbox`, `archive`…)
/// de la cuenta de cada hilo, o una carpeta concreta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    Role(String),
    Mailbox(Id),
}

/// Mensajes que hay que mover también en el servidor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveOp {
    pub account_id: Id,
    pub from: String,
    pub to: String,
    pub uids: Vec<u32>,
}

/// Mensajes que hay que borrar definitivamente en el servidor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpungeOp {
    pub account_id: Id,
    pub mailbox: String,
    pub uids: Vec<u32>,
}

struct Located {
    id: Id,
    account: Id,
    uid: i64,
    mailbox: Id,
    name: String,
    role: Option<String>,
}

impl UserDb {
    async fn located(&self, threads: &[Id], messages: &[Id]) -> Result<Vec<Located>> {
        let mut out = Vec::new();
        for (col, ids) in [("m.thread_id", threads), ("m.id", messages)] {
            for chunk in ids.chunks(400) {
                let mut q: QueryBuilder<Sqlite> = QueryBuilder::new(
                    "SELECT m.id, m.account_id, m.uid, m.mailbox_id, mb.name, mb.role FROM messages m \
                     JOIN mailboxes mb ON mb.id = m.mailbox_id WHERE ",
                );
                q.push(col).push(" IN (");
                let mut sep = q.separated(", ");
                for id in chunk {
                    sep.push_bind(b(*id));
                }
                q.push(")");
                for r in q.build().fetch_all(&self.pool).await? {
                    out.push(Located {
                        id: id_of(&r, "id")?,
                        account: id_of(&r, "account_id")?,
                        uid: r.try_get("uid")?,
                        mailbox: id_of(&r, "mailbox_id")?,
                        name: r.try_get("name")?,
                        role: r.try_get("role")?,
                    });
                }
            }
        }
        Ok(out)
    }

    async fn role_mailbox(&self, account: Id, role: &str) -> Result<Option<(Id, String)>> {
        let row = sqlx::query("SELECT id, name FROM mailboxes WHERE account_id = ? AND role = ? ORDER BY name LIMIT 1")
            .bind(b(account))
            .bind(role)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| Ok((id_of(&r, "id")?, r.try_get("name")?))).transpose()
    }

    /// Mueve hilos (o mensajes sueltos) a otra carpeta. Solo los mensajes que están en
    /// `from`; sin `from`, todos menos los de Enviados y Borradores. Se aplica ya en
    /// local; devuelve lo que hay que hacer en el servidor.
    pub async fn move_messages(
        &self,
        threads: &[Id],
        messages: &[Id],
        from: Option<&Place>,
        to: &Place,
    ) -> Result<Vec<MoveOp>> {
        let found = self.located(threads, messages).await?;
        let mut dests: BTreeMap<Id, (Id, String)> = BTreeMap::new();
        let mut ops: BTreeMap<(Id, String, String), Vec<u32>> = BTreeMap::new();
        let mut tx = self.pool.begin().await?;
        let t = now();
        for m in found {
            let wanted = match from {
                Some(Place::Role(r)) => m.role.as_deref() == Some(r.as_str()),
                Some(Place::Mailbox(id)) => m.mailbox == *id,
                None => !matches!(m.role.as_deref(), Some("sent" | "drafts")),
            };
            if !wanted {
                continue;
            }
            let (dest, dest_name) = match dests.get(&m.account) {
                Some(d) => d.clone(),
                None => {
                    let d = match to {
                        Place::Role(role) => self
                            .role_mailbox(m.account, role)
                            .await?
                            .ok_or_else(|| crate::Error::MissingFolder(role.clone()))?,
                        Place::Mailbox(id) => {
                            let Some(mb) = self.mailbox(*id).await? else {
                                return Err(crate::Error::BadId);
                            };
                            if mb.account_id != m.account {
                                continue;
                            }
                            (mb.id, mb.name)
                        }
                    };
                    dests.insert(m.account, d.clone());
                    d
                }
            };
            // Los que aún esperan confirmación de un movimiento anterior se quedan quietos.
            if dest == m.mailbox || m.uid <= 0 {
                continue;
            }
            sqlx::query("UPDATE messages SET mailbox_id = ?, uid = ?, moved_at = ? WHERE id = ?")
                .bind(b(dest))
                .bind(pending_uid(m.id))
                .bind(t)
                .bind(b(m.id))
                .execute(&mut *tx)
                .await?;
            ops.entry((m.account, m.name, dest_name))
                .or_default()
                .push(m.uid as u32);
        }
        tx.commit().await?;
        Ok(ops
            .into_iter()
            .map(|((account_id, from, to), uids)| MoveOp {
                account_id,
                from,
                to,
                uids,
            })
            .collect())
    }

    /// Borra para siempre los mensajes de los hilos que están en la papelera o en spam
    /// (o en `from`, si es una de esas). Devuelve los ids borrados y lo que hay que
    /// borrar en el servidor.
    pub async fn delete_forever(&self, threads: &[Id], from: Option<&Place>) -> Result<(Vec<Id>, Vec<ExpungeOp>)> {
        let found = self.located(threads, &[]).await?;
        let mut ids = Vec::new();
        let mut ops: BTreeMap<(Id, String), Vec<u32>> = BTreeMap::new();
        for m in found {
            let disposable = matches!(m.role.as_deref(), Some("trash" | "junk"));
            let wanted = match from {
                Some(Place::Role(r)) => m.role.as_deref() == Some(r.as_str()),
                Some(Place::Mailbox(id)) => m.mailbox == *id,
                None => true,
            };
            if !disposable || !wanted {
                continue;
            }
            ids.push(m.id);
            if m.uid > 0 {
                ops.entry((m.account, m.name)).or_default().push(m.uid as u32);
            }
        }
        let mut tx = self.pool.begin().await?;
        delete_messages_tx(&mut tx, &ids).await?;
        tx.commit().await?;
        let ops = ops
            .into_iter()
            .map(|((account_id, mailbox), uids)| ExpungeOp {
                account_id,
                mailbox,
                uids,
            })
            .collect();
        Ok((ids, ops))
    }

    /// Solo para pruebas: da por confirmado un mensaje movido, con este UID.
    #[doc(hidden)]
    pub async fn confirm_move_for_tests(&self, message: Id, uid: u32) -> Result<()> {
        sqlx::query("UPDATE messages SET uid = ?, moved_at = NULL WHERE id = ?")
            .bind(uid as i64)
            .bind(b(message))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Quita los mensajes movidos cuya copia real ya ha llegado a la carpeta (o que
    /// llevan demasiado esperando). Devuelve sus ids.
    pub async fn drop_confirmed_moves(&self, mailbox: Id) -> Result<Vec<Id>> {
        let rows: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT p.id FROM messages p WHERE p.mailbox_id = ?1 AND p.uid < 0 AND (p.moved_at < ?2 \
             OR (p.message_id IS NOT NULL AND EXISTS (SELECT 1 FROM messages r WHERE r.mailbox_id = ?1 \
             AND r.uid > 0 AND r.message_id = p.message_id)))",
        )
        .bind(b(mailbox))
        .bind(now() - MOVE_GRACE_SECS)
        .fetch_all(&self.pool)
        .await?;
        let ids = rows.iter().map(|v| Id::from_slice(v)).collect::<Result<Vec<_>>>()?;
        if !ids.is_empty() {
            self.delete_messages(&ids).await?;
        }
        Ok(ids)
    }
}

// ---------- Buscar ----------

/// Una búsqueda ya interpretada. Todo lo que tiene debe cumplirlo un mismo mensaje.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Search {
    /// Expresión FTS5 para el texto libre (asunto, personas y cuerpo).
    pub text: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub unread: bool,
    pub flagged: bool,
    pub attachments: bool,
    /// Desde esta fecha (incluida) y antes de esta otra, en segundos Unix.
    pub after: Option<i64>,
    pub before: Option<i64>,
    /// Nombre de etiqueta (`label:`), para resolverla fuera.
    pub label: Option<String>,
    /// Tipo de carpeta (`in:`).
    pub role: Option<String>,
}

/// Días desde 1970-01-01 de una fecha civil (algoritmo de Howard Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn parse_date(s: &str) -> Option<i64> {
    let mut it = s.split(['-', '/']).map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (it.next()??, it.next()??, it.next()??);
    if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) || y < 1970 {
        return None;
    }
    Some(days_from_civil(y, m, d) * 86_400)
}

fn role_word(w: &str) -> Option<&'static str> {
    Some(match w {
        "inbox" | "entrada" => "inbox",
        "sent" | "enviados" => "sent",
        "drafts" | "borradores" => "drafts",
        "archive" | "archivo" => "archive",
        "trash" | "papelera" => "trash",
        "junk" | "spam" => "junk",
        _ => return None,
    })
}

/// Palabras y `clave:valor` (el valor puede ir entre comillas).
fn tokens(q: &str) -> Vec<(Option<String>, String, bool)> {
    let mut out = Vec::new();
    let chars: Vec<char> = q.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let mut key = None;
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != ':' && chars[i] != '"' {
            i += 1;
        }
        if i < chars.len() && chars[i] == ':' && i > start {
            key = Some(chars[start..i].iter().collect::<String>().to_lowercase());
            i += 1;
        } else {
            i = start;
        }
        let quoted = i < chars.len() && chars[i] == '"';
        let value: String = if quoted {
            i += 1;
            let s = i;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            let v = chars[s..i].iter().collect();
            i += 1;
            v
        } else {
            let s = i;
            while i < chars.len() && !chars[i].is_whitespace() {
                i += 1;
            }
            chars[s..i].iter().collect()
        };
        if !value.is_empty() {
            out.push((key, value, quoted));
        }
    }
    out
}

fn fts_term(value: &str, quoted: bool) -> Option<String> {
    let clean: String = value
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '@' || c == '.' || c == '-' || c == '_' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let words: Vec<&str> = clean.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let phrase = words.join(" ").replace('"', "");
    // Sin comillas se busca por prefijo: «presu» encuentra «presupuesto».
    Some(if quoted {
        format!("\"{phrase}\"")
    } else {
        format!("\"{phrase}\"*")
    })
}

/// Interpreta lo que se escribe en el buscador. Entiende, en español o en inglés:
/// `de:` `para:` `asunto:` `etiqueta:` `en:` `tiene:adjunto` `es:noleido` `es:destacado`
/// `despues:AAAA-MM-DD` `antes:AAAA-MM-DD`. Lo demás se busca en todo el texto.
pub fn parse_search(q: &str) -> Search {
    let mut s = Search::default();
    let mut text: Vec<String> = Vec::new();
    for (key, value, quoted) in tokens(q) {
        let v = value.to_lowercase();
        match key.as_deref() {
            Some("from" | "de") => s.from = Some(value),
            Some("to" | "para") => s.to = Some(value),
            Some("subject" | "asunto") => {
                if let Some(t) = fts_term(&value, quoted) {
                    text.push(format!("subject : {t}"));
                }
            }
            Some("label" | "etiqueta") => s.label = Some(value),
            Some("in" | "en") if role_word(&v).is_some() => s.role = role_word(&v).map(String::from),
            Some("has" | "tiene") if matches!(v.as_str(), "attachment" | "adjunto" | "adjuntos") => {
                s.attachments = true
            }
            Some("is" | "es") if matches!(v.as_str(), "unread" | "noleido" | "no-leido" | "noleído" | "no-leído") => {
                s.unread = true
            }
            Some("is" | "es") if matches!(v.as_str(), "starred" | "flagged" | "destacado") => s.flagged = true,
            Some("after" | "despues" | "después" | "desde") if parse_date(&v).is_some() => s.after = parse_date(&v),
            Some("before" | "antes" | "hasta") if parse_date(&v).is_some() => s.before = parse_date(&v),
            // Una clave desconocida se busca como texto («10:30»).
            Some(k) => text.extend(fts_term(&format!("{k} {value}"), true)),
            None => text.extend(fts_term(&value, quoted)),
        }
    }
    if !text.is_empty() {
        s.text = Some(text.join(" AND "));
    }
    s
}

fn like(v: &str) -> String {
    format!("%{}%", v.replace(['%', '_'], ""))
}

impl Search {
    pub fn is_empty(&self) -> bool {
        *self == Search::default()
    }

    /// Añade las condiciones sobre el mensaje `m` (y su carpeta `mb`) a una consulta.
    pub(crate) fn push_conditions(&self, s: &mut QueryBuilder<'_, Sqlite>) {
        if let Some(t) = &self.text {
            s.push(" AND m.rowid IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ")
                .push_bind(t.clone())
                .push(")");
        }
        if let Some(f) = &self.from {
            s.push(" AND (m.from_addr LIKE ")
                .push_bind(like(f))
                .push(" OR m.from_name LIKE ")
                .push_bind(like(f))
                .push(")");
        }
        if let Some(t) = &self.to {
            s.push(" AND (m.to_json LIKE ")
                .push_bind(like(t))
                .push(" OR m.cc_json LIKE ")
                .push_bind(like(t))
                .push(")");
        }
        if self.unread {
            s.push(" AND m.seen = 0");
        }
        if self.flagged {
            s.push(" AND m.flagged = 1");
        }
        if self.attachments {
            s.push(" AND m.has_attachments = 1");
        }
        if let Some(a) = self.after {
            s.push(" AND m.date >= ").push_bind(a);
        }
        if let Some(bf) = self.before {
            s.push(" AND m.date < ").push_bind(bf);
        }
        if let Some(r) = &self.role {
            s.push(" AND mb.role = ").push_bind(r.clone());
        }
    }
}

// ---------- Etiquetas ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub id: Id,
    pub name: String,
    pub color: String,
    /// Hilos con la etiqueta y, de ellos, con algo sin leer.
    pub threads: i64,
    pub unread: i64,
}

impl UserDb {
    pub async fn labels(&self) -> Result<Vec<Label>> {
        let rows = sqlx::query(
            "SELECT l.id, l.name, l.color, \
             (SELECT COUNT(*) FROM thread_labels tl WHERE tl.label_id = l.id) AS threads, \
             (SELECT COUNT(*) FROM thread_labels tl WHERE tl.label_id = l.id AND EXISTS \
                (SELECT 1 FROM messages m WHERE m.thread_id = tl.thread_id AND m.seen = 0)) AS unread \
             FROM labels l ORDER BY l.name COLLATE NOCASE",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(Label {
                    id: id_of(r, "id")?,
                    name: r.try_get("name")?,
                    color: r.try_get("color")?,
                    threads: r.try_get("threads")?,
                    unread: r.try_get("unread")?,
                })
            })
            .collect()
    }

    pub async fn label_by_name(&self, name: &str) -> Result<Option<Id>> {
        let v: Option<Vec<u8>> = sqlx::query_scalar("SELECT id FROM labels WHERE name = ?")
            .bind(name.trim())
            .fetch_optional(&self.pool)
            .await?;
        v.map(|v| Id::from_slice(&v)).transpose()
    }

    /// Crea (sin `id`) o cambia una etiqueta. `Error::Duplicate` si el nombre ya existe.
    pub async fn save_label(&self, id: Option<Id>, name: &str, color: &str) -> Result<Id> {
        let id = match id {
            Some(id) => {
                let r = sqlx::query("UPDATE labels SET name = ?, color = ? WHERE id = ?")
                    .bind(name.trim())
                    .bind(color)
                    .bind(b(id))
                    .execute(&self.pool)
                    .await
                    .map_err(map_unique)?;
                if r.rows_affected() == 0 {
                    return Err(crate::Error::BadId);
                }
                id
            }
            None => {
                let id = Id::new();
                sqlx::query("INSERT INTO labels (id, name, color, created_at) VALUES (?, ?, ?, ?)")
                    .bind(b(id))
                    .bind(name.trim())
                    .bind(color)
                    .bind(now())
                    .execute(&self.pool)
                    .await
                    .map_err(map_unique)?;
                id
            }
        };
        Ok(id)
    }

    pub async fn delete_label(&self, id: Id) -> Result<bool> {
        let r = sqlx::query("DELETE FROM labels WHERE id = ?")
            .bind(b(id))
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Pone o quita etiquetas a varios hilos.
    pub async fn label_threads(&self, threads: &[Id], add: &[Id], remove: &[Id]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for t in threads {
            for l in add {
                sqlx::query(
                    "INSERT OR IGNORE INTO thread_labels (thread_id, label_id) \
                     SELECT ?1, ?2 WHERE EXISTS (SELECT 1 FROM threads WHERE id = ?1) \
                     AND EXISTS (SELECT 1 FROM labels WHERE id = ?2)",
                )
                .bind(b(*t))
                .bind(b(*l))
                .execute(&mut *tx)
                .await?;
            }
            for l in remove {
                sqlx::query("DELETE FROM thread_labels WHERE thread_id = ? AND label_id = ?")
                    .bind(b(*t))
                    .bind(b(*l))
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn thread_labels(&self, thread: Id) -> Result<Vec<Id>> {
        let rows: Vec<Vec<u8>> = sqlx::query_scalar("SELECT label_id FROM thread_labels WHERE thread_id = ?")
            .bind(b(thread))
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|v| Id::from_slice(v)).collect()
    }
}

// ---------- Filtros automáticos ----------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rule {
    pub id: Option<Id>,
    pub name: String,
    pub enabled: bool,
    /// Solo el correo de esta cuenta; sin ella, de todas.
    pub account_id: Option<Id>,
    pub from_has: String,
    pub to_has: String,
    pub subject_has: String,
    pub body_has: String,
    pub label_id: Option<Id>,
    pub mark_read: bool,
    pub star: bool,
    /// `archive`, `trash`, `junk` o vacío.
    pub move_to: String,
}

/// Lo que hay que hacer con un mensaje nuevo, sumando todos los filtros que cumple.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleActions {
    pub labels: Vec<Id>,
    pub mark_read: bool,
    pub star: bool,
    pub move_to: Option<String>,
}

impl RuleActions {
    pub fn is_empty(&self) -> bool {
        *self == RuleActions::default()
    }
}

/// Lo que miran los filtros de un mensaje.
pub struct RuleInput<'a> {
    pub account_id: Id,
    pub from: Option<&'a Addr>,
    pub to: &'a [Addr],
    pub cc: &'a [Addr],
    pub subject: &'a str,
    pub text: &'a str,
}

fn contains(hay: &str, needle: &str) -> bool {
    needle.trim().is_empty() || hay.to_lowercase().contains(&needle.trim().to_lowercase())
}

fn people(list: &[&Addr]) -> String {
    list.iter()
        .map(|a| format!("{} <{}>", a.name.as_deref().unwrap_or(""), a.email))
        .collect::<Vec<_>>()
        .join(", ")
}

impl Rule {
    /// Un filtro sin ninguna condición no hace nada (evita tocar todo el correo por error).
    pub fn has_conditions(&self) -> bool {
        [&self.from_has, &self.to_has, &self.subject_has, &self.body_has]
            .iter()
            .any(|c| !c.trim().is_empty())
    }

    pub fn matches(&self, m: &RuleInput<'_>) -> bool {
        self.enabled
            && self.has_conditions()
            && self.account_id.is_none_or(|a| a == m.account_id)
            && contains(&people(&m.from.into_iter().collect::<Vec<_>>()), &self.from_has)
            && contains(&people(&m.to.iter().chain(m.cc).collect::<Vec<_>>()), &self.to_has)
            && contains(m.subject, &self.subject_has)
            && contains(m.text, &self.body_has)
    }
}

/// Suma lo que piden los filtros que se cumplen, en orden.
pub fn apply_rules(rules: &[Rule], m: &RuleInput<'_>) -> RuleActions {
    let mut out = RuleActions::default();
    for r in rules.iter().filter(|r| r.matches(m)) {
        out.labels.extend(r.label_id.filter(|l| !out.labels.contains(l)));
        out.mark_read |= r.mark_read;
        out.star |= r.star;
        if out.move_to.is_none() && !r.move_to.is_empty() {
            out.move_to = Some(r.move_to.clone());
        }
    }
    out
}

fn rule_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Rule> {
    let opt = |col: &str| -> Result<Option<Id>> { r.try_get::<Option<&[u8]>, _>(col)?.map(Id::from_slice).transpose() };
    Ok(Rule {
        id: Some(id_of(r, "id")?),
        name: r.try_get("name")?,
        enabled: r.try_get("enabled")?,
        account_id: opt("account_id")?,
        from_has: r.try_get("from_has")?,
        to_has: r.try_get("to_has")?,
        subject_has: r.try_get("subject_has")?,
        body_has: r.try_get("body_has")?,
        label_id: opt("label_id")?,
        mark_read: r.try_get("mark_read")?,
        star: r.try_get("star")?,
        move_to: r.try_get("move_to")?,
    })
}

impl UserDb {
    pub async fn rules(&self) -> Result<Vec<Rule>> {
        let rows = sqlx::query("SELECT * FROM rules ORDER BY position, created_at")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(rule_from_row).collect()
    }

    pub async fn rule(&self, id: Id) -> Result<Option<Rule>> {
        let row = sqlx::query("SELECT * FROM rules WHERE id = ?")
            .bind(b(id))
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(rule_from_row).transpose()
    }

    /// Crea (sin `id`, al final de la lista) o cambia un filtro.
    pub async fn save_rule(&self, r: &Rule) -> Result<Id> {
        let id = r.id.unwrap_or_default();
        sqlx::query(
            "INSERT INTO rules (id, name, position, enabled, account_id, from_has, to_has, subject_has, body_has, \
             label_id, mark_read, star, move_to, created_at) \
             VALUES (?, ?, (SELECT COALESCE(MAX(position), 0) + 1 FROM rules), ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, enabled = excluded.enabled, \
             account_id = excluded.account_id, from_has = excluded.from_has, to_has = excluded.to_has, \
             subject_has = excluded.subject_has, body_has = excluded.body_has, label_id = excluded.label_id, \
             mark_read = excluded.mark_read, star = excluded.star, move_to = excluded.move_to",
        )
        .bind(b(id))
        .bind(r.name.trim())
        .bind(r.enabled)
        .bind(r.account_id.map(b))
        .bind(r.from_has.trim())
        .bind(r.to_has.trim())
        .bind(r.subject_has.trim())
        .bind(r.body_has.trim())
        .bind(r.label_id.map(b))
        .bind(r.mark_read)
        .bind(r.star)
        .bind(&r.move_to)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn delete_rule(&self, id: Id) -> Result<bool> {
        let r = sqlx::query("DELETE FROM rules WHERE id = ?")
            .bind(b(id))
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }
}

// ---------- Contactos ----------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookEntry {
    pub id: Option<Id>,
    pub name: String,
    pub email: String,
    pub phone: String,
    pub notes: String,
    /// `manual`, `sent` o `vcard`.
    pub source: String,
}

fn entry_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<BookEntry> {
    Ok(BookEntry {
        id: Some(id_of(r, "id")?),
        name: r.try_get("name")?,
        email: r.try_get("email")?,
        phone: r.try_get("phone")?,
        notes: r.try_get("notes")?,
        source: r.try_get("source")?,
    })
}

/// Deshace el escapado de vCard (`\n`, `\,`, `\;`, `\\`).
fn vcard_unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Lee un fichero vCard (2.1, 3.0 o 4.0). Cada dirección de correo es una entrada.
pub fn parse_vcards(text: &str) -> Vec<BookEntry> {
    // Las líneas que empiezan por espacio o tabulador continúan la anterior.
    let mut lines: Vec<String> = Vec::new();
    for l in text.replace("\r\n", "\n").split('\n') {
        match l.strip_prefix([' ', '\t']) {
            Some(rest) if !lines.is_empty() => lines.last_mut().expect("hay una").push_str(rest),
            _ => lines.push(l.to_string()),
        }
    }
    let mut out = Vec::new();
    let (mut name, mut n, mut phone, mut notes, mut emails) =
        (String::new(), String::new(), String::new(), String::new(), Vec::new());
    for l in lines {
        let Some((head, value)) = l.split_once(':') else {
            continue;
        };
        let prop = head.split(';').next().unwrap_or("").to_ascii_uppercase();
        // Quita el prefijo de grupo («item1.EMAIL»).
        let prop = prop.rsplit('.').next().unwrap_or("").to_string();
        let value = vcard_unescape(value.trim());
        match prop.as_str() {
            "BEGIN" => {
                (name, n, phone, notes) = Default::default();
                emails.clear();
            }
            "FN" => name = value,
            "N" => {
                let parts: Vec<&str> = value.split(';').collect();
                n = [parts.get(1), parts.first()]
                    .into_iter()
                    .flatten()
                    .filter(|p| !p.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ");
            }
            "EMAIL" if value.contains('@') => emails.push(value.trim_start_matches("mailto:").to_string()),
            "TEL" if phone.is_empty() => phone = value.trim_start_matches("tel:").to_string(),
            "NOTE" => notes = value,
            "END" => {
                let display = if name.trim().is_empty() {
                    n.clone()
                } else {
                    name.clone()
                };
                for e in emails.drain(..) {
                    out.push(BookEntry {
                        id: None,
                        name: display.trim().to_string(),
                        email: e.trim().to_string(),
                        phone: phone.trim().to_string(),
                        notes: notes.trim().to_string(),
                        source: "vcard".into(),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

impl UserDb {
    /// Contactos de la libreta; con `q`, los que lo contienen en el nombre o la dirección.
    pub async fn book(&self, q: &str) -> Result<Vec<BookEntry>> {
        let rows = sqlx::query(
            "SELECT * FROM contacts WHERE name LIKE ?1 OR email LIKE ?1 \
             ORDER BY CASE WHEN name = '' THEN email ELSE name END COLLATE NOCASE LIMIT 2000",
        )
        .bind(like(q))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(entry_from_row).collect()
    }

    pub async fn book_entry(&self, id: Id) -> Result<Option<BookEntry>> {
        let row = sqlx::query("SELECT * FROM contacts WHERE id = ?")
            .bind(b(id))
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(entry_from_row).transpose()
    }

    /// Crea o cambia un contacto. `Error::Duplicate` si la dirección ya está en otro.
    pub async fn save_book_entry(&self, e: &BookEntry) -> Result<Id> {
        let t = now();
        let id = match e.id {
            Some(id) => {
                let r = sqlx::query(
                    "UPDATE contacts SET name = ?, email = ?, phone = ?, notes = ?, updated_at = ? WHERE id = ?",
                )
                .bind(e.name.trim())
                .bind(e.email.trim())
                .bind(e.phone.trim())
                .bind(e.notes.trim())
                .bind(t)
                .bind(b(id))
                .execute(&self.pool)
                .await
                .map_err(map_unique)?;
                if r.rows_affected() == 0 {
                    return Err(crate::Error::BadId);
                }
                id
            }
            None => {
                let id = Id::new();
                sqlx::query(
                    "INSERT INTO contacts (id, name, email, phone, notes, source, created_at, updated_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(b(id))
                .bind(e.name.trim())
                .bind(e.email.trim())
                .bind(e.phone.trim())
                .bind(e.notes.trim())
                .bind(if e.source.is_empty() { "manual" } else { &e.source })
                .bind(t)
                .bind(t)
                .execute(&self.pool)
                .await
                .map_err(map_unique)?;
                id
            }
        };
        Ok(id)
    }

    pub async fn delete_book_entry(&self, id: Id) -> Result<bool> {
        let r = sqlx::query("DELETE FROM contacts WHERE id = ?")
            .bind(b(id))
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Añade a la libreta a quien escribes, si no estaba. Completa el nombre si faltaba.
    pub async fn remember_recipients(&self, list: &[Addr]) -> Result<()> {
        let t = now();
        for a in list {
            let name = a.name.as_deref().unwrap_or("").trim();
            sqlx::query(
                "INSERT INTO contacts (id, name, email, source, created_at, updated_at) VALUES (?, ?, ?, 'sent', ?, ?) \
                 ON CONFLICT (email) DO UPDATE SET name = excluded.name, updated_at = excluded.updated_at \
                 WHERE contacts.name = '' AND excluded.name <> ''",
            )
            .bind(b(Id::new()))
            .bind(name)
            .bind(a.email.trim())
            .bind(t)
            .bind(t)
            .execute(&self.pool)
            .await?;
            // Escribir a alguien que esperaba en «Por aprobar» lo aprueba.
            sqlx::query(
                "UPDATE senders SET status = 'approved', decided_at = ? WHERE email = ? AND status = 'pending'",
            )
            .bind(t)
            .bind(a.email.trim())
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Importa contactos de un vCard. Las direcciones que ya estaban se completan sin
    /// pisar lo que tenían. Devuelve cuántos son nuevos y cuántos ya existían.
    pub async fn import_vcards(&self, text: &str) -> Result<(usize, usize)> {
        let (mut added, mut existing) = (0, 0);
        let t = now();
        for e in parse_vcards(text) {
            let r = sqlx::query(
                "INSERT INTO contacts (id, name, email, phone, notes, source, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, 'vcard', ?, ?) ON CONFLICT (email) DO NOTHING",
            )
            .bind(b(Id::new()))
            .bind(&e.name)
            .bind(&e.email)
            .bind(&e.phone)
            .bind(&e.notes)
            .bind(t)
            .bind(t)
            .execute(&self.pool)
            .await?;
            if r.rows_affected() > 0 {
                added += 1;
            } else {
                existing += 1;
                sqlx::query(
                    "UPDATE contacts SET name = CASE WHEN name = '' THEN ? ELSE name END, \
                     phone = CASE WHEN phone = '' THEN ? ELSE phone END, \
                     notes = CASE WHEN notes = '' THEN ? ELSE notes END WHERE email = ?",
                )
                .bind(&e.name)
                .bind(&e.phone)
                .bind(&e.notes)
                .bind(&e.email)
                .execute(&self.pool)
                .await?;
            }
        }
        Ok((added, existing))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::mail::{Flags, NewMessage, ThreadQuery};

    pub(crate) async fn db() -> (tempfile::TempDir, UserDb, Id) {
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
                    imap_security: "tls",
                    smtp_security: "tls",
                },
                &key,
            )
            .await
            .unwrap();
        (dir, db, acc.id)
    }

    pub(crate) fn msg(
        account: Id,
        mailbox: Id,
        uid: u32,
        mid: &str,
        subject: &str,
        from: &str,
        text: &str,
    ) -> NewMessage {
        NewMessage {
            id: Id::new(),
            account_id: account,
            mailbox_id: mailbox,
            uid,
            message_id: Some(mid.into()),
            in_reply_to: None,
            refs: vec![],
            subject: subject.into(),
            from: Some(Addr {
                name: Some("Ana García".into()),
                email: from.into(),
            }),
            to: vec![Addr {
                name: None,
                email: "yo@ejemplo.es".into(),
            }],
            cc: vec![],
            date: 1_800_000_000 + uid as i64,
            snippet: text.into(),
            size: 10,
            flags: Flags::default(),
            text: Some(text.into()),
            html: None,
            remote_images: 0,
            attachments: vec![],
            kind: "principal".into(),
            trackers: vec![],
        }
    }

    fn q(search: &str) -> ThreadQuery {
        ThreadQuery {
            search: Some(parse_search(search)),
            limit: 50,
            ..Default::default()
        }
    }

    #[test]
    fn search_syntax() {
        let s = parse_search("de:ana presupuesto \"cena del viernes\" tiene:adjunto es:noleído despues:2026-10-01 en:archivo etiqueta:\"Viajes 2026\" 10:30");
        assert_eq!(s.from.as_deref(), Some("ana"));
        assert_eq!(
            s.text.as_deref(),
            Some("\"presupuesto\"* AND \"cena del viernes\" AND \"10 30\"")
        );
        assert!(s.attachments && s.unread && !s.flagged);
        assert_eq!(s.after, Some(1_790_812_800));
        assert_eq!(s.role.as_deref(), Some("archive"));
        assert_eq!(s.label.as_deref(), Some("Viajes 2026"));
        assert_eq!(
            parse_search("asunto:factura").text.as_deref(),
            Some("subject : \"factura\"*")
        );
        assert_eq!(
            parse_search("\"a\"\"b\" ( ) *").text.as_deref(),
            Some("\"a\" AND \"b\"")
        );
        assert!(parse_search("   ").is_empty());
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(parse_date("2026-13-01"), None);
    }

    #[tokio::test]
    async fn search_moves_and_labels() {
        let (_d, db, acc) = db().await;
        let inbox = db.upsert_mailbox(acc, "INBOX", None, Some("inbox")).await.unwrap();
        let archive = db.upsert_mailbox(acc, "Archivo", None, Some("archive")).await.unwrap();
        let t1 = db
            .insert_message(&msg(
                acc,
                inbox.id,
                1,
                "a@x",
                "Presupuesto reforma",
                "ana@x.es",
                "Te mando el presupuesto de la cocina",
            ))
            .await
            .unwrap();
        let t2 = db
            .insert_message(&msg(
                acc,
                inbox.id,
                2,
                "b@x",
                "Cena",
                "luis@x.es",
                "¿Cenamos el viernes? Llevo el vinó",
            ))
            .await
            .unwrap();

        let ids = |v: Vec<crate::mail::ThreadSummary>| v.into_iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids(db.threads(&q("presu")).await.unwrap()), vec![t1]);
        assert_eq!(ids(db.threads(&q("vino")).await.unwrap()), vec![t2], "sin acentos");
        assert_eq!(ids(db.threads(&q("de:luis")).await.unwrap()), vec![t2]);
        assert_eq!(ids(db.threads(&q("de:luis cocina")).await.unwrap()), Vec::<Id>::new());
        assert_eq!(ids(db.threads(&q("garcía")).await.unwrap()).len(), 2);

        // Etiquetas.
        let l = db.save_label(None, "Casa", "#2f9a72").await.unwrap();
        assert!(matches!(
            db.save_label(None, "casa", "#000").await,
            Err(crate::Error::Duplicate)
        ));
        db.label_threads(&[t1], &[l], &[]).await.unwrap();
        let page = db
            .threads(&ThreadQuery {
                label: Some(l),
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(ids(page.clone()), vec![t1]);
        assert_eq!(page[0].labels, vec![l]);
        assert_eq!(db.labels().await.unwrap()[0].unread, 1);

        // Archivar: se mueve ya en local con UID provisional.
        let ops = db
            .move_messages(
                &[t1],
                &[],
                Some(&Place::Role("inbox".into())),
                &Place::Role("archive".into()),
            )
            .await
            .unwrap();
        assert_eq!(
            ops,
            vec![MoveOp {
                account_id: acc,
                from: "INBOX".into(),
                to: "Archivo".into(),
                uids: vec![1]
            }]
        );
        let inbox_q = ThreadQuery {
            role: Some("inbox".into()),
            limit: 10,
            ..Default::default()
        };
        assert_eq!(ids(db.threads(&inbox_q).await.unwrap()), vec![t2]);
        assert!(db.known_uids(archive.id).await.unwrap().is_empty(), "aún sin UID real");
        // Un segundo movimiento no toca lo que espera confirmación.
        assert!(db
            .move_messages(&[t1], &[], None, &Place::Role("inbox".into()))
            .await
            .unwrap()
            .is_empty());
        assert!(matches!(
            db.move_messages(&[t2], &[], None, &Place::Role("junk".into())).await,
            Err(crate::Error::MissingFolder(_))
        ));

        // Llega la copia real al archivo: la provisional se va y la etiqueta sigue.
        db.insert_message(&msg(acc, archive.id, 7, "a@x", "Presupuesto reforma", "ana@x.es", "x"))
            .await
            .unwrap();
        assert_eq!(db.drop_confirmed_moves(archive.id).await.unwrap().len(), 1);
        let th = db.thread(t1).await.unwrap().unwrap();
        assert_eq!(th.messages.len(), 1);
        assert_eq!(th.messages[0].uid, 7);
        assert_eq!(db.thread_labels(t1).await.unwrap(), vec![l]);

        // Borrar para siempre solo afecta a papelera y spam.
        let trash = db.upsert_mailbox(acc, "Papelera", None, Some("trash")).await.unwrap();
        let (gone, _) = db.delete_forever(&[t2], None).await.unwrap();
        assert!(gone.is_empty());
        db.move_messages(&[t2], &[], None, &Place::Mailbox(trash.id))
            .await
            .unwrap();
        let (gone, ops) = db.delete_forever(&[t2], None).await.unwrap();
        assert_eq!(gone.len(), 1);
        assert!(ops.is_empty(), "sin UID real no hay nada que borrar en el servidor");
        assert!(db.thread(t2).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn rules_and_contacts() {
        let (_d, db, acc) = db().await;
        let l = db.save_label(None, "Facturas", "#c4683a").await.unwrap();
        let rule = Rule {
            name: "Facturas".into(),
            enabled: true,
            from_has: "@luz.es".into(),
            subject_has: "factura".into(),
            label_id: Some(l),
            mark_read: true,
            move_to: "archive".into(),
            ..Default::default()
        };
        db.save_rule(&rule).await.unwrap();
        db.save_rule(&Rule {
            name: "Vacío".into(),
            enabled: true,
            star: true,
            ..Default::default()
        })
        .await
        .unwrap();
        let rules = db.rules().await.unwrap();
        assert_eq!(rules.len(), 2);
        let from = Addr {
            name: Some("Luz".into()),
            email: "avisos@luz.es".into(),
        };
        let input = RuleInput {
            account_id: acc,
            from: Some(&from),
            to: &[],
            cc: &[],
            subject: "Tu FACTURA de octubre",
            text: "",
        };
        let a = apply_rules(&rules, &input);
        assert_eq!(a.labels, vec![l]);
        assert!(a.mark_read && !a.star, "un filtro sin condiciones no hace nada");
        assert_eq!(a.move_to.as_deref(), Some("archive"));
        assert!(apply_rules(
            &rules,
            &RuleInput {
                subject: "Hola",
                ..input
            }
        )
        .is_empty());

        // Contactos.
        db.remember_recipients(&[Addr {
            name: None,
            email: "ana@x.es".into(),
        }])
        .await
        .unwrap();
        db.remember_recipients(&[Addr {
            name: Some("Ana García".into()),
            email: "ANA@x.es".into(),
        }])
        .await
        .unwrap();
        let book = db.book("").await.unwrap();
        assert_eq!(book.len(), 1);
        assert_eq!(book[0].name, "Ana García");
        let suggested = db.contacts("ana", 8).await.unwrap();
        assert_eq!(suggested.len(), 1);
        assert_eq!(suggested[0].name.as_deref(), Some("Ana García"));
        let vcf = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Luis Pérez\r\nEMAIL;TYPE=INTERNET:luis@x.es\r\n\
                   item1.EMAIL:luis@trabajo.es\r\nTEL:+34 600\r\nNOTE:Vecino\\, del 3º\r\nEND:VCARD\r\n\
                   BEGIN:VCARD\r\nVERSION:2.1\r\nN:García;Ana;;;\r\nEMAIL:ana@x.es\r\nNOTE:Larga\r\n  continuación\r\nEND:VCARD\r\n";
        let parsed = parse_vcards(vcf);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].notes, "Vecino, del 3º");
        assert_eq!(parsed[2].name, "Ana García");
        assert_eq!(parsed[2].notes, "Larga continuación");
        assert_eq!(db.import_vcards(vcf).await.unwrap(), (2, 1));
        let ana = db.book("ana@").await.unwrap();
        assert_eq!(ana[0].notes, "Larga continuación", "completa lo que faltaba");
        assert_eq!(ana[0].source, "sent");
        let id = ana[0].id.unwrap();
        assert!(matches!(
            db.save_book_entry(&BookEntry {
                email: "luis@x.es".into(),
                ..ana[0].clone()
            })
            .await,
            Err(crate::Error::Duplicate)
        ));
        assert!(db.delete_book_entry(id).await.unwrap());
        assert_eq!(db.book("").await.unwrap().len(), 2);
    }
}

//! Bucle de sincronización de una cuenta.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_imap::extensions::idle::IdleResponse;
use async_imap::types::{Fetch, Flag, NameAttribute};
use buzon_store::mail::{decode_mailbox_name, Flags, Mailbox};
use buzon_store::organize::{self, Place, Rule, RuleActions, RuleInput};
use buzon_store::{now, Id, UserDb};
use futures::TryStreamExt;
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::conn::{self, ServerConfig, Session};
use crate::parse::{self, Envelope};
use crate::{Cmd, Result, Shared, State};

/// Mensajes más recientes que se descargan la primera vez en cada carpeta.
const INITIAL: usize = 300;
/// Mensajes antiguos que se descargan en cada vuelta hasta completar el historial.
const BACKFILL: usize = 300;
/// Límite de cada descarga: número de mensajes y tamaño total.
const BATCH_COUNT: usize = 25;
const BATCH_BYTES: u64 = 20 * 1024 * 1024;
/// Cada cuánto se revisan todas las carpetas (la bandeja de entrada se escucha con IDLE).
const FULL_SYNC: Duration = Duration::from_secs(10 * 60);
/// Sin IDLE, cada cuánto se mira la bandeja de entrada.
const POLL: Duration = Duration::from_secs(2 * 60);
const MIN_RETRY: Duration = Duration::from_secs(30);
const MAX_RETRY: Duration = Duration::from_secs(15 * 60);

pub(crate) struct Ctx {
    pub user: Id,
    pub account: Id,
    pub db: UserDb,
    pub cfg: ServerConfig,
    pub raw_dir: PathBuf,
    pub shared: Arc<Shared>,
}

impl Ctx {
    fn state(&self, s: State) {
        self.shared.set_state(self.user, self.account, s);
    }

    fn changed(&self) {
        self.shared.emit(self.user, "mail", self.account);
    }

    fn remove_raw(&self, ids: &[Id]) {
        for id in ids {
            let _ = std::fs::remove_file(self.raw_dir.join(format!("{id}.eml")));
        }
    }
}

enum Wake {
    Stop,
    SyncNow,
    NewData,
    Timeout,
    Cmd(Cmd),
}

pub(crate) async fn run(ctx: Ctx, mut rx: mpsc::UnboundedReceiver<Cmd>) {
    if let Err(e) = std::fs::create_dir_all(&ctx.raw_dir) {
        tracing::error!("no se puede crear {}: {e}", ctx.raw_dir.display());
    }
    let mut retry = MIN_RETRY;
    let mut pending = Vec::new();
    loop {
        ctx.state(State::Connecting);
        let err = match session(&ctx, &mut rx, &mut pending, &mut retry).await {
            Ok(()) => return,
            Err(e) => e,
        };
        let msg = err.to_string();
        tracing::warn!("cuenta {} ({:?}): {msg}", ctx.account, ctx.cfg);
        let _ = ctx.db.set_sync_result(ctx.account, Some(&msg)).await;
        ctx.state(State::Error);
        tokio::select! {
            _ = tokio::time::sleep(retry) => {}
            cmd = rx.recv() => match cmd {
                None | Some(Cmd::Stop) => return,
                Some(Cmd::SyncNow) => {}
                Some(c) => pending.push(c),
            }
        }
        retry = (retry * 2).min(MAX_RETRY);
    }
}

async fn session(
    ctx: &Ctx,
    rx: &mut mpsc::UnboundedReceiver<Cmd>,
    pending: &mut Vec<Cmd>,
    retry: &mut Duration,
) -> Result<()> {
    let mut s = conn::connect(&ctx.cfg).await?;
    let caps = {
        let c = s.capabilities().await?;
        Caps {
            idle: c.has_str("IDLE"),
            mv: c.has_str("MOVE"),
            uidplus: c.has_str("UIDPLUS"),
        }
    };
    let idle = caps.idle;
    loop {
        ctx.state(State::Syncing);
        for c in pending.drain(..) {
            apply(&mut s, c, caps).await?;
        }
        let (inbox, more) = full_sync(&mut s, ctx, caps).await?;
        ctx.db.set_sync_result(ctx.account, None).await?;
        ctx.state(State::Idle);
        *retry = MIN_RETRY;

        // Mientras falte historial por bajar, se vuelve enseguida a por más.
        let next_full = Instant::now() + if more { Duration::from_secs(1) } else { FULL_SYNC };
        loop {
            let left = next_full.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let wake;
            (s, wake) = match &inbox {
                Some(mb) if idle => wait_idle(s, &mb.name, rx, left).await?,
                _ => wait_poll(s, rx, left.min(POLL)).await,
            };
            match wake {
                Wake::Stop => {
                    let _ = s.logout().await;
                    return Ok(());
                }
                Wake::SyncNow => break,
                Wake::Cmd(c) => apply(&mut s, c, caps).await?,
                Wake::NewData | Wake::Timeout => {
                    if let Some(mb) = &inbox {
                        let fresh = ctx.db.mailbox(mb.id).await?.unwrap_or_else(|| mb.clone());
                        if sync_mailbox(&mut s, ctx, &fresh, caps).await?.changed {
                            ctx.changed();
                        }
                    }
                }
            }
        }
    }
}

fn wake_from(cmd: Option<Cmd>) -> Wake {
    match cmd {
        None | Some(Cmd::Stop) => Wake::Stop,
        Some(Cmd::SyncNow) => Wake::SyncNow,
        Some(c) => Wake::Cmd(c),
    }
}

async fn wait_idle(
    mut s: Session,
    inbox: &str,
    rx: &mut mpsc::UnboundedReceiver<Cmd>,
    dur: Duration,
) -> Result<(Session, Wake)> {
    s.select(inbox).await?;
    let mut h = s.idle();
    h.init().await?;
    let wake = {
        let (fut, stop) = h.wait_with_timeout(dur.min(Duration::from_secs(25 * 60)));
        tokio::pin!(fut);
        tokio::select! {
            r = &mut fut => match r? {
                IdleResponse::NewData(_) => Wake::NewData,
                _ => Wake::Timeout,
            },
            cmd = rx.recv() => {
                drop(stop);
                let _ = (&mut fut).await;
                wake_from(cmd)
            }
        }
    };
    Ok((h.done().await?, wake))
}

async fn wait_poll(s: Session, rx: &mut mpsc::UnboundedReceiver<Cmd>, dur: Duration) -> (Session, Wake) {
    let wake = tokio::select! {
        _ = tokio::time::sleep(dur) => Wake::Timeout,
        cmd = rx.recv() => wake_from(cmd),
    };
    (s, wake)
}

/// Lo que sabe hacer el servidor, aparte de lo básico.
#[derive(Clone, Copy)]
struct Caps {
    idle: bool,
    mv: bool,
    uidplus: bool,
}

async fn imap_move(s: &mut Session, from: &str, to: &str, uids: &[u32], caps: Caps) -> Result<()> {
    s.select(from).await?;
    let set = uid_set(uids);
    if caps.mv {
        s.uid_mv(&set, to).await?;
    } else {
        s.uid_copy(&set, to).await?;
        expunge(s, &set, caps).await?;
    }
    Ok(())
}

/// Marca como borrados y los elimina. Sin UIDPLUS, EXPUNGE borra también lo que otro
/// programa hubiera marcado como borrado en esa carpeta (es lo que haría él).
async fn expunge(s: &mut Session, set: &str, caps: Caps) -> Result<()> {
    let _: Vec<Fetch> = s
        .uid_store(set, "+FLAGS.SILENT (\\Deleted)")
        .await?
        .try_collect()
        .await?;
    if caps.uidplus {
        let _: Vec<_> = s.uid_expunge(set).await?.try_collect().await?;
    } else {
        let _: Vec<_> = s.expunge().await?.try_collect().await?;
    }
    Ok(())
}

async fn apply(s: &mut Session, cmd: Cmd, caps: Caps) -> Result<()> {
    match cmd {
        Cmd::Store {
            mailbox,
            uids,
            flag,
            on,
        } => {
            s.select(&mailbox).await?;
            let set = uid_set(&uids);
            let query = format!("{}FLAGS.SILENT ({flag})", if on { "+" } else { "-" });
            let _: Vec<Fetch> = s.uid_store(&set, &query).await?.try_collect().await?;
        }
        Cmd::Append { mailbox, raw } => s.append(&mailbox, Some("(\\Seen)"), None, &raw).await?,
        Cmd::Move { from, to, uids } => imap_move(s, &from, &to, &uids, caps).await?,
        Cmd::Expunge { mailbox, uids } => {
            s.select(&mailbox).await?;
            expunge(s, &uid_set(&uids), caps).await?;
        }
        Cmd::SyncNow | Cmd::Stop => {}
    }
    Ok(())
}

fn uid_set(uids: &[u32]) -> String {
    uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

/// Tipo de carpeta por sus atributos SPECIAL-USE o, si no los tiene, por su nombre.
pub(crate) fn role_of(name: &str, delimiter: Option<&str>, attrs: &[NameAttribute<'_>]) -> Option<&'static str> {
    if name.eq_ignore_ascii_case("INBOX") {
        return Some("inbox");
    }
    for a in attrs {
        let role = match a {
            NameAttribute::Sent => "sent",
            NameAttribute::Drafts => "drafts",
            NameAttribute::Junk => "junk",
            NameAttribute::Trash => "trash",
            NameAttribute::Archive => "archive",
            _ => continue,
        };
        return Some(role);
    }
    let decoded = decode_mailbox_name(name);
    let leaf = match delimiter {
        Some(d) if !d.is_empty() => decoded.rsplit(d).next().unwrap_or(&decoded),
        _ => &decoded,
    }
    .to_lowercase();
    Some(match leaf.as_str() {
        "sent" | "sent items" | "sent messages" | "sent mail" | "enviados" | "elementos enviados" | "enviado" => "sent",
        "drafts" | "draft" | "borradores" => "drafts",
        "trash" | "bin" | "deleted items" | "deleted messages" | "papelera" | "elementos eliminados" => "trash",
        "junk" | "spam" | "junk e-mail" | "junk email" | "bulk mail" | "correo no deseado" => "junk",
        "archive" | "archives" | "archivo" | "archivados" => "archive",
        _ => return None,
    })
}

/// Repasa todas las carpetas. Devuelve la bandeja de entrada y si falta historial.
async fn full_sync(s: &mut Session, ctx: &Ctx, caps: Caps) -> Result<(Option<Mailbox>, bool)> {
    let names: Vec<_> = s.list(Some(""), Some("*")).await?.try_collect().await?;
    let mut keep = Vec::new();
    let mut boxes = Vec::new();
    for n in &names {
        let attrs = n.attributes();
        if attrs
            .iter()
            .any(|a| matches!(a, NameAttribute::NoSelect | NameAttribute::All | NameAttribute::Flagged))
        {
            continue;
        }
        let role = role_of(n.name(), n.delimiter(), attrs);
        keep.push(n.name().to_string());
        boxes.push(
            ctx.db
                .upsert_mailbox(ctx.account, n.name(), n.delimiter(), role)
                .await?,
        );
    }
    let gone = ctx.db.retain_mailboxes(ctx.account, &keep).await?;
    ctx.remove_raw(&gone);
    if !gone.is_empty() {
        ctx.changed();
    }

    boxes.sort_by_key(|m| m.role.as_deref() != Some("inbox"));
    let mut more = false;
    for mb in &boxes {
        let r = sync_mailbox(s, ctx, mb, caps).await?;
        more |= r.more;
        if r.changed {
            ctx.changed();
        }
    }
    let inbox = boxes.into_iter().find(|m| m.role.as_deref() == Some("inbox"));
    Ok((inbox, more))
}

struct MailboxSync {
    changed: bool,
    more: bool,
}

fn flags_of(f: &Fetch) -> Flags {
    let mut out = Flags::default();
    for fl in f.flags() {
        match fl {
            Flag::Seen => out.seen = true,
            Flag::Flagged => out.flagged = true,
            Flag::Answered => out.answered = true,
            _ => {}
        }
    }
    out
}

async fn search(s: &mut Session, query: &str) -> Result<Vec<u32>> {
    let mut v: Vec<u32> = s.uid_search(query).await?.into_iter().collect();
    v.sort_unstable();
    Ok(v)
}

async fn sync_mailbox(s: &mut Session, ctx: &Ctx, mb: &Mailbox, caps: Caps) -> Result<MailboxSync> {
    let info = s.select(&mb.name).await?;
    let validity = info.uid_validity.unwrap_or(0);
    let (mut last_uid, mut oldest) = (mb.last_uid, mb.oldest_uid);
    let mut changed = false;

    if mb.uidvalidity.is_some_and(|v| v != validity) {
        let gone = ctx.db.clear_mailbox(mb.id).await?;
        ctx.remove_raw(&gone);
        changed |= !gone.is_empty();
        (last_uid, oldest) = (0, None);
    }

    // Cambios de flags y mensajes borrados en otros clientes.
    let known = ctx.db.known_uids(mb.id).await?;
    if let Some(&(max_known, _, _)) = known.last() {
        let server: HashMap<u32, Flags> = if info.exists == 0 {
            HashMap::new()
        } else {
            let fetches: Vec<Fetch> = s
                .uid_fetch(format!("1:{max_known}"), "(UID FLAGS)")
                .await?
                .try_collect()
                .await?;
            fetches.iter().filter_map(|f| Some((f.uid?, flags_of(f)))).collect()
        };
        let mut gone = Vec::new();
        for (uid, id, flags) in &known {
            match server.get(uid) {
                None => gone.push(*id),
                Some(f) if f != flags => {
                    ctx.db.set_flags(*id, *f).await?;
                    changed = true;
                }
                _ => {}
            }
        }
        if !gone.is_empty() {
            ctx.db.delete_messages(&gone).await?;
            ctx.remove_raw(&gone);
            changed = true;
        }
    }

    // Mensajes nuevos.
    let new = if info.exists == 0 {
        if last_uid == 0 {
            oldest = Some(0);
        }
        Vec::new()
    } else if last_uid == 0 {
        let mut all = search(s, "ALL").await?;
        let start = all.len().saturating_sub(INITIAL);
        oldest = Some(if start == 0 { 0 } else { all[start] });
        all.split_off(start)
    } else {
        let mut v = search(s, &format!("UID {}:*", last_uid + 1)).await?;
        v.retain(|u| *u > last_uid);
        v
    };
    if !new.is_empty() {
        // Los filtros automáticos solo se aplican al correo que llega a la entrada,
        // no al que se baja la primera vez.
        let rules = if mb.last_uid > 0 && mb.role.as_deref() == Some("inbox") {
            Some(ctx.db.rules().await?)
        } else {
            None
        };
        let mut hits = Vec::new();
        download(s, ctx, mb, &new, rules.as_deref(), &mut hits).await?;
        last_uid = last_uid.max(*new.last().expect("no vacío"));
        changed = true;
        ctx.db.set_mailbox_state(mb.id, validity, last_uid, oldest).await?;
        apply_rules(s, ctx, mb, hits, caps).await?;
    }

    // Historial antiguo, por tandas.
    if let Some(o) = oldest.filter(|o| *o > 1) {
        let mut older = search(s, &format!("UID 1:{}", o - 1)).await?;
        older.retain(|u| *u < o);
        let start = older.len().saturating_sub(BACKFILL);
        let batch = older.split_off(start);
        if !batch.is_empty() {
            download(s, ctx, mb, &batch, None, &mut Vec::new()).await?;
            changed = true;
        }
        oldest = Some(if start == 0 { 0 } else { batch[0] });
    }
    ctx.db.set_mailbox_state(mb.id, validity, last_uid, oldest).await?;

    // Lo movido desde Buzón ya tiene su copia real aquí.
    let confirmed = ctx.db.drop_confirmed_moves(mb.id).await?;
    if !confirmed.is_empty() {
        ctx.remove_raw(&confirmed);
        changed = true;
    }
    Ok(MailboxSync {
        changed,
        more: oldest.is_some_and(|o| o > 1),
    })
}

/// Descarga y guarda mensajes, en tandas limitadas por número y tamaño.
/// Un mensaje recién llegado que cumple algún filtro automático.
struct Hit {
    uid: u32,
    message: Id,
    thread: Id,
    flags: Flags,
    actions: RuleActions,
}

async fn apply_rules(s: &mut Session, ctx: &Ctx, mb: &Mailbox, hits: Vec<Hit>, caps: Caps) -> Result<()> {
    for h in hits {
        let a = &h.actions;
        if !a.labels.is_empty() {
            ctx.db.label_threads(&[h.thread], &a.labels, &[]).await?;
        }
        let flags = Flags {
            seen: h.flags.seen || a.mark_read,
            flagged: h.flags.flagged || a.star,
            answered: h.flags.answered,
        };
        if flags != h.flags {
            ctx.db.set_flags(h.message, flags).await?;
            let mut names = Vec::new();
            if a.mark_read {
                names.push("\\Seen");
            }
            if a.star {
                names.push("\\Flagged");
            }
            s.select(&mb.name).await?;
            let query = format!("+FLAGS.SILENT ({})", names.join(" "));
            let _: Vec<Fetch> = s.uid_store(h.uid.to_string(), &query).await?.try_collect().await?;
        }
        if let Some(role) = &a.move_to {
            match ctx
                .db
                .move_messages(&[], &[h.message], None, &Place::Role(role.clone()))
                .await
            {
                Ok(ops) => {
                    for op in ops {
                        imap_move(s, &op.from, &op.to, &op.uids, caps).await?;
                    }
                }
                Err(buzon_store::Error::MissingFolder(f)) => {
                    tracing::warn!("cuenta {}: un filtro mueve a «{f}», pero no existe", ctx.account)
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    Ok(())
}

async fn download(
    s: &mut Session,
    ctx: &Ctx,
    mb: &Mailbox,
    uids: &[u32],
    rules: Option<&[Rule]>,
    hits: &mut Vec<Hit>,
) -> Result<()> {
    for chunk in uids.chunks(200) {
        let sizes: Vec<Fetch> = s
            .uid_fetch(uid_set(chunk), "(UID RFC822.SIZE)")
            .await?
            .try_collect()
            .await?;
        let mut sizes: Vec<(u32, u64)> = sizes
            .iter()
            .filter_map(|f| Some((f.uid?, f.size.unwrap_or(0) as u64)))
            .collect();
        sizes.sort_unstable();
        let mut batch: Vec<u32> = Vec::new();
        let mut bytes = 0;
        for (uid, size) in sizes {
            if !batch.is_empty() && (batch.len() >= BATCH_COUNT || bytes + size > BATCH_BYTES) {
                fetch_batch(s, ctx, mb, &batch, rules, hits).await?;
                batch.clear();
                bytes = 0;
            }
            batch.push(uid);
            bytes += size;
        }
        if !batch.is_empty() {
            fetch_batch(s, ctx, mb, &batch, rules, hits).await?;
        }
    }
    Ok(())
}

async fn fetch_batch(
    s: &mut Session,
    ctx: &Ctx,
    mb: &Mailbox,
    uids: &[u32],
    rules: Option<&[Rule]>,
    hits: &mut Vec<Hit>,
) -> Result<()> {
    let fetches: Vec<Fetch> = s
        .uid_fetch(uid_set(uids), "(UID FLAGS RFC822.SIZE BODY.PEEK[])")
        .await?
        .try_collect()
        .await?;
    for f in &fetches {
        let (Some(uid), Some(raw)) = (f.uid, f.body()) else {
            continue;
        };
        let id = Id::new();
        let env = Envelope {
            id,
            account_id: ctx.account,
            mailbox_id: mb.id,
            uid,
            size: f.size.unwrap_or(raw.len() as u32),
            flags: flags_of(f),
            fallback_date: now(),
        };
        let raw = raw.to_vec();
        let (msg, raw) = tokio::task::spawn_blocking(move || (parse::parse(&raw, env), raw))
            .await
            .map_err(|_| crate::Error::Config("análisis interrumpido"))?;
        tokio::fs::write(ctx.raw_dir.join(format!("{id}.eml")), &raw).await?;
        let thread = ctx.db.insert_message(&msg).await?;
        // Remitentes nuevos: lo que llega después de la primera descarga espera aprobación.
        if mb.role.as_deref() == Some("inbox") {
            if let Some(from) = &msg.from {
                ctx.db.screen_sender(from, &msg.kind, rules.is_some()).await?;
            }
        }
        if let Some(rules) = rules {
            let actions = organize::apply_rules(
                rules,
                &RuleInput {
                    account_id: ctx.account,
                    from: msg.from.as_ref(),
                    to: &msg.to,
                    cc: &msg.cc,
                    subject: &msg.subject,
                    text: msg.text.as_deref().unwrap_or(""),
                },
            );
            if !actions.is_empty() {
                hits.push(Hit {
                    uid,
                    message: msg.id,
                    thread,
                    flags: msg.flags,
                    actions,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_from_attributes_and_names() {
        assert_eq!(role_of("INBOX", Some("/"), &[]), Some("inbox"));
        assert_eq!(role_of("Cosas", Some("/"), &[NameAttribute::Sent]), Some("sent"));
        assert_eq!(role_of("INBOX.Enviados", Some("."), &[]), Some("sent"));
        assert_eq!(role_of("Elementos eliminados", None, &[]), Some("trash"));
        assert_eq!(role_of("Correo no deseado", Some("/"), &[]), Some("junk"));
        assert_eq!(role_of("Proyectos/2026", Some("/"), &[]), None);
        assert_eq!(uid_set(&[1, 5, 9]), "1,5,9");
    }
}

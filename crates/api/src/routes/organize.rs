//! Organizar: mover y borrar hilos (también varios a la vez), etiquetas, filtros
//! automáticos y la libreta de contactos.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use buzon_store::organize::{BookEntry, Place, Rule};
use buzon_store::user::user_raw_dir;
use buzon_store::{Id, UserDb};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::error::{ApiError, ApiResult, ErrorBody};
use crate::models::{
    BookEntryInput, BookEntryOut, DeleteResult, ImportResult, LabelInput, LabelOut, MoveResult, RuleData, RuleOut,
    ThreadsDelete, ThreadsFlags, ThreadsLabels, ThreadsMove, VcardImport,
};
use crate::session::CurrentUser;
use crate::AppState;

/// Hilos por petición, como máximo.
const MAX_THREADS: usize = 1000;
const MOVE_ROLES: [&str; 4] = ["inbox", "archive", "trash", "junk"];
const ROLES: [&str; 6] = ["inbox", "sent", "drafts", "archive", "junk", "trash"];

fn parse_id(s: &str, what: &str) -> ApiResult<Id> {
    s.parse().map_err(|_| ApiError::not_found(format!("{what} no existe.")))
}

fn check_threads(t: &[Id]) -> ApiResult<()> {
    if t.is_empty() || t.len() > MAX_THREADS {
        return Err(ApiError::bad_request(format!("Indica entre 1 y {MAX_THREADS} hilos.")));
    }
    Ok(())
}

fn place(role: Option<String>, mailbox: Option<Id>, roles: &[&str]) -> ApiResult<Option<Place>> {
    match (role, mailbox) {
        (Some(_), Some(_)) => Err(ApiError::bad_request(
            "Indica una carpeta o un tipo de carpeta, no los dos.",
        )),
        (Some(r), None) if !roles.contains(&r.as_str()) => {
            Err(ApiError::bad_request("Ese tipo de carpeta no vale aquí."))
        }
        (Some(r), None) => Ok(Some(Place::Role(r))),
        (None, Some(m)) => Ok(Some(Place::Mailbox(m))),
        (None, None) => Ok(None),
    }
}

/// Cuentas de los hilos, para avisar a las demás pestañas.
async fn accounts_of(udb: &UserDb, threads: &[Id]) -> ApiResult<Vec<Id>> {
    let mut out = Vec::new();
    for t in threads.iter().take(50) {
        if let Some(th) = udb.thread(*t).await? {
            if !out.contains(&th.account_id) {
                out.push(th.account_id);
            }
        }
    }
    Ok(out)
}

// ---------- Hilos ----------

/// Mueve hilos a otra carpeta (archivar, papelera, spam, devolver a la entrada…).
/// Se ve al momento en Buzón y se aplica en el servidor IMAP por detrás.
#[utoipa::path(post, path = "/api/v1/threads/move", tag = "organize", request_body = ThreadsMove,
    responses((status = 200, body = MoveResult), (status = 400, body = ErrorBody),
        (status = 409, description = "La cuenta no tiene esa carpeta (`missing_folder`)", body = ErrorBody)))]
pub async fn move_threads(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<ThreadsMove>,
) -> ApiResult<Json<MoveResult>> {
    check_threads(&req.threads)?;
    let from = place(req.from_role, req.from_mailbox, &ROLES)?;
    let to = place(req.to_role, req.to_mailbox, &MOVE_ROLES)?
        .ok_or_else(|| ApiError::bad_request("Indica adónde mover (to_role o to_mailbox)."))?;
    let udb = st.user_db(cu.user.id).await?;
    let ops = udb.move_messages(&req.threads, &[], from.as_ref(), &to).await?;
    st.sync.move_messages(&ops);
    let mut accounts: Vec<Id> = ops.iter().map(|o| o.account_id).collect();
    accounts.dedup();
    for a in accounts {
        st.sync.emit(cu.user.id, a);
    }
    Ok(Json(MoveResult {
        moved: ops.iter().map(|o| o.uids.len()).sum(),
    }))
}

/// Borra para siempre lo que los hilos tienen en la papelera o en spam.
#[utoipa::path(post, path = "/api/v1/threads/delete", tag = "organize", request_body = ThreadsDelete,
    responses((status = 200, body = DeleteResult), (status = 400, body = ErrorBody)))]
pub async fn delete_threads(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<ThreadsDelete>,
) -> ApiResult<Json<DeleteResult>> {
    check_threads(&req.threads)?;
    let from = place(req.from_role, req.from_mailbox, &ROLES)?;
    let udb = st.user_db(cu.user.id).await?;
    let accounts = accounts_of(&udb, &req.threads).await?;
    let (ids, ops) = udb.delete_forever(&req.threads, from.as_ref()).await?;
    st.sync.expunge(&ops);
    let raw = user_raw_dir(&st.data_dir, cu.user.id);
    for id in &ids {
        let _ = tokio::fs::remove_file(raw.join(format!("{id}.eml"))).await;
    }
    for a in accounts {
        st.sync.emit(cu.user.id, a);
    }
    let _ = st.db.audit(Some(cu.user.id), "mail.deleted", None).await;
    Ok(Json(DeleteResult { deleted: ids.len() }))
}

/// Marca varios hilos como leídos o no leídos, o los destaca.
#[utoipa::path(post, path = "/api/v1/threads/flags", tag = "organize", request_body = ThreadsFlags,
    responses((status = 204), (status = 400, body = ErrorBody)))]
pub async fn flag_threads(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<ThreadsFlags>,
) -> ApiResult<StatusCode> {
    check_threads(&req.threads)?;
    let udb = st.user_db(cu.user.id).await?;
    let (mut seen, mut flagged) = (Vec::new(), Vec::new());
    for t in &req.threads {
        let c = udb.update_thread(*t, req.seen, req.flagged).await?;
        seen.extend(c.seen);
        flagged.extend(c.flagged);
    }
    if let Some(v) = req.seen {
        st.sync.store_flag(&seen, "\\Seen", v);
    }
    if let Some(v) = req.flagged {
        st.sync.store_flag(&flagged, "\\Flagged", v);
    }
    let mut accounts: Vec<Id> = seen.iter().chain(&flagged).map(|t| t.account_id).collect();
    accounts.sort();
    accounts.dedup();
    for a in accounts {
        st.sync.emit(cu.user.id, a);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Pone o quita etiquetas a varios hilos.
#[utoipa::path(post, path = "/api/v1/threads/labels", tag = "organize", request_body = ThreadsLabels,
    responses((status = 204), (status = 400, body = ErrorBody)))]
pub async fn label_threads(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<ThreadsLabels>,
) -> ApiResult<StatusCode> {
    check_threads(&req.threads)?;
    let udb = st.user_db(cu.user.id).await?;
    udb.label_threads(&req.threads, &req.add, &req.remove).await?;
    for a in accounts_of(&udb, &req.threads).await? {
        st.sync.emit(cu.user.id, a);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Etiquetas ----------

fn check_label(l: &LabelInput) -> ApiResult<()> {
    let name = l.name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(ApiError::bad_request("El nombre debe tener entre 1 y 60 caracteres."));
    }
    let c = l.color.as_bytes();
    if c.len() != 7 || c[0] != b'#' || !c[1..].iter().all(u8::is_ascii_hexdigit) {
        return Err(ApiError::bad_request("El color debe ser como #2f9a72."));
    }
    Ok(())
}

fn label_conflict(e: buzon_store::Error) -> ApiError {
    match e {
        buzon_store::Error::Duplicate => ApiError::conflict("duplicate", "Ya tienes una etiqueta con ese nombre."),
        e => e.into(),
    }
}

async fn label_out(udb: &UserDb, id: Id) -> ApiResult<LabelOut> {
    udb.labels()
        .await?
        .into_iter()
        .find(|l| l.id == id)
        .map(Into::into)
        .ok_or_else(|| ApiError::not_found("Esa etiqueta no existe."))
}

#[utoipa::path(get, path = "/api/v1/labels", tag = "organize", responses((status = 200, body = [LabelOut])))]
pub async fn labels(State(st): State<Arc<AppState>>, cu: CurrentUser) -> ApiResult<Json<Vec<LabelOut>>> {
    let udb = st.user_db(cu.user.id).await?;
    Ok(Json(udb.labels().await?.into_iter().map(Into::into).collect()))
}

#[utoipa::path(post, path = "/api/v1/labels", tag = "organize", request_body = LabelInput,
    responses((status = 200, body = LabelOut), (status = 409, body = ErrorBody)))]
pub async fn create_label(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<LabelInput>,
) -> ApiResult<Json<LabelOut>> {
    check_label(&req)?;
    let udb = st.user_db(cu.user.id).await?;
    let id = udb
        .save_label(None, &req.name, &req.color.to_ascii_lowercase())
        .await
        .map_err(label_conflict)?;
    Ok(Json(label_out(&udb, id).await?))
}

#[utoipa::path(put, path = "/api/v1/labels/{id}", tag = "organize", request_body = LabelInput,
    params(("id" = String, Path, description = "Id de la etiqueta")),
    responses((status = 200, body = LabelOut), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)))]
pub async fn update_label(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<LabelInput>,
) -> ApiResult<Json<LabelOut>> {
    let id = parse_id(&id, "Esa etiqueta")?;
    check_label(&req)?;
    let udb = st.user_db(cu.user.id).await?;
    udb.save_label(Some(id), &req.name, &req.color.to_ascii_lowercase())
        .await
        .map_err(label_conflict)?;
    Ok(Json(label_out(&udb, id).await?))
}

/// Borra la etiqueta (los hilos no se tocan).
#[utoipa::path(delete, path = "/api/v1/labels/{id}", tag = "organize",
    params(("id" = String, Path, description = "Id de la etiqueta")),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn delete_label(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let id = parse_id(&id, "Esa etiqueta")?;
    let udb = st.user_db(cu.user.id).await?;
    if !udb.delete_label(id).await? {
        return Err(ApiError::not_found("Esa etiqueta no existe."));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Filtros ----------

async fn rule_from(udb: &UserDb, id: Option<Id>, r: RuleData) -> ApiResult<Rule> {
    let rule = Rule {
        id,
        name: r.name.trim().to_string(),
        enabled: r.enabled,
        account_id: r.account_id,
        from_has: r.from_has,
        to_has: r.to_has,
        subject_has: r.subject_has,
        body_has: r.body_has,
        label_id: r.label_id,
        mark_read: r.mark_read,
        star: r.star,
        move_to: r.move_to,
    };
    if rule.name.is_empty() {
        return Err(ApiError::bad_request("Ponle un nombre al filtro."));
    }
    if !rule.has_conditions() {
        return Err(ApiError::bad_request("Indica al menos una condición."));
    }
    if !["", "archive", "trash", "junk"].contains(&rule.move_to.as_str()) {
        return Err(ApiError::bad_request(
            "Solo se puede mover al archivo, a la papelera o a spam.",
        ));
    }
    if rule.label_id.is_none() && !rule.mark_read && !rule.star && rule.move_to.is_empty() {
        return Err(ApiError::bad_request(
            "Indica qué hacer con el correo que cumpla el filtro.",
        ));
    }
    if let Some(a) = rule.account_id {
        if udb.account(a).await?.is_none() {
            return Err(ApiError::not_found("Esa cuenta no existe."));
        }
    }
    if let Some(l) = rule.label_id {
        if !udb.labels().await?.iter().any(|x| x.id == l) {
            return Err(ApiError::not_found("Esa etiqueta no existe."));
        }
    }
    Ok(rule)
}

async fn rule_out(udb: &UserDb, id: Id) -> ApiResult<RuleOut> {
    Ok(udb
        .rule(id)
        .await?
        .ok_or_else(|| ApiError::not_found("Ese filtro no existe."))?
        .into())
}

#[utoipa::path(get, path = "/api/v1/rules", tag = "organize", responses((status = 200, body = [RuleOut])))]
pub async fn rules(State(st): State<Arc<AppState>>, cu: CurrentUser) -> ApiResult<Json<Vec<RuleOut>>> {
    let udb = st.user_db(cu.user.id).await?;
    Ok(Json(udb.rules().await?.into_iter().map(Into::into).collect()))
}

/// Crea un filtro. Se aplica al correo que llegue a partir de ahora.
#[utoipa::path(post, path = "/api/v1/rules", tag = "organize", request_body = RuleData,
    responses((status = 200, body = RuleOut), (status = 400, body = ErrorBody)))]
pub async fn create_rule(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<RuleData>,
) -> ApiResult<Json<RuleOut>> {
    let udb = st.user_db(cu.user.id).await?;
    let rule = rule_from(&udb, None, req).await?;
    let id = udb.save_rule(&rule).await?;
    Ok(Json(rule_out(&udb, id).await?))
}

#[utoipa::path(put, path = "/api/v1/rules/{id}", tag = "organize", request_body = RuleData,
    params(("id" = String, Path, description = "Id del filtro")),
    responses((status = 200, body = RuleOut), (status = 404, body = ErrorBody)))]
pub async fn update_rule(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<RuleData>,
) -> ApiResult<Json<RuleOut>> {
    let id = parse_id(&id, "Ese filtro")?;
    let udb = st.user_db(cu.user.id).await?;
    if udb.rule(id).await?.is_none() {
        return Err(ApiError::not_found("Ese filtro no existe."));
    }
    let rule = rule_from(&udb, Some(id), req).await?;
    udb.save_rule(&rule).await?;
    Ok(Json(rule_out(&udb, id).await?))
}

#[utoipa::path(delete, path = "/api/v1/rules/{id}", tag = "organize",
    params(("id" = String, Path, description = "Id del filtro")),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn delete_rule(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let id = parse_id(&id, "Ese filtro")?;
    let udb = st.user_db(cu.user.id).await?;
    if !udb.delete_rule(id).await? {
        return Err(ApiError::not_found("Ese filtro no existe."));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Contactos ----------

#[derive(Deserialize, IntoParams)]
pub struct BookParams {
    /// Texto a buscar en el nombre o la dirección.
    pub q: Option<String>,
}

fn entry_from(id: Option<Id>, r: BookEntryInput) -> ApiResult<BookEntry> {
    let email = r.email.trim().to_string();
    let valid = match email.split_once('@') {
        Some((l, d)) => !l.is_empty() && d.contains('.') && !email.contains(char::is_whitespace),
        None => false,
    };
    if !valid {
        return Err(ApiError::bad_request("La dirección de correo no es válida."));
    }
    Ok(BookEntry {
        id,
        name: r.name,
        email,
        phone: r.phone,
        notes: r.notes,
        source: String::new(),
    })
}

fn book_conflict(e: buzon_store::Error) -> ApiError {
    match e {
        buzon_store::Error::Duplicate => ApiError::conflict("duplicate", "Ya tienes un contacto con esa dirección."),
        e => e.into(),
    }
}

async fn book_out(udb: &UserDb, id: Id) -> ApiResult<BookEntryOut> {
    Ok(udb
        .book_entry(id)
        .await?
        .ok_or_else(|| ApiError::not_found("Ese contacto no existe."))?
        .into())
}

/// La libreta: contactos guardados y gente a la que has escrito.
#[utoipa::path(get, path = "/api/v1/contacts", tag = "organize", params(BookParams),
    responses((status = 200, body = [BookEntryOut])))]
pub async fn book(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Query(p): Query<BookParams>,
) -> ApiResult<Json<Vec<BookEntryOut>>> {
    let udb = st.user_db(cu.user.id).await?;
    let q = p.q.unwrap_or_default();
    Ok(Json(udb.book(q.trim()).await?.into_iter().map(Into::into).collect()))
}

#[utoipa::path(post, path = "/api/v1/contacts", tag = "organize", request_body = BookEntryInput,
    responses((status = 200, body = BookEntryOut), (status = 409, body = ErrorBody)))]
pub async fn create_entry(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<BookEntryInput>,
) -> ApiResult<Json<BookEntryOut>> {
    let udb = st.user_db(cu.user.id).await?;
    let id = udb
        .save_book_entry(&entry_from(None, req)?)
        .await
        .map_err(book_conflict)?;
    Ok(Json(book_out(&udb, id).await?))
}

#[utoipa::path(put, path = "/api/v1/contacts/{id}", tag = "organize", request_body = BookEntryInput,
    params(("id" = String, Path, description = "Id del contacto")),
    responses((status = 200, body = BookEntryOut), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)))]
pub async fn update_entry(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<BookEntryInput>,
) -> ApiResult<Json<BookEntryOut>> {
    let id = parse_id(&id, "Ese contacto")?;
    let udb = st.user_db(cu.user.id).await?;
    udb.save_book_entry(&entry_from(Some(id), req)?)
        .await
        .map_err(book_conflict)?;
    Ok(Json(book_out(&udb, id).await?))
}

#[utoipa::path(delete, path = "/api/v1/contacts/{id}", tag = "organize",
    params(("id" = String, Path, description = "Id del contacto")),
    responses((status = 204), (status = 404, body = ErrorBody)))]
pub async fn delete_entry(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let id = parse_id(&id, "Ese contacto")?;
    let udb = st.user_db(cu.user.id).await?;
    if !udb.delete_book_entry(id).await? {
        return Err(ApiError::not_found("Ese contacto no existe."));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Importa contactos de un fichero vCard (`.vcf`, versiones 2.1, 3.0 y 4.0).
#[utoipa::path(post, path = "/api/v1/contacts/import", tag = "organize", request_body = VcardImport,
    responses((status = 200, body = ImportResult), (status = 400, body = ErrorBody)))]
pub async fn import_vcard(
    State(st): State<Arc<AppState>>,
    cu: CurrentUser,
    Json(req): Json<VcardImport>,
) -> ApiResult<Json<ImportResult>> {
    if !req.vcard.to_ascii_uppercase().contains("BEGIN:VCARD") {
        return Err(ApiError::bad_request("Eso no parece un fichero vCard (.vcf)."));
    }
    let udb = st.user_db(cu.user.id).await?;
    let (added, existing) = udb.import_vcards(&req.vcard).await?;
    Ok(Json(ImportResult { added, existing }))
}

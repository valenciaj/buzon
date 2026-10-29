//! Tipos de entrada y salida de la API (con su esquema OpenAPI).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use buzon_store::app::User;
use buzon_store::Id;

#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// `focus` es el modo Enfoque; `classic`, el modo Clásico.
#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Focus,
    Classic,
}

/// Todas las cuentas combinadas o una cuenta cada vez.
#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccountView {
    #[default]
    Unified,
    PerAccount,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Font {
    #[default]
    Atkinson,
    Inter,
    SourceSans,
    Plex,
    Lexend,
    System,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    #[default]
    Comfortable,
    Compact,
}

/// Preferencias de interfaz de un usuario. Se guardan en su propia base de datos.
#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, PartialEq)]
pub struct Preferences {
    pub theme: Theme,
    pub mode: Mode,
    pub view: AccountView,
    pub font: Font,
    /// Tamaño del texto en %, de 87.5 a 150 en pasos de 12.5.
    pub font_size: f64,
    pub density: Density,
    /// Segundos para «deshacer envío»: 0 (sin margen), 5, 10, 20 o 30.
    pub undo_send: u32,
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            theme: Theme::default(),
            mode: Mode::default(),
            view: AccountView::default(),
            font: Font::default(),
            font_size: 100.0,
            density: Density::default(),
            undo_send: 10,
        }
    }
}

/// Cambio parcial de preferencias: solo se aplican los campos presentes.
#[derive(Deserialize, ToSchema, Default, Debug)]
pub struct PreferencesUpdate {
    pub theme: Option<Theme>,
    pub mode: Option<Mode>,
    pub view: Option<AccountView>,
    pub font: Option<Font>,
    pub font_size: Option<f64>,
    pub density: Option<Density>,
    pub undo_send: Option<u32>,
    /// Idioma de la interfaz: `es` o `en`. Se guarda en el usuario.
    pub language: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct Me {
    #[schema(value_type = String, example = "1C4xWbQq9k3vT7hF2mYp8z")]
    pub id: Id,
    pub username: String,
    pub email: Option<String>,
    pub is_admin: bool,
    pub totp_enabled: bool,
    pub language: String,
    pub preferences: Preferences,
}

impl Me {
    pub fn new(u: &User, preferences: Preferences) -> Self {
        Me {
            id: u.id,
            username: u.username.clone(),
            email: u.email.clone(),
            is_admin: u.is_admin(),
            totp_enabled: u.totp_enabled,
            language: u.language.clone(),
            preferences,
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    /// Código de 6 cifras de la app de autenticación, si el usuario tiene 2FA.
    pub totp: Option<String>,
}

/// Datos de una cuenta de correo. Este usuario y contraseña son los del
/// servidor de correo, no los de Buzón.
#[derive(Deserialize, ToSchema)]
pub struct AccountInput {
    pub name: String,
    pub email: String,
    pub imap_host: String,
    #[schema(example = 993)]
    pub imap_port: u16,
    pub smtp_host: String,
    #[schema(example = 465)]
    pub smtp_port: u16,
    pub login: String,
    pub password: String,
    /// Cifrado IMAP: `tls` (por defecto, puerto 993), `starttls` (143) o `none`.
    #[serde(default = "tls")]
    pub imap_security: String,
    /// Cifrado SMTP: `tls` (por defecto, puerto 465), `starttls` (587) o `none`.
    #[serde(default = "tls")]
    pub smtp_security: String,
    /// Comprobar antes de guardar que el servidor IMAP acepta el usuario y la contraseña.
    #[serde(default = "yes")]
    pub verify: bool,
}

fn tls() -> String {
    "tls".into()
}

fn yes() -> bool {
    true
}

#[derive(Serialize, ToSchema)]
pub struct AccountOut {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub login: String,
    pub imap_security: String,
    pub smtp_security: String,
    /// Cómo se entra: `password`, o con OAuth2 `google` o `microsoft`.
    pub auth: String,
    /// Estado de la sincronización: `off`, `connecting`, `syncing`, `idle` o `error`.
    pub state: String,
    /// Última sincronización completa (segundos Unix).
    pub synced_at: Option<i64>,
    /// Último error al sincronizar, si lo hubo.
    pub sync_error: Option<String>,
}

impl AccountOut {
    pub fn new(a: buzon_store::user::Account, state: buzon_sync::State) -> Self {
        let state = serde_json::to_value(state)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        AccountOut {
            id: a.id,
            name: a.name,
            email: a.email,
            imap_host: a.imap_host,
            imap_port: a.imap_port,
            smtp_host: a.smtp_host,
            smtp_port: a.smtp_port,
            login: a.login,
            imap_security: a.imap_security,
            smtp_security: a.smtp_security,
            auth: a.auth,
            state,
            synced_at: a.synced_at,
            sync_error: a.sync_error,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct AddrOut {
    pub name: Option<String>,
    pub email: String,
}

impl From<buzon_store::mail::Addr> for AddrOut {
    fn from(a: buzon_store::mail::Addr) -> Self {
        AddrOut {
            name: a.name,
            email: a.email,
        }
    }
}

/// Carpeta IMAP de una cuenta.
#[derive(Serialize, ToSchema)]
pub struct MailboxOut {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub account_id: Id,
    /// Nombre completo y legible, por ejemplo `Proyectos/2026`.
    pub name: String,
    /// Separador de niveles (`/` o `.`).
    pub delimiter: Option<String>,
    /// `inbox`, `sent`, `drafts`, `archive`, `junk`, `trash` o nada.
    pub role: Option<String>,
    pub total: i64,
    pub unread: i64,
}

/// Resumen de un hilo para la lista.
#[derive(Serialize, ToSchema)]
pub struct ThreadOut {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub account_id: Id,
    pub subject: String,
    /// Fecha del último mensaje (segundos Unix).
    pub last_date: i64,
    /// Remitentes, del más antiguo al más reciente.
    pub participants: Vec<String>,
    pub snippet: String,
    pub messages: i64,
    pub unread: i64,
    pub flagged: bool,
    pub has_attachments: bool,
    /// Ids de sus etiquetas.
    #[schema(value_type = Vec<String>)]
    pub labels: Vec<Id>,
    /// En la pila «Responder luego».
    pub reply_later: bool,
    /// En la pila «Apartados».
    pub aside: bool,
    /// Pospuesto hasta esta fecha (segundos Unix). Si ya pasó, ha reaparecido.
    pub snooze_until: Option<i64>,
    /// Tiene nota privada.
    pub has_note: bool,
    /// Algún mensaje traía píxeles espía (ya quitados).
    pub tracked: bool,
}

impl From<buzon_store::mail::ThreadSummary> for ThreadOut {
    fn from(t: buzon_store::mail::ThreadSummary) -> Self {
        ThreadOut {
            id: t.id,
            account_id: t.account_id,
            subject: t.subject,
            last_date: t.last_date,
            participants: t.participants,
            snippet: t.snippet,
            messages: t.messages,
            unread: t.unread,
            flagged: t.flagged,
            has_attachments: t.has_attachments,
            labels: t.labels,
            reply_later: t.flow.reply_later,
            aside: t.flow.aside,
            snooze_until: t.flow.snooze_until,
            has_note: t.flow.has_note,
            tracked: t.flow.tracked,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct ThreadPage {
    pub threads: Vec<ThreadOut>,
    /// Pásalo como `cursor` para la página siguiente. Falta si no hay más.
    pub next_cursor: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct AttachmentOut {
    pub index: u32,
    pub name: String,
    pub content_type: String,
    pub size: u64,
    /// Imagen incrustada en el HTML del mensaje.
    pub inline: bool,
    pub url: String,
}

#[derive(Serialize, ToSchema)]
pub struct MessageOut {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub mailbox_id: Id,
    pub subject: String,
    pub from: Option<AddrOut>,
    pub to: Vec<AddrOut>,
    pub cc: Vec<AddrOut>,
    pub date: i64,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub text: Option<String>,
    /// HTML ya saneado. Las imágenes remotas siguen en él: muéstralo con una CSP que
    /// las bloquee hasta que la persona las pida.
    pub html: Option<String>,
    pub remote_images: u32,
    /// Dominios de los píxeles espía que se han quitado.
    pub trackers: Vec<String>,
    pub attachments: Vec<AttachmentOut>,
}

#[derive(Serialize, ToSchema)]
pub struct ThreadDetail {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub account_id: Id,
    pub subject: String,
    #[schema(value_type = Vec<String>)]
    pub labels: Vec<Id>,
    pub messages: Vec<MessageOut>,
    /// Nota privada (solo la ves tú).
    pub note: String,
    pub reply_later: bool,
    pub aside: bool,
    pub snooze_until: Option<i64>,
    /// Quien escribe el hilo y lo decidido sobre él (modo Enfoque).
    pub sender: Option<SenderOut>,
}

// ---------- Flujo (modo Enfoque) ----------

/// Remitente y lo decidido sobre él.
#[derive(Serialize, ToSchema)]
pub struct SenderOut {
    pub email: String,
    pub name: String,
    /// `pending` (por aprobar), `approved` o `blocked`.
    pub status: String,
    /// Dónde va su correo: `principal`, `news` (Novedades) o `receipts` (Recibos).
    pub bucket: String,
}

impl From<buzon_store::flow::Sender> for SenderOut {
    fn from(s: buzon_store::flow::Sender) -> Self {
        SenderOut {
            email: s.email,
            name: s.name,
            status: s.status,
            bucket: s.bucket,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct SenderSummaryOut {
    #[serde(flatten)]
    pub sender: SenderOut,
    /// Hilos suyos en la entrada.
    pub threads: i64,
    /// Su hilo más reciente.
    #[schema(value_type = Option<String>)]
    pub last_thread: Option<Id>,
    pub last_subject: String,
    pub last_date: i64,
}

impl From<buzon_store::flow::SenderSummary> for SenderSummaryOut {
    fn from(s: buzon_store::flow::SenderSummary) -> Self {
        SenderSummaryOut {
            sender: s.sender.into(),
            threads: s.threads,
            last_thread: s.last_thread,
            last_subject: s.last_subject,
            last_date: s.last_date,
        }
    }
}

/// Aprobar, rechazar o cambiar de bandeja a un remitente.
#[derive(Deserialize, ToSchema)]
pub struct SenderDecision {
    pub email: String,
    /// `approved`, `blocked` o `pending`.
    pub status: String,
    /// `principal`, `news` o `receipts`. Sin él, la que tenía (o `principal`).
    pub bucket: Option<String>,
}

/// Meter o sacar hilos de las pilas y posponerlos. Lo que falta no cambia.
#[derive(Deserialize, ToSchema)]
pub struct ThreadsFlow {
    #[schema(value_type = Vec<String>)]
    pub threads: Vec<Id>,
    pub reply_later: Option<bool>,
    pub aside: Option<bool>,
    /// Posponer hasta esta fecha (segundos Unix). `0` lo quita.
    pub snooze_until: Option<i64>,
}

#[derive(Deserialize, ToSchema)]
pub struct NoteInput {
    pub note: String,
}

/// Cuántos hilos hay en cada sitio del modo Enfoque.
#[derive(Serialize, ToSchema)]
pub struct FlowCountsOut {
    pub screener: i64,
    pub reply_later: i64,
    pub aside: i64,
    pub snoozed: i64,
    /// Pospuestos que ya han reaparecido.
    pub bubbled: i64,
    pub principal_unread: i64,
    pub news_unread: i64,
    pub receipts_unread: i64,
}

impl From<buzon_store::flow::FlowCounts> for FlowCountsOut {
    fn from(c: buzon_store::flow::FlowCounts) -> Self {
        FlowCountsOut {
            screener: c.screener,
            reply_later: c.reply_later,
            aside: c.aside,
            snoozed: c.snoozed,
            bubbled: c.bubbled,
            principal_unread: c.principal_unread,
            news_unread: c.news_unread,
            receipts_unread: c.receipts_unread,
        }
    }
}

/// Cambios en un hilo. `seen` afecta a todos sus mensajes; `flagged: true` destaca el último.
#[derive(Deserialize, ToSchema)]
pub struct ThreadUpdate {
    pub seen: Option<bool>,
    pub flagged: Option<bool>,
}

/// Remitente de una cuenta (su dirección o un alias) con su firma.
#[derive(Serialize, ToSchema)]
pub struct IdentityOut {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub account_id: Id,
    /// Nombre que verán los destinatarios. Vacío: solo la dirección.
    pub name: String,
    pub email: String,
    /// Firma en texto plano; se añade al final de los correos nuevos.
    pub signature: String,
    pub is_default: bool,
}

impl From<buzon_store::compose::Identity> for IdentityOut {
    fn from(i: buzon_store::compose::Identity) -> Self {
        IdentityOut {
            id: i.id,
            account_id: i.account_id,
            name: i.name,
            email: i.email,
            signature: i.signature,
            is_default: i.is_default,
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct IdentityInput {
    /// Cuenta a la que pertenece (solo al crearla).
    #[schema(value_type = Option<String>)]
    pub account_id: Option<Id>,
    #[serde(default)]
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Serialize, Deserialize, ToSchema, Clone)]
pub struct AddrIn {
    pub name: Option<String>,
    pub email: String,
}

impl From<AddrIn> for buzon_store::mail::Addr {
    fn from(a: AddrIn) -> Self {
        buzon_store::mail::Addr {
            name: a.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()),
            email: a.email.trim().to_string(),
        }
    }
}

impl From<buzon_store::mail::Addr> for AddrIn {
    fn from(a: buzon_store::mail::Addr) -> Self {
        AddrIn {
            name: a.name,
            email: a.email,
        }
    }
}

/// Un correo a medias. Lo mismo sirve para guardarlo como borrador o para enviarlo.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct DraftIn {
    #[schema(value_type = Option<String>)]
    pub identity_id: Option<Id>,
    #[serde(default)]
    pub to: Vec<AddrIn>,
    #[serde(default)]
    pub cc: Vec<AddrIn>,
    #[serde(default)]
    pub bcc: Vec<AddrIn>,
    #[serde(default)]
    pub subject: String,
    /// Cuerpo en HTML (se sanea antes de enviarlo).
    #[serde(default)]
    pub html: String,
    /// Cuerpo en texto plano.
    #[serde(default)]
    pub text: String,
    /// Mensaje al que se responde.
    #[schema(value_type = Option<String>)]
    pub reply_to: Option<Id>,
    /// Mensaje que se reenvía (sus adjuntos van incluidos).
    #[schema(value_type = Option<String>)]
    pub forward_of: Option<Id>,
    /// Ids de ficheros subidos con `POST /uploads`.
    #[serde(default)]
    #[schema(value_type = Vec<String>)]
    pub attachments: Vec<Id>,
}

#[derive(Serialize, ToSchema)]
pub struct UploadOut {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    pub content_type: String,
    pub size: u64,
}

impl From<buzon_store::compose::Upload> for UploadOut {
    fn from(u: buzon_store::compose::Upload) -> Self {
        UploadOut {
            id: u.id,
            name: u.name,
            content_type: u.content_type,
            size: u.size,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct DraftOut {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = Option<String>)]
    pub identity_id: Option<Id>,
    pub to: Vec<AddrIn>,
    pub cc: Vec<AddrIn>,
    pub bcc: Vec<AddrIn>,
    pub subject: String,
    pub html: String,
    pub text: String,
    #[schema(value_type = Option<String>)]
    pub reply_to: Option<Id>,
    #[schema(value_type = Option<String>)]
    pub forward_of: Option<Id>,
    pub attachments: Vec<UploadOut>,
    pub updated_at: i64,
    /// Cuándo sale, si está programado (negativo: se está enviando ahora).
    pub send_at: Option<i64>,
    /// Por qué falló el último envío programado.
    pub send_error: Option<String>,
    pub follow_up_days: Option<i64>,
}

#[derive(Deserialize, ToSchema)]
pub struct SendRequest {
    #[serde(flatten)]
    pub message: DraftIn,
    /// Borrador que se envía: se borra al terminar.
    #[schema(value_type = Option<String>)]
    pub draft_id: Option<Id>,
    /// Programa el envío para esta fecha (segundos Unix). La web lo usa también para
    /// «deshacer envío»: sale unos segundos después y se puede cancelar.
    pub send_at: Option<i64>,
    /// Avisar si nadie responde en estos días (1 a 30).
    pub follow_up_days: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct SendResult {
    /// `Message-ID` del correo enviado (si ha salido ya).
    pub message_id: Option<String>,
    /// Si se ha programado: el borrador que saldrá y cuándo.
    #[schema(value_type = Option<String>)]
    pub scheduled_id: Option<Id>,
    pub send_at: Option<i64>,
}

/// Correo enviado que espera respuesta.
#[derive(Serialize, ToSchema)]
pub struct FollowUpOut {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub account_id: Id,
    pub subject: String,
    pub to: Vec<AddrIn>,
    pub sent_at: i64,
    /// Desde cuándo cuenta como «sin respuesta».
    pub due_at: i64,
    /// Hilo del correo enviado (cuando ya se ha sincronizado la copia).
    #[schema(value_type = Option<String>)]
    pub thread: Option<Id>,
}

impl From<buzon_store::extras::FollowUp> for FollowUpOut {
    fn from(f: buzon_store::extras::FollowUp) -> Self {
        FollowUpOut {
            id: f.id,
            account_id: f.account_id,
            subject: f.subject,
            to: f.to.into_iter().map(Into::into).collect(),
            sent_at: f.sent_at,
            due_at: f.due_at,
            thread: f.thread,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct ContactOut {
    pub name: Option<String>,
    pub email: String,
}

// ---------- Organizar ----------

/// Varios hilos a la vez. `from_role` o `from_mailbox` limitan la acción a los mensajes
/// de esa carpeta (la que se está viendo); sin ellos, a todos menos Enviados y Borradores.
#[derive(Deserialize, ToSchema)]
pub struct ThreadsMove {
    #[schema(value_type = Vec<String>)]
    pub threads: Vec<Id>,
    pub from_role: Option<String>,
    #[schema(value_type = Option<String>)]
    pub from_mailbox: Option<Id>,
    /// `inbox`, `archive`, `trash` o `junk`.
    pub to_role: Option<String>,
    #[schema(value_type = Option<String>)]
    pub to_mailbox: Option<Id>,
}

#[derive(Serialize, ToSchema)]
pub struct MoveResult {
    /// Mensajes movidos.
    pub moved: usize,
}

/// Borra para siempre lo que esos hilos tienen en la papelera o en spam.
#[derive(Deserialize, ToSchema)]
pub struct ThreadsDelete {
    #[schema(value_type = Vec<String>)]
    pub threads: Vec<Id>,
    pub from_role: Option<String>,
    #[schema(value_type = Option<String>)]
    pub from_mailbox: Option<Id>,
}

#[derive(Serialize, ToSchema)]
pub struct DeleteResult {
    pub deleted: usize,
}

#[derive(Deserialize, ToSchema)]
pub struct ThreadsFlags {
    #[schema(value_type = Vec<String>)]
    pub threads: Vec<Id>,
    pub seen: Option<bool>,
    pub flagged: Option<bool>,
}

#[derive(Deserialize, ToSchema)]
pub struct ThreadsLabels {
    #[schema(value_type = Vec<String>)]
    pub threads: Vec<Id>,
    #[serde(default)]
    #[schema(value_type = Vec<String>)]
    pub add: Vec<Id>,
    #[serde(default)]
    #[schema(value_type = Vec<String>)]
    pub remove: Vec<Id>,
}

#[derive(Serialize, ToSchema)]
pub struct LabelOut {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    /// Color en formato `#rrggbb`.
    pub color: String,
    pub threads: i64,
    pub unread: i64,
}

impl From<buzon_store::organize::Label> for LabelOut {
    fn from(l: buzon_store::organize::Label) -> Self {
        LabelOut {
            id: l.id,
            name: l.name,
            color: l.color,
            threads: l.threads,
            unread: l.unread,
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct LabelInput {
    pub name: String,
    pub color: String,
}

/// Filtro automático para el correo nuevo de la bandeja de entrada. Las condiciones
/// vacías no cuentan; las demás deben cumplirse todas (sin distinguir mayúsculas).
#[derive(Serialize, Deserialize, ToSchema)]
pub struct RuleData {
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Solo el correo de esta cuenta. Sin ella, el de todas.
    #[schema(value_type = Option<String>)]
    pub account_id: Option<Id>,
    /// El remitente (nombre o dirección) contiene…
    #[serde(default)]
    pub from_has: String,
    /// Algún destinatario (Para o Cc) contiene…
    #[serde(default)]
    pub to_has: String,
    #[serde(default)]
    pub subject_has: String,
    #[serde(default)]
    pub body_has: String,
    /// Etiqueta que se pone.
    #[schema(value_type = Option<String>)]
    pub label_id: Option<Id>,
    #[serde(default)]
    pub mark_read: bool,
    #[serde(default)]
    pub star: bool,
    /// `archive`, `trash`, `junk` o vacío (se queda en la entrada).
    #[serde(default)]
    pub move_to: String,
}

#[derive(Serialize, ToSchema)]
pub struct RuleOut {
    #[schema(value_type = String)]
    pub id: Id,
    #[serde(flatten)]
    pub rule: RuleData,
}

impl From<buzon_store::organize::Rule> for RuleOut {
    fn from(r: buzon_store::organize::Rule) -> Self {
        RuleOut {
            id: r.id.expect("guardado"),
            rule: RuleData {
                name: r.name,
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
            },
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct BookEntryOut {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    pub email: String,
    pub phone: String,
    pub notes: String,
    /// `manual`, `sent` (alguien a quien escribiste) o `vcard`.
    pub source: String,
}

impl From<buzon_store::organize::BookEntry> for BookEntryOut {
    fn from(e: buzon_store::organize::BookEntry) -> Self {
        BookEntryOut {
            id: e.id.expect("guardado"),
            name: e.name,
            email: e.email,
            phone: e.phone,
            notes: e.notes,
            source: e.source,
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct BookEntryInput {
    #[serde(default)]
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Deserialize, ToSchema)]
pub struct VcardImport {
    /// Contenido del fichero `.vcf`.
    pub vcard: String,
}

#[derive(Serialize, ToSchema)]
pub struct ImportResult {
    pub added: usize,
    /// Direcciones que ya estaban (se completan sin pisar lo que tenían).
    pub existing: usize,
}

// ---------- Tokens y webhooks ----------

/// Token personal de la API. El token completo solo se muestra al crearlo.
#[derive(Serialize, ToSchema)]
pub struct TokenOut {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    /// Primeros caracteres del token, para reconocerlo.
    pub prefix: String,
    pub scopes: Vec<String>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
}

impl From<buzon_store::tokens::ApiToken> for TokenOut {
    fn from(t: buzon_store::tokens::ApiToken) -> Self {
        TokenOut {
            id: t.id,
            name: t.name,
            prefix: t.prefix,
            scopes: t.scopes,
            created_at: t.created_at,
            expires_at: t.expires_at,
            last_used_at: t.last_used_at,
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct TokenInput {
    pub name: String,
    /// `mail:read`, `mail:write`, `mail:send`, `contacts` o `webhooks`.
    pub scopes: Vec<String>,
    /// Días hasta que caduca (1 a 365). Sin él, no caduca.
    pub expires_days: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct TokenCreated {
    #[serde(flatten)]
    pub info: TokenOut,
    /// El token. Guárdalo ahora: no se vuelve a mostrar.
    pub token: String,
}

#[derive(Serialize, ToSchema)]
pub struct WebhookOut {
    #[schema(value_type = String)]
    pub id: Id,
    pub url: String,
    pub description: String,
    pub events: Vec<String>,
    pub active: bool,
    pub created_at: i64,
    /// Cómo acabó el último envío: `ok`, `failed` o nada si aún no hay.
    pub last_status: Option<String>,
}

impl From<buzon_store::hooks::Webhook> for WebhookOut {
    fn from(w: buzon_store::hooks::Webhook) -> Self {
        WebhookOut {
            id: w.id,
            url: w.url,
            description: w.description,
            events: w.events,
            active: w.active,
            created_at: w.created_at,
            last_status: w.last_status,
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct WebhookInput {
    /// `https://…`. Sin HTTPS o a la red local solo si el servidor lo permite.
    pub url: String,
    #[serde(default)]
    pub description: String,
    pub events: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct WebhookUpdate {
    pub url: Option<String>,
    pub description: Option<String>,
    pub events: Option<Vec<String>>,
    pub active: Option<bool>,
}

#[derive(Serialize, ToSchema)]
pub struct WebhookCreated {
    #[serde(flatten)]
    pub info: WebhookOut,
    /// Secreto para comprobar la firma. Guárdalo ahora: no se vuelve a mostrar.
    pub secret: String,
}

#[derive(Serialize, ToSchema)]
pub struct DeliveryOut {
    #[schema(value_type = String)]
    pub id: Id,
    pub event: String,
    /// `pending` (por enviar o reintentar), `ok` o `failed` (se agotaron los reintentos).
    pub status: String,
    pub attempts: i64,
    /// Cuándo es el próximo intento.
    pub next_at: Option<i64>,
    /// Código HTTP de la última respuesta.
    pub response: Option<i64>,
    pub error: Option<String>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    /// El JSON enviado.
    pub payload: String,
}

impl From<buzon_store::hooks::Delivery> for DeliveryOut {
    fn from(d: buzon_store::hooks::Delivery) -> Self {
        DeliveryOut {
            id: d.id,
            event: d.event,
            status: d.status,
            attempts: d.attempts,
            next_at: d.next_at,
            response: d.response,
            error: d.error,
            created_at: d.created_at,
            finished_at: d.finished_at,
            payload: d.payload,
        }
    }
}

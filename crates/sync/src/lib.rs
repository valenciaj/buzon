//! Sincronización IMAP de Buzón: un worker por cuenta que descarga el correo a la
//! base de datos del usuario y escucha con IDLE para enterarse al momento de lo nuevo.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use buzon_store::crypto::SecretBox;
use buzon_store::mail::ImapTarget;
use buzon_store::organize::{ExpungeOp, MoveOp};
use buzon_store::user::{user_raw_dir, Account};
use buzon_store::{Id, UserDb};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};
use tokio::time::{Duration, Instant};

pub mod conn;
pub mod oauth;
pub mod parse;
pub mod send;
mod worker;

pub use conn::{check, Auth, Mech, Security, ServerConfig};
pub use oauth::{OAuthApp, OAuthError, Provider};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no se pudo conectar con el servidor: {0}")]
    Io(#[from] std::io::Error),
    #[error("el servidor IMAP respondió con un error: {0}")]
    Imap(#[from] async_imap::error::Error),
    #[error("el servidor rechazó el usuario o la contraseña: {0}")]
    Login(String),
    #[error("el servidor no responde")]
    Timeout,
    #[error("el servidor cerró la conexión")]
    Closed,
    #[error("configuración no válida: {0}")]
    Config(&'static str),
    #[error("{0}")]
    Store(#[from] buzon_store::Error),
    #[error("{0}")]
    OAuth(#[from] OAuthError),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Estado de la sincronización de una cuenta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Off,
    Connecting,
    Syncing,
    Idle,
    Error,
}

/// Aviso para la web: algo cambió en el correo o en el estado de una cuenta.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    #[serde(skip)]
    pub user: Id,
    /// `mail` (hay cambios en el correo) o `status` (cambió el estado de la cuenta).
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub account: Id,
}

/// Algo del correo que puede interesar a un webhook.
#[derive(Debug, Clone)]
pub enum HookEvent {
    /// Llegó un mensaje nuevo a la bandeja de entrada (no cuenta la primera descarga).
    Received { user: Id, account: Id, message: Id },
    /// La cuenta dejó de sincronizar.
    SyncFailed { user: Id, account: Id, error: String },
}

pub(crate) enum Cmd {
    SyncNow,
    Store {
        mailbox: String,
        uids: Vec<u32>,
        flag: &'static str,
        on: bool,
    },
    /// Guarda un mensaje en una carpeta (por ejemplo, la copia en Enviados).
    Append {
        mailbox: String,
        raw: Vec<u8>,
    },
    /// Cambia (o borra, sin `raw`) la copia de un borrador en la carpeta Borradores.
    PutDraft {
        mailbox: String,
        key: String,
        raw: Option<Vec<u8>>,
    },
    Move {
        from: String,
        to: String,
        uids: Vec<u32>,
    },
    /// Borra mensajes para siempre.
    Expunge {
        mailbox: String,
        uids: Vec<u32>,
    },
    Stop,
}

/// Con qué se entra en el correo de una cuenta.
#[derive(Clone)]
pub enum Creds {
    Password(String),
    /// El secreto guardado de la cuenta es el token de renovación de OAuth2.
    OAuth {
        app: Arc<OAuthApp>,
        secrets: Arc<SecretBox>,
    },
}

pub(crate) struct Shared {
    pub events: broadcast::Sender<Event>,
    pub hooks: broadcast::Sender<HookEvent>,
    pub status: Mutex<HashMap<Id, State>>,
    /// Tokens de acceso de OAuth2 en vigor y cuándo caducan.
    tokens: Mutex<HashMap<Id, (String, Instant)>>,
    /// Una renovación cada vez, para no pedir dos tokens a la vez.
    refreshing: tokio::sync::Mutex<()>,
}

impl Shared {
    /// Credenciales para conectar ahora: con OAuth2, renueva el token de acceso si caduca pronto.
    pub async fn auth(&self, db: &UserDb, account: Id, creds: &Creds) -> Result<Auth> {
        let (app, secrets) = match creds {
            Creds::Password(p) => return Ok(Auth::Password(p.clone())),
            Creds::OAuth { app, secrets } => (app, secrets),
        };
        let cached = |tokens: &Mutex<HashMap<Id, (String, Instant)>>| {
            tokens
                .lock()
                .expect("mutex")
                .get(&account)
                .filter(|(_, until)| *until > Instant::now() + Duration::from_secs(120))
                .map(|(t, _)| Auth::XOAuth2(t.clone()))
        };
        if let Some(a) = cached(&self.tokens) {
            return Ok(a);
        }
        let _guard = self.refreshing.lock().await;
        if let Some(a) = cached(&self.tokens) {
            return Ok(a);
        }
        let refresh = db
            .account_password(account, secrets)
            .await?
            .ok_or(Error::Config("la cuenta no existe"))?;
        let t = app.refresh(&refresh).await?;
        // Microsoft cambia el token de renovación en cada uso: hay que guardar el nuevo.
        if let Some(new) = t.refresh_token.as_deref().filter(|n| *n != refresh) {
            db.set_account_secret(account, new, secrets).await?;
        }
        let until = Instant::now() + Duration::from_secs(t.expires_in);
        self.tokens
            .lock()
            .expect("mutex")
            .insert(account, (t.access_token.clone(), until));
        Ok(Auth::XOAuth2(t.access_token))
    }

    pub fn set_state(&self, user: Id, account: Id, state: State) {
        let prev = self.status.lock().expect("mutex").insert(account, state);
        if prev != Some(state) {
            self.emit(user, "status", account);
        }
    }

    pub fn emit(&self, user: Id, kind: &'static str, account: Id) {
        let _ = self.events.send(Event { user, kind, account });
    }
}

struct Worker {
    tx: mpsc::UnboundedSender<Cmd>,
}

/// Coordina los workers de todas las cuentas y reparte sus avisos.
pub struct SyncHub {
    data_dir: PathBuf,
    enabled: bool,
    workers: Mutex<HashMap<Id, Worker>>,
    shared: Arc<Shared>,
}

impl SyncHub {
    /// `enabled = false` no conecta con ningún servidor (útil en pruebas).
    pub fn new(data_dir: PathBuf, enabled: bool) -> Self {
        SyncHub {
            data_dir,
            enabled,
            workers: Mutex::new(HashMap::new()),
            shared: Arc::new(Shared {
                events: broadcast::channel(256).0,
                hooks: broadcast::channel(1024).0,
                status: Mutex::new(HashMap::new()),
                tokens: Mutex::new(HashMap::new()),
                refreshing: tokio::sync::Mutex::new(()),
            }),
        }
    }

    /// `false` en las pruebas: no se conecta con ningún servidor.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Arranca (o reinicia) la sincronización de una cuenta.
    pub fn start(&self, user: Id, db: UserDb, account: &Account, creds: Creds) {
        if !self.enabled {
            return;
        }
        let Some(security) = Security::parse(&account.imap_security) else {
            tracing::warn!("cuenta {}: seguridad IMAP desconocida", account.id);
            return;
        };
        self.stop(account.id);
        let (tx, rx) = mpsc::unbounded_channel();
        let ctx = worker::Ctx {
            user,
            account: account.id,
            db,
            cfg: ServerConfig {
                host: account.imap_host.clone(),
                port: account.imap_port,
                security,
                login: account.login.clone(),
                auth: Auth::Password(String::new()),
                mech: Mech::parse(&account.auth_mech).unwrap_or_default(),
            },
            creds,
            raw_dir: user_raw_dir(&self.data_dir, user),
            shared: self.shared.clone(),
        };
        tokio::spawn(worker::run(ctx, rx));
        self.workers.lock().expect("mutex").insert(account.id, Worker { tx });
    }

    pub fn stop(&self, account: Id) {
        if let Some(w) = self.workers.lock().expect("mutex").remove(&account) {
            let _ = w.tx.send(Cmd::Stop);
        }
        self.shared.status.lock().expect("mutex").remove(&account);
        self.shared.tokens.lock().expect("mutex").remove(&account);
    }

    /// Credenciales para conectar ahora con el servidor (IMAP o SMTP) de la cuenta.
    pub async fn auth(&self, db: &UserDb, account: Id, creds: &Creds) -> Result<Auth> {
        self.shared.auth(db, account, creds).await
    }

    pub fn sync_now(&self, account: Id) {
        self.send(account, Cmd::SyncNow);
    }

    /// Aplica en IMAP un cambio de flag (`\Seen`, `\Flagged`…) ya guardado en local.
    pub fn store_flag(&self, targets: &[ImapTarget], flag: &'static str, on: bool) {
        let mut groups: BTreeMap<(Id, &str), Vec<u32>> = BTreeMap::new();
        for t in targets.iter().filter(|t| t.uid > 0) {
            groups
                .entry((t.account_id, t.mailbox.as_str()))
                .or_default()
                .push(t.uid);
        }
        for ((account, mailbox), uids) in groups {
            self.send(
                account,
                Cmd::Store {
                    mailbox: mailbox.to_string(),
                    uids,
                    flag,
                    on,
                },
            );
        }
    }

    /// Guarda una copia (ya leída) de un mensaje en una carpeta y vuelve a sincronizar.
    pub fn append(&self, account: Id, mailbox: &str, raw: Vec<u8>) {
        self.send(
            account,
            Cmd::Append {
                mailbox: mailbox.to_string(),
                raw,
            },
        );
        self.send(account, Cmd::SyncNow);
    }

    /// Deja en la carpeta Borradores del servidor la última versión del borrador `key`
    /// (de `send::build_draft`), o la borra si `raw` es `None`.
    pub fn put_draft(&self, account: Id, mailbox: &str, key: &str, raw: Option<Vec<u8>>) {
        self.send(
            account,
            Cmd::PutDraft {
                mailbox: mailbox.to_string(),
                key: key.to_string(),
                raw,
            },
        );
    }

    /// Aplica en IMAP movimientos ya hechos en local y vuelve a sincronizar para
    /// recibir los mensajes en su nueva carpeta.
    pub fn move_messages(&self, ops: &[MoveOp]) {
        let mut accounts = Vec::new();
        for op in ops.iter().filter(|o| !o.uids.is_empty()) {
            self.send(
                op.account_id,
                Cmd::Move {
                    from: op.from.clone(),
                    to: op.to.clone(),
                    uids: op.uids.clone(),
                },
            );
            if !accounts.contains(&op.account_id) {
                accounts.push(op.account_id);
            }
        }
        for a in accounts {
            self.send(a, Cmd::SyncNow);
        }
    }

    /// Borra para siempre en IMAP mensajes ya borrados en local.
    pub fn expunge(&self, ops: &[ExpungeOp]) {
        for op in ops.iter().filter(|o| !o.uids.is_empty()) {
            self.send(
                op.account_id,
                Cmd::Expunge {
                    mailbox: op.mailbox.clone(),
                    uids: op.uids.clone(),
                },
            );
        }
    }

    fn send(&self, account: Id, cmd: Cmd) {
        if let Some(w) = self.workers.lock().expect("mutex").get(&account) {
            let _ = w.tx.send(cmd);
        }
    }

    pub fn state(&self, account: Id) -> State {
        self.shared
            .status
            .lock()
            .expect("mutex")
            .get(&account)
            .copied()
            .unwrap_or(State::Off)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.shared.events.subscribe()
    }

    /// Avisos para los webhooks.
    pub fn subscribe_hooks(&self) -> broadcast::Receiver<HookEvent> {
        self.shared.hooks.subscribe()
    }

    /// Avisa a las demás pestañas del usuario de un cambio hecho desde la API.
    pub fn emit(&self, user: Id, account: Id) {
        self.shared.emit(user, "mail", account);
    }
}

//! Sincronización IMAP de Buzón: un worker por cuenta que descarga el correo a la
//! base de datos del usuario y escucha con IDLE para enterarse al momento de lo nuevo.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use buzon_store::mail::ImapTarget;
use buzon_store::user::{user_raw_dir, Account};
use buzon_store::{Id, UserDb};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};

pub mod conn;
pub mod parse;
pub mod send;
mod worker;

pub use conn::{check, Security, ServerConfig};

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
    Stop,
}

pub(crate) struct Shared {
    pub events: broadcast::Sender<Event>,
    pub status: Mutex<HashMap<Id, State>>,
}

impl Shared {
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
                status: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Arranca (o reinicia) la sincronización de una cuenta.
    pub fn start(&self, user: Id, db: UserDb, account: &Account, password: String) {
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
                password,
            },
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
    }

    pub fn sync_now(&self, account: Id) {
        self.send(account, Cmd::SyncNow);
    }

    /// Aplica en IMAP un cambio de flag (`\Seen`, `\Flagged`…) ya guardado en local.
    pub fn store_flag(&self, targets: &[ImapTarget], flag: &'static str, on: bool) {
        let mut groups: BTreeMap<(Id, &str), Vec<u32>> = BTreeMap::new();
        for t in targets {
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

    /// Avisa a las demás pestañas del usuario de un cambio hecho desde la API.
    pub fn emit(&self, user: Id, account: Id) {
        self.shared.emit(user, "mail", account);
    }
}

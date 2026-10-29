//! Conexión IMAP con TLS directo, STARTTLS o (solo para pruebas) sin cifrar.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{self, ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

use crate::oauth::{self, XOAuth2};
use crate::{Error, Result};

pub trait Io: AsyncRead + AsyncWrite + Unpin + Send + std::fmt::Debug {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + std::fmt::Debug> Io for T {}

pub type Session = async_imap::Session<Box<dyn Io>>;

const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Tls,
    StartTls,
    None,
}

impl Security {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "tls" => Some(Security::Tls),
            "starttls" => Some(Security::StartTls),
            "none" => Some(Security::None),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub security: Security,
    pub login: String,
    pub auth: Auth,
}

/// Cómo se entra en el servidor.
#[derive(Clone)]
pub enum Auth {
    Password(String),
    /// Token de acceso de OAuth2 (Gmail, Outlook), con XOAUTH2.
    XOAuth2(String),
}

/// Sin secretos, para que no acaben en los registros.
impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Auth::Password(_) => "Password(…)",
            Auth::XOAuth2(_) => "XOAuth2(…)",
        })
    }
}

impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{} ({:?}, {})", self.host, self.port, self.security, self.login)
    }
}

fn tls() -> TlsConnector {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    let cfg = CONFIG.get_or_init(|| {
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let cfg = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("versiones TLS")
            .with_root_certificates(roots)
            .with_no_client_auth();
        Arc::new(cfg)
    });
    TlsConnector::from(cfg.clone())
}

async fn wrap_tls(host: &str, tcp: TcpStream) -> Result<Box<dyn Io>> {
    let name = ServerName::try_from(host.to_string()).map_err(|_| Error::Config("nombre de servidor no válido"))?;
    Ok(Box::new(tls().connect(name, tcp).await?))
}

async fn greeting<T: Io>(client: &mut async_imap::Client<T>) -> Result<()> {
    match client.read_response().await {
        Some(Ok(_)) => Ok(()),
        Some(Err(e)) => Err(e.into()),
        None => Err(Error::Closed),
    }
}

async fn open(cfg: &ServerConfig) -> Result<Session> {
    let tcp = TcpStream::connect((cfg.host.as_str(), cfg.port)).await?;
    let client = match cfg.security {
        Security::Tls => {
            let mut c = async_imap::Client::new(wrap_tls(&cfg.host, tcp).await?);
            greeting(&mut c).await?;
            c
        }
        Security::StartTls => {
            let mut plain = async_imap::Client::new(tcp);
            greeting(&mut plain).await?;
            plain.run_command_and_check_ok("STARTTLS", None).await?;
            async_imap::Client::new(wrap_tls(&cfg.host, plain.into_inner()).await?)
        }
        Security::None => {
            let mut c = async_imap::Client::new(Box::new(tcp) as Box<dyn Io>);
            greeting(&mut c).await?;
            c
        }
    };
    match &cfg.auth {
        Auth::Password(p) => client
            .login(&cfg.login, p)
            .await
            .map_err(|(e, _)| Error::Login(login_error(&e))),
        Auth::XOAuth2(token) => {
            let a = XOAuth2 {
                data: oauth::xoauth2(&cfg.login, token),
                sent: false,
            };
            client
                .authenticate("XOAUTH2", a)
                .await
                .map_err(|(e, _)| Error::Login(login_error(&e)))
        }
    }
}

/// Conecta e inicia sesión, con un límite de tiempo.
pub async fn connect(cfg: &ServerConfig) -> Result<Session> {
    tokio::time::timeout(TIMEOUT, open(cfg))
        .await
        .map_err(|_| Error::Timeout)?
}

/// Comprueba que el servidor y las credenciales funcionan.
pub async fn check(cfg: &ServerConfig) -> Result<()> {
    let mut s = connect(cfg).await?;
    let _ = s.logout().await;
    Ok(())
}

/// Lo que dijo el servidor al rechazar la entrada, sin el envoltorio de async-imap:
/// `code: None, info: Some("[AUTHENTICATIONFAILED] LOGIN failed")` → `LOGIN failed`.
fn login_error(e: &async_imap::error::Error) -> String {
    let raw = match e {
        async_imap::error::Error::No(s) | async_imap::error::Error::Bad(s) => s.clone(),
        other => return other.to_string(),
    };
    let info = raw
        .split_once("info: Some(\"")
        .and_then(|(_, rest)| rest.rsplit_once("\")"))
        .map(|(text, _)| text.replace("\\\"", "\""))
        .unwrap_or(raw);
    let text = match info.trim().strip_prefix('[') {
        Some(rest) => rest.split_once(']').map_or(rest, |(_, t)| t).trim().to_string(),
        None => info.trim().to_string(),
    };
    if text.is_empty() {
        "acceso denegado".into()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_errors_show_only_the_servers_words() {
        let no = |s: &str| async_imap::error::Error::No(s.into());
        assert_eq!(
            login_error(&no(
                "code: None, info: Some(\"[AUTHENTICATIONFAILED] LOGIN login failed\")"
            )),
            "LOGIN login failed"
        );
        assert_eq!(login_error(&no("credenciales no válidas")), "credenciales no válidas");
    }
}

//! OAuth2 para Gmail y Outlook: autorización, renovación del acceso y XOAUTH2.

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;

const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    Google,
    Microsoft,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "google" => Some(Provider::Google),
            "microsoft" => Some(Provider::Microsoft),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Google => "google",
            Provider::Microsoft => "microsoft",
        }
    }

    /// Servidores IMAP y SMTP: (host, puerto, cifrado).
    pub fn imap(self) -> (&'static str, u16, &'static str) {
        match self {
            Provider::Google => ("imap.gmail.com", 993, "tls"),
            Provider::Microsoft => ("outlook.office365.com", 993, "tls"),
        }
    }

    pub fn smtp(self) -> (&'static str, u16, &'static str) {
        match self {
            Provider::Google => ("smtp.gmail.com", 465, "tls"),
            Provider::Microsoft => ("smtp.office365.com", 587, "starttls"),
        }
    }
}

/// Una aplicación OAuth registrada por el administrador en Google Cloud o en Microsoft Entra.
#[derive(Debug, Clone)]
pub struct OAuthApp {
    pub provider: Provider,
    pub client_id: String,
    pub client_secret: String,
    pub auth_url: String,
    pub token_url: String,
    pub scope: String,
}

impl OAuthApp {
    pub fn google(client_id: String, client_secret: String) -> Self {
        OAuthApp {
            provider: Provider::Google,
            client_id,
            client_secret,
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token_url: "https://oauth2.googleapis.com/token".into(),
            scope: "https://mail.google.com/ openid email".into(),
        }
    }

    /// `tenant`: `common` (cuentas personales y de empresa), `organizations`, `consumers` o un id.
    pub fn microsoft(client_id: String, client_secret: String, tenant: &str) -> Self {
        let base = format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0");
        OAuthApp {
            provider: Provider::Microsoft,
            client_id,
            client_secret,
            auth_url: format!("{base}/authorize"),
            token_url: format!("{base}/token"),
            scope: "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send \
                    offline_access openid email"
                .into(),
        }
    }

    /// Dirección a la que se manda al usuario para dar permiso.
    pub fn authorize_url(&self, redirect_uri: &str, state: &str, challenge: &str, login_hint: Option<&str>) -> String {
        let mut params = vec![
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", redirect_uri),
            ("response_type", "code"),
            ("scope", self.scope.as_str()),
            ("state", state),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
        ];
        match self.provider {
            // Sin `prompt=consent`, Google solo da el token de renovación la primera vez.
            Provider::Google => params.extend([("access_type", "offline"), ("prompt", "consent")]),
            Provider::Microsoft => params.push(("prompt", "select_account")),
        }
        if let Some(h) = login_hint.filter(|h| !h.is_empty()) {
            params.push(("login_hint", h));
        }
        let query: Vec<String> = params.iter().map(|(k, v)| format!("{k}={}", urlencode(v))).collect();
        format!("{}?{}", self.auth_url, query.join("&"))
    }

    /// Cambia el código de la vuelta por los tokens.
    pub async fn exchange(&self, code: &str, redirect_uri: &str, verifier: &str) -> Result<Tokens, OAuthError> {
        self.token_request(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
        ])
        .await
    }

    /// Pide un token de acceso nuevo. Microsoft devuelve también otro token de renovación.
    pub async fn refresh(&self, refresh_token: &str) -> Result<Tokens, OAuthError> {
        self.token_request(&[("grant_type", "refresh_token"), ("refresh_token", refresh_token)])
            .await
    }

    async fn token_request(&self, extra: &[(&str, &str)]) -> Result<Tokens, OAuthError> {
        let mut form = vec![
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
        ];
        form.extend_from_slice(extra);
        let body: Vec<String> = form.iter().map(|(k, v)| format!("{k}={}", urlencode(v))).collect();
        let res = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| OAuthError::Network(e.to_string()))?
            .post(&self.token_url)
            .header("content-type", "application/x-www-form-urlencoded")
            .header("accept", "application/json")
            .body(body.join("&"))
            .send()
            .await
            .map_err(|e| OAuthError::Network(e.to_string()))?;
        let status = res.status();
        let bytes = res.bytes().await.map_err(|e| OAuthError::Network(e.to_string()))?;
        if !status.is_success() {
            #[derive(Deserialize)]
            struct Fail {
                error: Option<String>,
                error_description: Option<String>,
            }
            let f: Option<Fail> = serde_json::from_slice(&bytes).ok();
            let code = f.as_ref().and_then(|f| f.error.clone()).unwrap_or_default();
            if code == "invalid_grant" {
                return Err(OAuthError::Revoked);
            }
            let text = f
                .and_then(|f| f.error_description.or(f.error))
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(OAuthError::Rejected(text));
        }
        serde_json::from_slice(&bytes).map_err(|_| OAuthError::Rejected("respuesta no válida".into()))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Segundos que dura el token de acceso.
    #[serde(default = "default_expiry")]
    pub expires_in: u64,
    pub id_token: Option<String>,
}

fn default_expiry() -> u64 {
    3600
}

impl Tokens {
    /// La dirección de la cuenta, del `id_token`. Llega directamente del proveedor por
    /// HTTPS, así que no hace falta comprobar su firma (OpenID Connect, 3.1.3.7).
    pub fn email(&self) -> Option<String> {
        let payload = self.id_token.as_deref()?.split('.').nth(1)?;
        let json: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
        ["email", "preferred_username", "upn"]
            .iter()
            .filter_map(|k| json[*k].as_str())
            .find(|e| e.contains('@'))
            .map(|e| e.to_ascii_lowercase())
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum OAuthError {
    #[error("no se pudo contactar con el proveedor: {0}")]
    Network(String),
    #[error("el proveedor rechazó la petición: {0}")]
    Rejected(String),
    #[error("se retiró el permiso de acceso al correo; vuelve a conectar la cuenta")]
    Revoked,
}

/// Par PKCE: el verificador (secreto) y su reto (va en la URL).
pub fn pkce_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Cadena de XOAUTH2 (antes de pasarla a Base64), igual para IMAP y SMTP.
pub fn xoauth2(user: &str, access_token: &str) -> String {
    format!("user={user}\x01auth=Bearer {access_token}\x01\x01")
}

/// XOAUTH2 para `AUTHENTICATE` de IMAP.
pub(crate) struct XOAuth2 {
    pub data: String,
    pub sent: bool,
}

impl async_imap::Authenticator for XOAuth2 {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> String {
        // Si falla, el servidor manda un reto con el error y espera una respuesta vacía.
        if std::mem::replace(&mut self.sent, true) {
            String::new()
        } else {
            self.data.clone()
        }
    }
}

/// Codifica un valor para una URL o un formulario.
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Solo para las pruebas: un `id_token` sin firmar con esta dirección.
#[doc(hidden)]
pub fn fake_id_token(email: &str) -> String {
    let body = URL_SAFE_NO_PAD.encode(format!(r#"{{"email":"{email}"}}"#));
    format!("e30.{body}.firma")
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_imap::Authenticator;

    #[test]
    fn authorize_url_has_pkce_state_and_offline_access() {
        let app = OAuthApp::google("id".into(), "secreto".into());
        let url = app.authorize_url(
            "https://correo.ejemplo.es/api/v1/oauth/callback",
            "st4te",
            "reto",
            Some("ana@gmail.com"),
        );
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?client_id=id&"));
        assert!(url.contains("redirect_uri=https%3A%2F%2Fcorreo.ejemplo.es%2Fapi%2Fv1%2Foauth%2Fcallback"));
        assert!(url.contains("scope=https%3A%2F%2Fmail.google.com%2F%20openid%20email"));
        assert!(url.contains("state=st4te") && url.contains("code_challenge=reto&code_challenge_method=S256"));
        assert!(url.contains("access_type=offline") && url.contains("login_hint=ana%40gmail.com"));
        assert!(!url.contains("secreto"));

        let ms = OAuthApp::microsoft("id".into(), "s".into(), "common");
        let url = ms.authorize_url("https://x/cb", "s", "r", None);
        assert!(url.starts_with("https://login.microsoftonline.com/common/oauth2/v2.0/authorize?"));
        assert!(url.contains("offline_access") && !url.contains("login_hint"));
    }

    #[test]
    fn pkce_matches_rfc_example() {
        // RFC 7636, apéndice B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn email_comes_from_the_id_token() {
        let t = Tokens {
            access_token: "a".into(),
            refresh_token: None,
            expires_in: 3600,
            id_token: Some(fake_id_token("Ana@Gmail.com")),
        };
        assert_eq!(t.email().as_deref(), Some("ana@gmail.com"));
        assert_eq!(Tokens { id_token: None, ..t }.email(), None);
    }

    #[test]
    fn xoauth2_answers_once_then_empty() {
        let mut a = XOAuth2 {
            data: xoauth2("ana@gmail.com", "tok"),
            sent: false,
        };
        assert_eq!(a.process(b""), "user=ana@gmail.com\x01auth=Bearer tok\x01\x01");
        assert_eq!(a.process(b"{\"status\":\"401\"}"), "");
    }
}

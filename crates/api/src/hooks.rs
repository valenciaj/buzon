//! Reparto de webhooks: convierte los avisos de la sincronización en eventos y
//! envía la cola, firmada, con reintentos.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use buzon_store::hooks::{Attempt, Delivery, Webhook};
use buzon_store::now;
use buzon_sync::HookEvent;
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;
use tokio::sync::broadcast::error::RecvError;

use crate::AppState;

/// Lo que espera cada envío a que responda el otro lado.
const TIMEOUT: Duration = Duration::from_secs(10);
/// Envíos a la vez.
const BATCH: i64 = 20;

/// Arranca el repartidor y el puente con la sincronización.
pub fn start(st: Arc<AppState>) {
    tokio::spawn(bridge(st.clone()));
    tokio::spawn(run(st));
}

/// Avisos de la sincronización (correo nuevo, cuentas que fallan) a eventos.
async fn bridge(st: Arc<AppState>) {
    let mut rx = st.sync.subscribe_hooks();
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(RecvError::Lagged(n)) => {
                tracing::warn!("webhooks: se perdieron {n} avisos de la sincronización");
                continue;
            }
            Err(RecvError::Closed) => return,
        };
        match ev {
            HookEvent::Received { user, message, .. } => {
                let Ok(udb) = st.user_db(user).await else { continue };
                if let Ok(Some(m)) = udb.message_brief(message).await {
                    st.emit_hook(user, "message.received", json!(m)).await;
                }
            }
            HookEvent::SyncFailed { user, account, error } => {
                st.emit_hook(
                    user,
                    "account.sync_failed",
                    json!({ "account": account, "error": error }),
                )
                .await;
            }
        }
    }
}

async fn run(st: Arc<AppState>) {
    let mut pruned = 0;
    loop {
        match st.db.due_deliveries(BATCH).await {
            Ok(list) => {
                let mut set = tokio::task::JoinSet::new();
                for (d, w) in list {
                    let st = st.clone();
                    set.spawn(async move {
                        let (ok, response, error) = deliver(&st, &d, &w).await;
                        let a = Attempt {
                            ok,
                            response,
                            error: error.as_deref(),
                        };
                        if let Err(e) = st.db.record_attempt(d.id, a).await {
                            tracing::warn!("webhooks: no se pudo apuntar el envío {}: {e}", d.id);
                        }
                    });
                }
                set.join_all().await;
            }
            Err(e) => tracing::warn!("webhooks: no se pudo leer la cola: {e}"),
        }
        let t = now();
        if t - pruned > 3600 {
            pruned = t;
            let _ = st.db.prune_deliveries().await;
        }
        let wait = match st.db.next_delivery_at().await {
            Ok(Some(at)) => (at - now()).clamp(0, 60),
            _ => 60,
        };
        if wait > 0 {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(wait as u64)) => {}
                _ = st.hooks_ready.notified() => {}
            }
        }
    }
}

/// Firma de un envío: HMAC-SHA256 de `<t>.<cuerpo>`.
pub fn signature(secret: &[u8], t: i64, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC admite claves de cualquier tamaño");
    mac.update(format!("{t}.{body}").as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Hace un envío. Devuelve si fue bien, el código HTTP y el error.
async fn deliver(st: &AppState, d: &Delivery, w: &Webhook) -> (bool, Option<i64>, Option<String>) {
    let secret = match st.hooks_box().open(&w.secret) {
        Ok(s) => s,
        Err(_) => return (false, None, Some("No se pudo descifrar el secreto.".into())),
    };
    let client = match client_for(&w.url, st.allow_private_webhooks).await {
        Ok(c) => c,
        Err(e) => return (false, None, Some(e)),
    };
    let t = now();
    let res = client
        .post(&w.url)
        .header("content-type", "application/json")
        .header("buzon-event", &d.event)
        .header("buzon-delivery", d.id.to_string())
        .header(
            "buzon-signature",
            format!("t={t},v1={}", signature(&secret, t, &d.payload)),
        )
        .body(d.payload.clone())
        .send()
        .await;
    match res {
        Ok(r) => {
            let code = r.status().as_u16() as i64;
            if r.status().is_success() {
                (true, Some(code), None)
            } else {
                (false, Some(code), Some(format!("Respondió {code}.")))
            }
        }
        Err(e) if e.is_timeout() => (false, None, Some("No respondió en 10 segundos.".into())),
        Err(e) => {
            let mut msg = format!("No se pudo conectar: {e}");
            msg.truncate(300);
            (false, None, Some(msg))
        }
    }
}

/// Cliente para una URL. Sin permiso para la red local, resuelve el nombre, comprueba
/// que todas sus direcciones son públicas y se conecta solo a ellas (así un cambio de DNS
/// entre la comprobación y la conexión no lleva a la red interna).
async fn client_for(url: &str, allow_private: bool) -> Result<reqwest::Client, String> {
    let b = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("Buzon-Webhooks/", env!("CARGO_PKG_VERSION")));
    if allow_private {
        return b.build().map_err(|e| e.to_string());
    }
    let u = reqwest::Url::parse(url).map_err(|_| "La URL no es válida.".to_string())?;
    let host = u.host_str().ok_or("La URL no tiene servidor.")?.to_string();
    let port = u.port_or_known_default().unwrap_or(443);
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.trim_matches(['[', ']']), port))
        .await
        .map_err(|_| format!("No se encuentra el servidor {host}."))?
        .collect();
    if addrs.is_empty() || !addrs.iter().all(|a| is_public(a.ip())) {
        return Err("La URL apunta a la red local, y este servidor no lo permite.".into());
    }
    b.resolve_to_addrs(&host, &addrs).build().map_err(|e| e.to_string())
}

/// Dirección de internet (no local, privada ni reservada).
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64)
                || (o[0] == 198 && (o[1] & 0xfe) == 18)
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || s[0] == 0x2001 && s[1] == 0x0db8)
        }
    }
}

/// Comprueba la URL de un webhook al guardarlo. El error es para la persona.
pub fn check_url(url: &str, allow_private: bool) -> Result<(), String> {
    if url.len() > 2000 {
        return Err("La URL es demasiado larga.".into());
    }
    let u = reqwest::Url::parse(url).map_err(|_| "La URL no es válida.".to_string())?;
    match u.scheme() {
        "https" => {}
        "http" if allow_private => {}
        "http" => return Err("La URL debe empezar por https://.".into()),
        _ => return Err("La URL debe empezar por https://.".into()),
    }
    if u.host_str().is_none_or(str::is_empty) {
        return Err("La URL no tiene servidor.".into());
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err("No pongas usuario ni contraseña en la URL; comprueba la firma.".into());
    }
    if !allow_private {
        let host = u.host_str().unwrap_or_default().trim_matches(['[', ']']);
        let local = host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") || host.ends_with(".local");
        if local || host.parse::<IpAddr>().is_ok_and(|ip| !is_public(ip)) {
            return Err("La URL apunta a la red local, y este servidor no lo permite.".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_like_the_docs_say() {
        // echo -n '1700000000.{"a":1}' | openssl dgst -sha256 -hmac secreto
        assert_eq!(
            signature(b"secreto", 1_700_000_000, r#"{"a":1}"#),
            "b066ea5660a5cec64764b2352f7f0817fd722e527a09b7225028bb29454d1595"
        );
    }

    #[test]
    fn rejects_local_urls_unless_allowed() {
        assert!(check_url("https://hooks.example.com/x", false).is_ok());
        for bad in [
            "http://hooks.example.com/",
            "ftp://a.example/",
            "https://localhost/",
            "https://127.0.0.1/",
            "https://192.168.1.10/",
            "https://[::1]/",
            "https://[fd00::1]/",
            "https://10.0.0.1:8443/",
            "https://169.254.169.254/latest",
            "https://user:pw@a.example/",
            "no es una url",
        ] {
            assert!(check_url(bad, false).is_err(), "{bad}");
        }
        assert!(check_url("http://192.168.1.10:5678/webhook", true).is_ok());
        assert!(is_public("8.8.8.8".parse().unwrap()));
        assert!(!is_public("::ffff:10.0.0.1".parse().unwrap()));
        assert!(!is_public("100.64.0.1".parse().unwrap()));
    }
}

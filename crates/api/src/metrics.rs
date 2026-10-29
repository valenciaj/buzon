//! `GET /metrics`: estadísticas en el formato de texto de Prometheus, para Grafana.
//!
//! Solo responde si `metrics_token` está configurado, y pide ese token como
//! `Authorization: Bearer …`. Sin token configurado, la ruta no existe (404).

use std::fmt::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use buzon_sync::State as SyncState;

use crate::error::ApiError;
use crate::AppState;

/// Contadores desde que arrancó el servidor.
#[derive(Default)]
pub struct Counters {
    pub mails_sent: AtomicU64,
    pub send_failures: AtomicU64,
    pub webhooks_ok: AtomicU64,
    pub webhooks_failed: AtomicU64,
}

impl Counters {
    pub fn inc(c: &AtomicU64) {
        c.fetch_add(1, Ordering::Relaxed);
    }
}

/// Compara sin delatar por el tiempo cuántos caracteres coinciden.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn authorized(st: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(token) = st.metrics_token.as_deref() else {
        return Err(ApiError::not_found("Esa ruta no existe."));
    };
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    if same(given.trim().as_bytes(), token.as_bytes()) {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Falta el token de métricas o no es válido.",
        ))
    }
}

struct Out(String);

impl Out {
    fn metric(&mut self, name: &str, kind: &str, help: &str) {
        let _ = writeln!(self.0, "# HELP {name} {help}\n# TYPE {name} {kind}");
    }
    fn value(&mut self, name: &str, labels: &str, v: impl std::fmt::Display) {
        if labels.is_empty() {
            let _ = writeln!(self.0, "{name} {v}");
        } else {
            let _ = writeln!(self.0, "{name}{{{labels}}} {v}");
        }
    }
    fn single(&mut self, name: &str, kind: &str, help: &str, v: impl std::fmt::Display) {
        self.metric(name, kind, help);
        self.value(name, "", v);
    }
}

pub async fn metrics(State(st): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(e) = authorized(&st, &headers) {
        return e.into_response();
    }
    match render(&st).await {
        Ok(body) => (
            [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
            body,
        )
            .into_response(),
        Err(e) => e.into_response(),
    }
}

async fn render(st: &AppState) -> Result<String, ApiError> {
    let users = st.db.user_ids().await?;
    // Estados de las cuentas, en el orden en que se publican.
    let states = [
        (SyncState::Idle, "idle"),
        (SyncState::Syncing, "syncing"),
        (SyncState::Connecting, "connecting"),
        (SyncState::Error, "error"),
        (SyncState::Off, "off"),
    ];
    let mut by_state = [0u64; 5];
    let mut scheduled = 0u64;
    for user in &users {
        let udb = st.user_db(*user).await?;
        for a in udb.accounts().await? {
            let s = st.sync.state(a.id);
            if let Some(i) = states.iter().position(|(x, _)| *x == s) {
                by_state[i] += 1;
            }
        }
        scheduled += udb.scheduled().await?.len() as u64;
    }
    let pending_hooks = st.db.pending_deliveries().await?;
    let c = &st.counters;
    let load = |a: &AtomicU64| a.load(Ordering::Relaxed);

    let mut o = Out(String::new());
    o.metric("buzon_info", "gauge", "Versión de Buzón.");
    o.value("buzon_info", &format!("version=\"{}\"", env!("CARGO_PKG_VERSION")), 1);
    o.single(
        "buzon_start_time_seconds",
        "gauge",
        "Cuándo arrancó el servidor (segundos Unix).",
        st.started_at,
    );
    o.single("buzon_users", "gauge", "Usuarios de Buzón.", users.len());
    o.metric(
        "buzon_accounts",
        "gauge",
        "Cuentas de correo por estado de la sincronización.",
    );
    for (i, (_, name)) in states.iter().enumerate() {
        o.value("buzon_accounts", &format!("state=\"{name}\""), by_state[i]);
    }
    o.single(
        "buzon_scheduled_sends",
        "gauge",
        "Correos programados que aún no han salido.",
        scheduled,
    );
    o.single(
        "buzon_webhook_queue",
        "gauge",
        "Avisos de webhooks pendientes de entregar.",
        pending_hooks,
    );
    o.single(
        "buzon_mails_sent_total",
        "counter",
        "Correos enviados desde que arrancó el servidor.",
        load(&c.mails_sent),
    );
    o.single(
        "buzon_send_failures_total",
        "counter",
        "Envíos que el servidor SMTP rechazó o no pudieron salir.",
        load(&c.send_failures),
    );
    o.metric(
        "buzon_webhook_deliveries_total",
        "counter",
        "Intentos de entrega de webhooks por resultado.",
    );
    o.value("buzon_webhook_deliveries_total", "result=\"ok\"", load(&c.webhooks_ok));
    o.value(
        "buzon_webhook_deliveries_total",
        "result=\"failed\"",
        load(&c.webhooks_failed),
    );
    Ok(o.0)
}

#[cfg(test)]
mod tests {
    use super::same;

    #[test]
    fn compares_tokens() {
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"abcd"));
        assert!(!same(b"", b"x"));
    }
}

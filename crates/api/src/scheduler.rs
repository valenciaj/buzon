//! Envíos programados: cada borrador con fecha sale cuando le toca.
//! «Deshacer envío» es un envío programado a pocos segundos vista.

use std::sync::Arc;
use std::time::Duration;

use buzon_store::{now, Id};

use crate::routes::compose::{deliver, draft_in};
use crate::AppState;

const INTERRUPTED: &str =
    "El servidor se reinició mientras salía este correo. Revisa si llegó a enviarse antes de volver a intentarlo.";

/// Arranca el repartidor de envíos programados.
pub fn start(st: Arc<AppState>) {
    tokio::spawn(run(st));
}

async fn run(st: Arc<AppState>) {
    if let Ok(users) = st.db.user_ids().await {
        for user in users {
            if let Ok(udb) = st.user_db(user).await {
                let _ = udb.recover_interrupted_sends(INTERRUPTED).await;
            }
        }
    }
    loop {
        let next = match tick(&st).await {
            Ok(next) => next,
            Err(e) => {
                tracing::warn!("envíos programados: {e}");
                None
            }
        };
        let wait = next.map_or(30, |at| (at - now()).clamp(0, 30));
        if wait > 0 {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(wait as u64)) => {}
                _ = st.sends_ready.notified() => {}
            }
        }
    }
}

/// Envía lo que toca. Devuelve cuándo es el siguiente envío.
async fn tick(st: &AppState) -> buzon_store::Result<Option<i64>> {
    let mut next: Option<i64> = None;
    for user in st.db.user_ids().await? {
        let Ok(udb) = st.user_db(user).await else { continue };
        for d in udb.take_due_drafts(now()).await? {
            let id = d.id.expect("guardado");
            let days = d.follow_up_days;
            if let Err(e) = deliver(st, user, draft_in(d), Some(id), days).await {
                tracing::info!("envío programado {id}: {}", e.message);
                let _ = udb.schedule_draft(id, None, days, Some(&e.message)).await;
                notify(st, user).await;
            }
        }
        if let Some(at) = udb.next_send_at().await? {
            next = Some(next.map_or(at, |n| n.min(at)));
        }
    }
    Ok(next)
}

/// Para que la web vuelva a cargar Borradores y Programados.
async fn notify(st: &AppState, user: Id) {
    if let Ok(udb) = st.user_db(user).await {
        if let Ok(accounts) = udb.accounts().await {
            for a in accounts {
                st.sync.emit(user, a.id);
            }
        }
    }
}

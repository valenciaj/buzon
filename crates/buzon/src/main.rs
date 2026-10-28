mod config;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use buzon_api::AppState;
use buzon_store::crypto::MasterKey;
use buzon_store::AppDb;
use config::Config;

fn config_path() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" | "-c" => return args.next().map(PathBuf::from),
            "--version" | "-V" => {
                println!("buzon {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!("Uso: buzon [--config /etc/buzon/config.toml]");
                std::process::exit(0);
            }
            _ => {}
        }
    }
    std::env::var_os("BUZON_CONFIG").map(PathBuf::from)
}

/// Lee la clave maestra o crea una nueva (permisos 600) la primera vez.
fn load_master_key(path: &Path) -> anyhow::Result<MasterKey> {
    if path.exists() {
        let text = std::fs::read_to_string(path).with_context(|| format!("leyendo {}", path.display()))?;
        return Ok(MasterKey::from_hex(&text)?);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let key = MasterKey::generate();
    std::fs::write(path, key.to_hex() + "\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    tracing::warn!(
        "clave maestra nueva creada en {}; haz copia de seguridad",
        path.display()
    );
    Ok(key)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "buzon=info,tower_http=info".into()),
        )
        .init();

    let cfg = Config::load(config_path().as_deref())?;
    std::fs::create_dir_all(&cfg.data_dir)?;
    let master = load_master_key(&cfg.master_key_path())?;
    let db = AppDb::open(&cfg.data_dir.join("app.db"))
        .await
        .context("abriendo app.db")?;

    let web_dir = cfg.web_dir.clone().filter(|d| d.join("index.html").exists());
    if web_dir.is_none() {
        tracing::warn!("no se encuentra el frontend compilado; solo se sirve la API");
    }
    let state = Arc::new(AppState::new(
        db,
        cfg.data_dir.clone(),
        master,
        cfg.cookie_secure,
        cfg.public_url.clone(),
    ));
    let app = buzon_api::app(state, web_dir).layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&cfg.listen)
        .await
        .with_context(|| format!("escuchando en {}", cfg.listen))?;
    tracing::info!("Buzón escuchando en http://{}", cfg.listen);
    axum::serve(listener, app).with_graceful_shutdown(shutdown()).await?;
    Ok(())
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}

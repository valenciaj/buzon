mod config;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use buzon_api::AppState;
use buzon_store::crypto::MasterKey;
use buzon_store::AppDb;
use buzon_sync::{OAuthApp, Provider};
use config::Config;

const USAGE: &str = "Uso:
  buzon [--config FICHERO]                               arranca el servidor
  buzon [--config FICHERO] backup CARPETA [--keep N]     copia de seguridad en caliente

La configuración también se puede indicar con BUZON_CONFIG.
Copias: crea CARPETA/buzon-AAAAMMDD-HHMMSS y deja las N más recientes (7 por defecto).";

enum Command {
    Serve,
    Backup { dest: PathBuf, keep: usize },
}

fn parse_args() -> anyhow::Result<(Option<PathBuf>, Command)> {
    let mut config = std::env::var_os("BUZON_CONFIG").map(PathBuf::from);
    let mut keep = 7;
    let mut positional = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" | "-c" => config = args.next().map(PathBuf::from),
            "--keep" => {
                keep = args
                    .next()
                    .and_then(|n| n.parse().ok())
                    .filter(|n| *n > 0)
                    .context("--keep necesita un número mayor que 0")?
            }
            "--version" | "-V" => {
                println!("buzon {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            _ if a.starts_with('-') => anyhow::bail!("opción desconocida: {a}\n\n{USAGE}"),
            _ => positional.push(a),
        }
    }
    let cmd = match positional.as_slice() {
        [] => Command::Serve,
        [b, dest] if b == "backup" => Command::Backup {
            dest: PathBuf::from(dest),
            keep,
        },
        _ => anyhow::bail!("{USAGE}"),
    };
    Ok((config, cmd))
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

    let (config, cmd) = parse_args()?;
    let cfg = Config::load(config.as_deref())?;
    if let Command::Backup { dest, keep } = cmd {
        let r = buzon_store::backup::backup(&cfg.data_dir, &dest, keep)
            .await
            .context("haciendo la copia de seguridad")?;
        println!(
            "Copia en {}: {} bases de datos, {} ficheros copiados y {} enlazados de la copia anterior.",
            r.path.display(),
            r.databases,
            r.copied,
            r.linked
        );
        for old in r.removed {
            println!("Borrada la copia antigua {}", old.display());
        }
        println!(
            "Recuerda guardar aparte la clave maestra ({}): sin ella no se pueden leer las contraseñas de las cuentas.",
            cfg.master_key_path().display()
        );
        return Ok(());
    }
    std::fs::create_dir_all(&cfg.data_dir)?;
    let master = load_master_key(&cfg.master_key_path())?;
    let db = AppDb::open(&cfg.data_dir.join("app.db"))
        .await
        .context("abriendo app.db")?;

    let web_dir = cfg.web_dir.clone().filter(|d| d.join("index.html").exists());
    if web_dir.is_none() {
        tracing::warn!("no se encuentra el frontend compilado; solo se sirve la API");
    }
    let mut state = AppState::new(
        db,
        cfg.data_dir.clone(),
        master,
        cfg.cookie_secure,
        cfg.public_url.clone(),
    )
    .with_sync();
    state.allow_insecure_mail = cfg.allow_insecure_mail;
    state.allow_private_webhooks = cfg.allow_private_webhooks;
    state.metrics_token = cfg.metrics_token.clone().filter(|t| !t.trim().is_empty());
    if let Some(g) = &cfg.oauth.google {
        let app = OAuthApp::google(g.client_id.clone(), g.client_secret.clone());
        state.oauth.insert(Provider::Google, Arc::new(app));
    }
    if let Some(m) = &cfg.oauth.microsoft {
        let tenant = m.tenant.as_deref().unwrap_or("common");
        let app = OAuthApp::microsoft(m.client_id.clone(), m.client_secret.clone(), tenant);
        state.oauth.insert(Provider::Microsoft, Arc::new(app));
    }
    let state = Arc::new(state);
    buzon_api::start_background(state.clone());
    match state.start_all_accounts().await {
        Ok(n) => tracing::info!("sincronizando {n} cuentas de correo"),
        Err(e) => tracing::error!("no se pudo arrancar la sincronización: {}", e.message),
    }
    let app = buzon_api::app(state.clone(), web_dir).layer(tower_http::trace::TraceLayer::new_for_http());

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

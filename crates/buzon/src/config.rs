use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Configuración del servidor. Se lee de un fichero TOML (`--config` o `BUZON_CONFIG`).
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Dirección donde escucha. En producción, solo local: nginx hace de proxy.
    pub listen: String,
    /// Carpeta de datos: `app.db`, `users/` y adjuntos.
    pub data_dir: PathBuf,
    /// Frontend compilado (`web/dist`). Si no existe, solo se sirve la API.
    pub web_dir: Option<PathBuf>,
    /// Fichero con la clave maestra (64 caracteres hex). Se crea si no existe.
    pub master_key_file: Option<PathBuf>,
    /// URL pública, por ejemplo `https://correo.ejemplo.es`.
    pub public_url: Option<String>,
    /// Cookie de sesión solo por HTTPS.
    pub cookie_secure: bool,
    /// Permite cuentas IMAP/SMTP sin cifrar. Solo para pruebas locales.
    pub allow_insecure_mail: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            listen: "127.0.0.1:8080".into(),
            data_dir: PathBuf::from("data"),
            web_dir: Some(PathBuf::from("web/dist")),
            master_key_file: None,
            public_url: None,
            cookie_secure: true,
            allow_insecure_mail: false,
        }
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> anyhow::Result<Self> {
        match path {
            Some(p) => {
                let text =
                    std::fs::read_to_string(p).map_err(|e| anyhow::anyhow!("no se pudo leer {}: {e}", p.display()))?;
                Ok(toml::from_str(&text)?)
            }
            None => Ok(Config::default()),
        }
    }

    pub fn master_key_path(&self) -> PathBuf {
        self.master_key_file
            .clone()
            .unwrap_or_else(|| self.data_dir.join("master.key"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_parses() {
        let text = include_str!("../../../deploy/config.example.toml");
        let c: Config = toml::from_str(text).unwrap();
        assert_eq!(c.listen, "127.0.0.1:8080");
        assert!(c.cookie_secure);
        assert!(!c.allow_insecure_mail);
    }
}

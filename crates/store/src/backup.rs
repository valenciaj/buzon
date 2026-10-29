//! Copias de seguridad en caliente: las bases de datos con `VACUUM INTO` (coherentes
//! aunque el servidor esté funcionando) y los ficheros de cada usuario (correos `.eml`
//! y adjuntos). Los ficheros que no han cambiado desde la copia anterior se enlazan
//! (hard link) en vez de copiarse, así que cada copia diaria ocupa poco.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection};

use crate::{now, Result};

const PREFIX: &str = "buzon-";

#[derive(Debug)]
pub struct BackupReport {
    /// Carpeta de la copia nueva.
    pub path: PathBuf,
    pub databases: usize,
    /// Ficheros copiados y enlazados desde la copia anterior.
    pub copied: usize,
    pub linked: usize,
    /// Copias antiguas borradas por `keep`.
    pub removed: Vec<PathBuf>,
}

/// Hace una copia de `data_dir` en `dest/buzon-AAAAMMDD-HHMMSS` y deja solo las `keep` más recientes.
/// La copia se escribe primero en una carpeta temporal: una copia a medias nunca cuenta.
pub async fn backup(data_dir: &Path, dest: &Path, keep: usize) -> Result<BackupReport> {
    std::fs::create_dir_all(dest)?;
    for stale in list(dest, ".tmp-")? {
        let _ = std::fs::remove_dir_all(stale);
    }
    let previous = list(dest, PREFIX)?.pop();
    let mut name = format!("{PREFIX}{}", stamp(now()));
    let mut n = 1;
    while dest.join(&name).exists() {
        n += 1;
        name = format!("{PREFIX}{}-{n}", stamp(now()));
    }
    let tmp = dest.join(format!(".tmp-{name}"));
    std::fs::create_dir_all(tmp.join("users"))?;

    let mut report = BackupReport {
        path: dest.join(&name),
        databases: 0,
        copied: 0,
        linked: 0,
        removed: Vec::new(),
    };
    let app = data_dir.join("app.db");
    if app.exists() {
        vacuum_into(&app, &tmp.join("app.db")).await?;
        report.databases += 1;
    }
    let users = data_dir.join("users");
    if users.is_dir() {
        for entry in std::fs::read_dir(&users)? {
            let path = entry?.path();
            let file = path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            if path.is_dir() {
                copy_tree(
                    &path,
                    &tmp.join("users").join(&file),
                    previous.as_ref().map(|p| p.join("users").join(&file)).as_deref(),
                    &mut report,
                )?;
            } else if file.ends_with(".db") {
                vacuum_into(&path, &tmp.join("users").join(&file)).await?;
                report.databases += 1;
            }
        }
    }
    std::fs::rename(&tmp, &report.path)?;

    let all = list(dest, PREFIX)?;
    let extra = all.len().saturating_sub(keep.max(1));
    for old in all.into_iter().take(extra) {
        std::fs::remove_dir_all(&old)?;
        report.removed.push(old);
    }
    Ok(report)
}

/// Copia coherente de una base de datos SQLite, aunque otro proceso la esté usando.
async fn vacuum_into(src: &Path, dst: &Path) -> Result<()> {
    let mut conn = SqliteConnectOptions::from_str(&format!("sqlite://{}", src.display()))?
        .connect()
        .await?;
    sqlx::query("VACUUM INTO ?")
        .bind(dst.to_string_lossy().into_owned())
        .execute(&mut conn)
        .await?;
    conn.close().await?;
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path, prev: Option<&Path>, report: &mut BackupReport) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let (from, to) = (entry.path(), dst.join(&name));
        let before = prev.map(|p| p.join(&name));
        let meta = entry.metadata()?;
        if meta.is_dir() {
            copy_tree(&from, &to, before.as_deref(), report)?;
            continue;
        }
        // Los correos y adjuntos no cambian una vez guardados: si ya estaban, se enlazan.
        let same = before
            .as_ref()
            .and_then(|b| std::fs::metadata(b).ok())
            .is_some_and(|m| m.is_file() && m.len() == meta.len());
        if same && std::fs::hard_link(before.as_ref().expect("existe"), &to).is_ok() {
            report.linked += 1;
        } else {
            std::fs::copy(&from, &to)?;
            report.copied += 1;
        }
    }
    Ok(())
}

/// Carpetas de `dir` que empiezan por `prefix`, de la más antigua a la más reciente.
fn list(dir: &Path, prefix: &str) -> Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(prefix) && e.path().is_dir())
        .map(|e| e.path())
        .collect();
    out.sort();
    Ok(out)
}

/// `AAAAMMDD-HHMMSS` en UTC.
fn stamp(secs: i64) -> String {
    let (days, rest) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Algoritmo de Howard Hinnant para pasar de días a fecha civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::MasterKey;
    use crate::user::NewAccount;
    use crate::{AppDb, Id, UserDb};

    #[test]
    fn stamps_are_utc_dates() {
        assert_eq!(stamp(0), "19700101-000000");
        assert_eq!(stamp(1_793_252_079), "20261029-053439");
        assert_eq!(stamp(951_782_400), "20000229-000000");
    }

    #[tokio::test]
    async fn backups_are_consistent_linked_and_pruned() {
        let data = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        AppDb::open(&data.path().join("app.db")).await.unwrap();
        let user = Id::new();
        let udb = UserDb::open(data.path(), user).await.unwrap();
        let key = MasterKey::generate().derive("user");
        udb.add_account(
            NewAccount {
                name: "Personal",
                email: "yo@ejemplo.es",
                imap_host: "imap.ejemplo.es",
                imap_port: 993,
                smtp_host: "smtp.ejemplo.es",
                smtp_port: 465,
                login: "yo",
                password: "secreto",
                auth: "password",
                imap_security: "tls",
                smtp_security: "tls",
            },
            &key,
        )
        .await
        .unwrap();
        let raw = crate::user::user_raw_dir(data.path(), user);
        std::fs::create_dir_all(&raw).unwrap();
        std::fs::write(raw.join("a.eml"), "Subject: hola\r\n\r\nhola").unwrap();

        // Con la base de datos abierta (como con el servidor en marcha).
        let first = backup(data.path(), dest.path(), 7).await.unwrap();
        assert_eq!((first.databases, first.copied, first.linked), (2, 1, 0));
        let restored = UserDb::open(&first.path, user).await.unwrap();
        assert_eq!(restored.accounts().await.unwrap()[0].email, "yo@ejemplo.es");

        // La segunda enlaza el correo que no ha cambiado y copia el nuevo.
        std::fs::write(raw.join("b.eml"), "Subject: otro\r\n\r\notro").unwrap();
        let second = backup(data.path(), dest.path(), 2).await.unwrap();
        assert_eq!((second.copied, second.linked), (1, 1));
        let rel = Path::new("users").join(user.to_base58()).join("raw").join("a.eml");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let ino = |p: &Path| std::fs::metadata(p).unwrap().ino();
            assert_eq!(ino(&first.path.join(&rel)), ino(&second.path.join(&rel)));
        }

        // Solo se guardan las `keep` más recientes.
        let third = backup(data.path(), dest.path(), 2).await.unwrap();
        assert_eq!(third.removed, vec![first.path.clone()]);
        assert!(!first.path.exists() && second.path.join(&rel).exists());
        assert_eq!(list(dest.path(), PREFIX).unwrap(), vec![second.path, third.path]);
    }
}

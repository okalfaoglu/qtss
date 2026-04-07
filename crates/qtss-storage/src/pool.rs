use std::path::{Path, PathBuf};
use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::migrate::Migrator;
use sqlx::PgPool;

use crate::error::StorageError;

/// Resolve the SQLx migrations directory at runtime (not compile-time embed).
///
/// Order: `QTSS_MIGRATIONS_DIR` (absolute or relative to cwd), then `./migrations` under
/// [`std::env::current_dir`], then `../../migrations` from this crate's manifest (covers
/// `cargo test` when cwd is `crates/qtss-storage`).
///
/// Production: run the worker/API with `WorkingDirectory` at the repo root (see
/// `deploy/systemd/*.service.example`) so `./migrations` resolves to the live tree.
fn resolve_migrations_dir() -> Result<PathBuf, StorageError> {
    if let Ok(raw) = std::env::var("QTSS_MIGRATIONS_DIR") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let p = Path::new(trimmed);
            let resolved = if p.is_absolute() {
                p.to_path_buf()
            } else {
                std::env::current_dir()
                    .map_err(|e| StorageError::Other(format!("cwd: {e}")))?
                    .join(p)
            };
            if resolved.is_dir() {
                return Ok(resolved);
            }
            return Err(StorageError::Other(format!(
                "QTSS_MIGRATIONS_DIR is not a directory: {}",
                resolved.display()
            )));
        }
    }

    let cwd = std::env::current_dir().map_err(|e| StorageError::Other(format!("cwd: {e}")))?;
    let under_cwd = cwd.join("migrations");
    if under_cwd.is_dir() {
        return Ok(under_cwd);
    }

    let from_manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    if from_manifest.is_dir() {
        return Ok(from_manifest);
    }

    Err(StorageError::Other(format!(
        "migrations directory not found (tried {}, {}). \
         Set QTSS_MIGRATIONS_DIR, or start the process with cwd at the repo root (e.g. systemd WorkingDirectory=/app/qtss).",
        under_cwd.display(),
        from_manifest.display()
    )))
}

pub async fn create_pool(database_url: &str, max_connections: u32) -> Result<PgPool, StorageError> {
    PgConnectOptions::from_str(database_url).map_err(|e| {
        StorageError::Other(format!(
            "DATABASE_URL ayrıştırılamadı: {e}. \
             Örnek: postgres://KULLANICI:SIFRE@127.0.0.1:5432/VERITABANI. \
             .env içinde `DATABASE_URL=` boş bırakmayın (satırı silin veya tam URL yazın); \
             dokümandaki `export DATABASE_URL='...'` ifadesindeki üç nokta yer tutucudur, shell’e aynen yapıştırmayın."
        ))
    })?;
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &PgPool) -> Result<(), StorageError> {
    let dir = resolve_migrations_dir()?;
    let migrator = Migrator::new(dir.clone())
        .await
        .map_err(StorageError::from)?;
    migrator
        .run(pool)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("has been modified") {
                return StorageError::Other(format!(
                    "{msg}\n\
                     [QTSS] Checksum drift for an already-applied migration. Typical causes: (1) two files share the same numeric prefix (e.g. `ls migrations/0001*.sql` must list exactly one); (2) the `.sql` file was edited after it was applied.\n\
                     Fix: remove duplicate `NNNN_*.sql` names, then from repo root with valid `DATABASE_URL`: \
                     `cargo run -p qtss-storage --bin qtss-sync-sqlx-checksums` — updates `_sqlx_migrations.checksum` for each version on disk. \
                     Then rerun worker/API. Only use this when the SQL on disk matches what was actually executed; if the DB schema is wrong, restore the original migration file or repair the DB. \
                     See docs/QTSS_CURSOR_DEV_GUIDE.md §6."
                ));
            }
            if msg.contains("bar_intervals") && msg.contains("does not exist") {
                return StorageError::Other(format!(
                    "{msg}\n\
                     [QTSS] `bar_intervals` tablosu eksik — tipik neden: eski/bozuk 0013 veya tablo silinmiş. \
                     Çözüm: `0036_bar_intervals_repair_if_missing.sql` migrasyonunu uygulayın (`cargo run -p qtss-api` / worker). \
                     Çift önek / checksum: docs/QTSS_CURSOR_DEV_GUIDE.md §6."
                ));
            }
            if msg.contains("previously applied but is missing in the resolved migrations") {
                return StorageError::Other(format!(
                    "{msg}\n\
                     [QTSS] `_sqlx_migrations` has a version that is not present under the resolved migrations directory ({}). \
                     Deploy every `NNNN_*.sql` through that version into that folder, or set QTSS_MIGRATIONS_DIR to the repo `migrations/` path. \
                     Do not delete `_sqlx_migrations` rows unless you are sure the SQL never ran. See docs/QTSS_CURSOR_DEV_GUIDE.md §6.",
                    dir.display()
                ));
            }
            StorageError::Migrate(e)
        })?;
    Ok(())
}

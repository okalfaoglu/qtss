use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

use crate::error::StorageError;

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
    sqlx::migrate!("../../migrations")
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
                     [QTSS] `_sqlx_migrations` lists a migration version the running binary did not embed at compile time (sqlx::migrate! reads `migrations/` when you `cargo build`). \
                     Fix: on the build host, ensure `migrations/` includes every numbered file up to that version (e.g. `0009_*.sql`), then rebuild and redeploy `qtss-worker` from that tree (`cargo build --release -p qtss-worker`). \
                     Do not delete `_sqlx_migrations` rows unless you know the SQL was never applied. See docs/QTSS_CURSOR_DEV_GUIDE.md §6."
                ));
            }
            StorageError::Migrate(e)
        })?;
    Ok(())
}

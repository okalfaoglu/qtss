use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::error::StorageError;

pub async fn create_pool(database_url: &str, max_connections: u32) -> Result<PgPool, StorageError> {
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
                     [QTSS] Migration dosyası uygulandıktan sonra değişmiş görünüyor. \
                     Yalnızca yorum/satır sonu gibi içerik değiştiyse, repo kökünde DATABASE_URL ile: \
                     `cargo run -p qtss-storage --bin qtss-sync-sqlx-checksums` \
                     — `_sqlx_migrations.checksum` diskteki dosyayla hizalanır. \
                     Gerçek şema farkı varsa dosyayı eski haline getirin veya yeni numaralı migration ekleyin; \
                     aynı sayısal önekli iki dosya kullanmayın (ör. çift `0013_*.sql`) — bkz. docs/QTSS_CURSOR_DEV_GUIDE.md §6."
                ));
            }
            if msg.contains("bar_intervals") && msg.contains("does not exist") {
                return StorageError::Other(format!(
                    "{msg}\n\
                     [QTSS] `bar_intervals` tablosu eksik — tipik neden: `0013_worker_analytics_schema.sql` şeması uygulanmamış veya tablo silinmiş. \
                     Geliştirme DB: migrasyon zincirini doğrula veya 0013 içeriğini güvenli uygula. \
                     `0034`/`0035` ve çift önek kuralları: docs/QTSS_CURSOR_DEV_GUIDE.md §6."
                ));
            }
            StorageError::Migrate(e)
        })?;
    Ok(())
}

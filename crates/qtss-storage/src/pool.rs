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
                     çift `0013_*.sql` kullanmayın."
                ));
            }
            StorageError::Migrate(e)
        })?;
    Ok(())
}

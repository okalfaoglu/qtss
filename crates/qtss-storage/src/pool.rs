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
                     [QTSS] `bar_intervals` tablosu eksik — tipik neden: eski/bozuk 0013 veya tablo silinmiş. \
                     Çözüm: `0036_bar_intervals_repair_if_missing.sql` migrasyonunu uygulayın (`cargo run -p qtss-api` / worker). \
                     Çift önek / checksum: docs/QTSS_CURSOR_DEV_GUIDE.md §6."
                ));
            }
            StorageError::Migrate(e)
        })?;
    Ok(())
}

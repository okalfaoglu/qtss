//! Geliştirici yardımcısı: `_sqlx_migrations.checksum` değerlerini, diskteki `migrations/*.sql`
//! içeriğinin SHA-384 özetine günceller (SQLx 0.8 ile aynı kural).
//!
//! Kullanım (repo kökünden; `.env` içindeki `DATABASE_URL` otomatik okunur, `qtss-api` ile aynı):
//! ```text
//! cargo run -p qtss-storage --bin qtss-sync-sqlx-checksums
//! ```
//! Ardından: `cargo run -p qtss-api`
//!
//! Uyarı: Şema dosyayla uyumsuzsa bu araç hatayı gizler; yalnızca dosya yorum/satır sonu
//! gibi değişikliklerde veya yanlışlıkla düzenlenmiş migration’larda checksum uyumsuzluğunu giderir.
//!
//! **Önemli:** Aynı sürüm numarası (`0002_*.sql` vb.) birden fazla dosyada kullanılamaz — SQLx tek checksum tutar;
//! ikinci dosya birincinin özetini ezer ve `migrate` sürekli hata verir. Tam kural ve sürüm tablosu: `docs/QTSS_CURSOR_DEV_GUIDE.md` §6.

use anyhow::{bail, Context};
use qtss_common::{load_dotenv, require_postgres_database_url};
use qtss_storage::sync_sqlx_migration_checksums_from_disk;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv();
    let database_url = require_postgres_database_url().map_err(anyhow::Error::msg)?;

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .context("PostgreSQL bağlantısı")?;

    let updated = sync_sqlx_migration_checksums_from_disk(&pool)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if updated == 0 {
        bail!(
            "hiç satır güncellenmedi — `_sqlx_migrations` boş veya sürümler diskteki dosyalarla eşleşmiyor. \
             Yeni veritabanında önce API/worker ile migrasyon çalıştırın; checksum drift için repo kökünde geçerli `migrations/` olduğundan emin olun."
        );
    }

    println!("güncellenen satır: {updated}. Next: cargo run -p qtss-api (and/or cargo run -p qtss-worker --bin qtss-worker)");
    Ok(())
}

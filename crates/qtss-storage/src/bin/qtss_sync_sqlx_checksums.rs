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
//! **Önemli:** Aynı sürüm numarası (`0014_*.sql`) birden fazla dosyada kullanılamaz — SQLx tek checksum tutar;
//! ikinci dosya birincinin özetini ezer ve `migrate` sürekli hata verir. Tam kural ve sürüm tablosu: `docs/QTSS_CURSOR_DEV_GUIDE.md` §6.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{bail, Context};
use qtss_common::load_dotenv;
use sha2::{Digest, Sha384};
use sqlx::postgres::PgPoolOptions;

fn default_migrations_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv();
    let database_url = std::env::var("DATABASE_URL")
        .context("DATABASE_URL gerekli (.env veya ortam; qtss-api ile aynı)")?;

    let migrations_dir = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(default_migrations_dir);
    let migrations_dir = migrations_dir
        .canonicalize()
        .with_context(|| format!("migrations dizini okunamadı: {}", migrations_dir.display()))?;

    let mut by_version: HashMap<i64, Vec<String>> = HashMap::new();
    for entry in fs::read_dir(&migrations_dir).with_context(|| migrations_dir.display().to_string())? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let parts: Vec<&str> = file_name.splitn(2, '_').collect();
        if parts.len() != 2 || !parts[1].ends_with(".sql") {
            continue;
        }
        let version: i64 = parts[0]
            .parse()
            .with_context(|| format!("migration dosya adı: {file_name}"))?;
        by_version.entry(version).or_default().push(file_name);
    }

    for (v, names) in &by_version {
        if names.len() > 1 {
            bail!(
                "Çift migration sürümü v{v}: SQLx her sürüm için tek dosya bekler. Dosyalar: {}. \
                 Birini yeniden numaralandırın (ör. 0015_...sql).",
                names.join(", ")
            );
        }
    }

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .context("PostgreSQL bağlantısı")?;

    let mut updated = 0u64;
    for entry in fs::read_dir(&migrations_dir).with_context(|| migrations_dir.display().to_string())? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let parts: Vec<&str> = file_name.splitn(2, '_').collect();
        if parts.len() != 2 || !parts[1].ends_with(".sql") {
            continue;
        }
        let version: i64 = parts[0]
            .parse()
            .with_context(|| format!("migration dosya adı: {file_name}"))?;

        let sql = fs::read_to_string(&path).with_context(|| path.display().to_string())?;
        let digest = Sha384::digest(sql.as_bytes());
        let checksum: &[u8] = digest.as_slice();

        let res = sqlx::query(
            r#"UPDATE _sqlx_migrations SET checksum = $1 WHERE version = $2"#,
        )
        .bind(checksum)
        .bind(version)
        .execute(&pool)
        .await
        .context("_sqlx_migrations güncelleme")?;

        if res.rows_affected() == 0 {
            eprintln!("atlandı (DB’de yok): v{version} {file_name}");
        } else {
            println!("güncellendi: v{version} {file_name}");
            updated += res.rows_affected();
        }
    }

    if updated == 0 {
        bail!(
            "hiç satır güncellenmedi — migrations dizini: {}. Tablo boş veya sürüm uyuşmuyor olabilir.",
            migrations_dir.display()
        );
    }

    println!("Tamam. Şimdi: cargo run -p qtss-api");
    Ok(())
}

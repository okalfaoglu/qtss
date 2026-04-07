//! Align `_sqlx_migrations.checksum` with on-disk `migrations/NNNN_*.sql` (SHA-384, SQLx 0.8 rule).
//! Same behavior as the `qtss-sync-sqlx-checksums` binary — used by integration tests before
//! [`crate::pool::run_migrations`] when a dev DB was migrated with an older file revision.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha384};
use sqlx::PgPool;

use crate::error::StorageError;
use crate::pool::resolve_migrations_dir;

fn collect_migration_files(dir: &Path) -> Result<Vec<(i64, std::path::PathBuf)>, StorageError> {
    let mut by_version: HashMap<i64, Vec<String>> = HashMap::new();
    let mut out: Vec<(i64, std::path::PathBuf)> = Vec::new();

    for entry in fs::read_dir(dir).map_err(|e| {
        StorageError::Other(format!("read migrations directory {}: {e}", dir.display()))
    })? {
        let entry = entry.map_err(|e| StorageError::Other(format!("migrations dir entry: {e}")))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let parts: Vec<&str> = file_name.splitn(2, '_').collect();
        if parts.len() != 2 || !parts[1].ends_with(".sql") {
            continue;
        }
        let version: i64 = parts[0].parse().map_err(|_| {
            StorageError::Other(format!("invalid migration version in file name: {file_name}"))
        })?;
        by_version
            .entry(version)
            .or_default()
            .push(file_name.clone());
        out.push((version, path));
    }

    for (v, names) in &by_version {
        if names.len() > 1 {
            return Err(StorageError::Other(format!(
                "duplicate migration version {v}: SQLx expects one file per version. Files: {}",
                names.join(", ")
            )));
        }
    }

    out.sort_by_key(|(v, _)| *v);
    Ok(out)
}

/// Updates `_sqlx_migrations.checksum` for each version that exists both on disk and in the table.
/// Returns how many rows were updated. Safe on an empty table (updates 0 rows).
///
/// **Warning:** Only use when on-disk SQL matches what was actually executed; otherwise schema drift
/// is hidden. Typical use: pulled a new `0001_qtss_baseline.sql` without recreating the database.
pub async fn sync_sqlx_migration_checksums_from_disk(pool: &PgPool) -> Result<u64, StorageError> {
    let table_ok: bool = sqlx::query_scalar::<_, bool>(
        "SELECT to_regclass('public._sqlx_migrations') IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);
    if !table_ok {
        return Ok(0);
    }

    let dir = resolve_migrations_dir()?;
    let files = collect_migration_files(&dir)?;

    let mut updated = 0u64;
    for (version, path) in files {
        let sql = fs::read_to_string(&path).map_err(|e| {
            StorageError::Other(format!("read migration {}: {e}", path.display()))
        })?;
        let digest = Sha384::digest(sql.as_bytes());
        let checksum: &[u8] = digest.as_slice();

        let res = sqlx::query(r#"UPDATE _sqlx_migrations SET checksum = $1 WHERE version = $2"#)
            .bind(checksum)
            .bind(version)
            .execute(pool)
            .await
            .map_err(|e| StorageError::Other(format!("_sqlx_migrations checksum update: {e}")))?;

        updated += res.rows_affected();
    }

    Ok(updated)
}

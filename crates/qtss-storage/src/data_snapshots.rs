//! Unified `data_snapshots` — one row per `source_key` (F7).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DataSnapshotRow {
    pub source_key: String,
    pub request_json: JsonValue,
    pub response_json: Option<JsonValue>,
    pub meta_json: Option<JsonValue>,
    pub computed_at: DateTime<Utc>,
    pub error: Option<String>,
}

pub async fn upsert_data_snapshot(
    pool: &PgPool,
    source_key: &str,
    request_json: &JsonValue,
    response_json: Option<&JsonValue>,
    meta_json: Option<&JsonValue>,
    error: Option<&str>,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"INSERT INTO data_snapshots (
               source_key, request_json, response_json, meta_json, computed_at, error
           ) VALUES ($1, $2, $3, $4, now(), $5)
           ON CONFLICT (source_key) DO UPDATE SET
             request_json = EXCLUDED.request_json,
             response_json = EXCLUDED.response_json,
             meta_json = EXCLUDED.meta_json,
             computed_at = now(),
             error = EXCLUDED.error"#,
    )
    .bind(source_key)
    .bind(request_json)
    .bind(response_json)
    .bind(meta_json)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_data_snapshots(pool: &PgPool) -> Result<Vec<DataSnapshotRow>, StorageError> {
    let rows = sqlx::query_as::<_, DataSnapshotRow>(
        r#"SELECT source_key, request_json, response_json, meta_json, computed_at, error
           FROM data_snapshots ORDER BY source_key ASC"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn fetch_data_snapshot(
    pool: &PgPool,
    source_key: &str,
) -> Result<Option<DataSnapshotRow>, StorageError> {
    let row = sqlx::query_as::<_, DataSnapshotRow>(
        r#"SELECT source_key, request_json, response_json, meta_json, computed_at, error
           FROM data_snapshots WHERE source_key = $1"#,
    )
    .bind(source_key)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Son yazımın yaşı (sn); satır yoksa `None` — worker HTTP tick için.
pub async fn data_snapshot_age_secs(
    pool: &PgPool,
    source_key: &str,
) -> Result<Option<i64>, StorageError> {
    let t: Option<DateTime<Utc>> =
        sqlx::query_scalar(r#"SELECT computed_at FROM data_snapshots WHERE source_key = $1"#)
            .bind(source_key)
            .fetch_optional(pool)
            .await?;
    Ok(t.map(|at| Utc::now().signed_duration_since(at).num_seconds()))
}

/// Yalnızca `external_data_sources` içinde tanımlı anahtarlar (HTTP kaynakları).
pub async fn list_snapshots_for_external_http_sources(
    pool: &PgPool,
) -> Result<Vec<DataSnapshotRow>, StorageError> {
    let rows = sqlx::query_as::<_, DataSnapshotRow>(
        r#"SELECT d.source_key, d.request_json, d.response_json, d.meta_json, d.computed_at, d.error
           FROM data_snapshots d
           INNER JOIN external_data_sources e ON e.key = d.source_key
           ORDER BY d.source_key ASC"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Tek anahtar; kaynak `external_data_sources`’ta yoksa `None` (Nansen vb. ayrı uçlarda).
pub async fn fetch_data_snapshot_for_external_http_source(
    pool: &PgPool,
    source_key: &str,
) -> Result<Option<DataSnapshotRow>, StorageError> {
    let row = sqlx::query_as::<_, DataSnapshotRow>(
        r#"SELECT d.source_key, d.request_json, d.response_json, d.meta_json, d.computed_at, d.error
           FROM data_snapshots d
           WHERE d.source_key = $1
             AND EXISTS (SELECT 1 FROM external_data_sources e WHERE e.key = d.source_key)"#,
    )
    .bind(source_key)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

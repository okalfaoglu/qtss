//! Latest row per `snapshot_kind` for Nansen token screener (global, not per symbol).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct NansenSnapshotRow {
    pub snapshot_kind: String,
    pub request_json: JsonValue,
    pub response_json: Option<JsonValue>,
    pub meta_json: Option<JsonValue>,
    pub computed_at: DateTime<Utc>,
    pub error: Option<String>,
}

pub async fn upsert_nansen_snapshot(
    pool: &PgPool,
    snapshot_kind: &str,
    request_json: &JsonValue,
    response_json: Option<&JsonValue>,
    meta_json: Option<&JsonValue>,
    error: Option<&str>,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"INSERT INTO nansen_snapshots (
               snapshot_kind, request_json, response_json, meta_json, computed_at, error
           ) VALUES ($1, $2, $3, $4, now(), $5)
           ON CONFLICT (snapshot_kind) DO UPDATE SET
             request_json = EXCLUDED.request_json,
             response_json = EXCLUDED.response_json,
             meta_json = EXCLUDED.meta_json,
             computed_at = now(),
             error = EXCLUDED.error"#,
    )
    .bind(snapshot_kind)
    .bind(request_json)
    .bind(response_json)
    .bind(meta_json)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn fetch_nansen_snapshot(
    pool: &PgPool,
    snapshot_kind: &str,
) -> Result<Option<NansenSnapshotRow>, StorageError> {
    let row = sqlx::query_as::<_, NansenSnapshotRow>(
        r#"SELECT snapshot_kind, request_json, response_json, meta_json, computed_at, error
           FROM nansen_snapshots WHERE snapshot_kind = $1"#,
    )
    .bind(snapshot_kind)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

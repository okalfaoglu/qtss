//! `external_data_sources` tanımı + `external_data_snapshots` son yanıt (worker HTTP çekimi).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ExternalDataSourceRow {
    pub key: String,
    pub enabled: bool,
    pub method: String,
    pub url: String,
    pub headers_json: JsonValue,
    pub body_json: Option<JsonValue>,
    pub tick_secs: i32,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ExternalDataSnapshotRow {
    pub source_key: String,
    pub request_json: JsonValue,
    pub response_json: Option<JsonValue>,
    pub status_code: Option<i16>,
    pub computed_at: DateTime<Utc>,
    pub error: Option<String>,
}

pub async fn list_external_sources(pool: &PgPool) -> Result<Vec<ExternalDataSourceRow>, StorageError> {
    let rows = sqlx::query_as::<_, ExternalDataSourceRow>(
        r#"SELECT key, enabled, method, url, headers_json, body_json, tick_secs, description
           FROM external_data_sources ORDER BY key ASC"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_enabled_external_sources(
    pool: &PgPool,
) -> Result<Vec<ExternalDataSourceRow>, StorageError> {
    let rows = sqlx::query_as::<_, ExternalDataSourceRow>(
        r#"SELECT key, enabled, method, url, headers_json, body_json, tick_secs, description
           FROM external_data_sources WHERE enabled = true ORDER BY key ASC"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Snapshot yoksa `None` (hemen çekilmeli).
pub async fn external_snapshot_age_secs(
    pool: &PgPool,
    source_key: &str,
) -> Result<Option<i64>, StorageError> {
    let t: Option<DateTime<Utc>> = sqlx::query_scalar(
        r#"SELECT computed_at FROM external_data_snapshots WHERE source_key = $1"#,
    )
    .bind(source_key)
    .fetch_optional(pool)
    .await?;
    Ok(t.map(|at| Utc::now().signed_duration_since(at).num_seconds()))
}

pub async fn list_external_snapshots(
    pool: &PgPool,
) -> Result<Vec<ExternalDataSnapshotRow>, StorageError> {
    let rows = sqlx::query_as::<_, ExternalDataSnapshotRow>(
        r#"SELECT source_key, request_json, response_json, status_code, computed_at, error
           FROM external_data_snapshots ORDER BY source_key ASC"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn fetch_external_snapshot(
    pool: &PgPool,
    source_key: &str,
) -> Result<Option<ExternalDataSnapshotRow>, StorageError> {
    let row = sqlx::query_as::<_, ExternalDataSnapshotRow>(
        r#"SELECT source_key, request_json, response_json, status_code, computed_at, error
           FROM external_data_snapshots WHERE source_key = $1"#,
    )
    .bind(source_key)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn upsert_external_source(
    pool: &PgPool,
    key: &str,
    enabled: bool,
    method: &str,
    url: &str,
    headers_json: &JsonValue,
    body_json: Option<&JsonValue>,
    tick_secs: i32,
    description: Option<&str>,
) -> Result<ExternalDataSourceRow, StorageError> {
    let row = sqlx::query_as::<_, ExternalDataSourceRow>(
        r#"
        INSERT INTO external_data_sources (
            key, enabled, method, url, headers_json, body_json, tick_secs, description
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (key) DO UPDATE SET
            enabled = EXCLUDED.enabled,
            method = EXCLUDED.method,
            url = EXCLUDED.url,
            headers_json = EXCLUDED.headers_json,
            body_json = EXCLUDED.body_json,
            tick_secs = EXCLUDED.tick_secs,
            description = EXCLUDED.description,
            updated_at = now()
        RETURNING key, enabled, method, url, headers_json, body_json, tick_secs, description
        "#,
    )
    .bind(key)
    .bind(enabled)
    .bind(method)
    .bind(url)
    .bind(headers_json)
    .bind(body_json)
    .bind(tick_secs)
    .bind(description)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete_external_source(pool: &PgPool, key: &str) -> Result<u64, StorageError> {
    let res = sqlx::query(r#"DELETE FROM external_data_sources WHERE key = $1"#)
        .bind(key)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

pub async fn upsert_external_snapshot(
    pool: &PgPool,
    source_key: &str,
    request_json: &JsonValue,
    response_json: Option<&JsonValue>,
    status_code: Option<i16>,
    error: Option<&str>,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"INSERT INTO external_data_snapshots (
               source_key, request_json, response_json, status_code, computed_at, error
           ) VALUES ($1, $2, $3, $4, now(), $5)
           ON CONFLICT (source_key) DO UPDATE SET
             request_json = EXCLUDED.request_json,
             response_json = EXCLUDED.response_json,
             status_code = EXCLUDED.status_code,
             computed_at = now(),
             error = EXCLUDED.error"#,
    )
    .bind(source_key)
    .bind(request_json)
    .bind(response_json)
    .bind(status_code)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

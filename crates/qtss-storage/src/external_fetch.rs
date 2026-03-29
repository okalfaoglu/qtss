//! `external_data_sources` — worker HTTP çekim tanımları. Son yanıtlar yalnızca `data_snapshots` içinde.

use serde_json::Value as JsonValue;
use sqlx::PgPool;

use crate::data_snapshots::data_snapshot_age_secs;
use crate::error::StorageError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
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

/// `data_snapshots.computed_at` yaşı — önceden `external_data_snapshots` idi.
pub async fn external_snapshot_age_secs(
    pool: &PgPool,
    source_key: &str,
) -> Result<Option<i64>, StorageError> {
    data_snapshot_age_secs(pool, source_key).await
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
    sqlx::query(r#"DELETE FROM data_snapshots WHERE source_key = $1"#)
        .bind(key)
        .execute(pool)
        .await?;
    let res = sqlx::query(r#"DELETE FROM external_data_sources WHERE key = $1"#)
        .bind(key)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

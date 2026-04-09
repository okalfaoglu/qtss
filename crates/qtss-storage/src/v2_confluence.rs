//! Faz 7.8 — `qtss_v2_confluence` repo.
//!
//! One row per `(exchange, symbol, timeframe, computed_at)` produced
//! by the `v2_confluence_loop`. The Setup Engine (Faz 8.0) reads the
//! latest row per `(symbol, timeframe)` and gates on
//! `guven >= threshold` before arming any setup.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct V2ConfluenceRow {
    pub id: Uuid,
    pub computed_at: DateTime<Utc>,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub erken_uyari: f32,
    pub guven: f32,
    pub direction: String,
    pub layer_count: i32,
    pub raw_meta: JsonValue,
}

#[derive(Debug, Clone)]
pub struct V2ConfluenceInsert {
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub erken_uyari: f32,
    pub guven: f32,
    pub direction: String,
    pub layer_count: i32,
    pub raw_meta: JsonValue,
}

pub async fn insert_v2_confluence(
    pool: &PgPool,
    row: &V2ConfluenceInsert,
) -> Result<Uuid, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO qtss_v2_confluence (
            exchange, symbol, timeframe,
            erken_uyari, guven, direction, layer_count, raw_meta
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
        RETURNING id
        "#,
    )
    .bind(&row.exchange)
    .bind(&row.symbol)
    .bind(&row.timeframe)
    .bind(row.erken_uyari)
    .bind(row.guven)
    .bind(&row.direction)
    .bind(row.layer_count)
    .bind(&row.raw_meta)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn fetch_latest_v2_confluence(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
) -> Result<Option<V2ConfluenceRow>, StorageError> {
    let row = sqlx::query_as::<_, V2ConfluenceRow>(
        r#"SELECT id, computed_at, exchange, symbol, timeframe,
                  erken_uyari, guven, direction, layer_count, raw_meta
             FROM qtss_v2_confluence
            WHERE exchange = $1 AND symbol = $2 AND timeframe = $3
            ORDER BY computed_at DESC
            LIMIT 1"#,
    )
    .bind(exchange)
    .bind(symbol)
    .bind(timeframe)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Latest row across every (symbol, timeframe). Used by `GET /v2/confluence`
/// to populate the Q-RADAR overview without N+1 queries.
pub async fn list_latest_v2_confluence(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<V2ConfluenceRow>, StorageError> {
    let rows = sqlx::query_as::<_, V2ConfluenceRow>(
        r#"SELECT DISTINCT ON (exchange, symbol, timeframe)
                  id, computed_at, exchange, symbol, timeframe,
                  erken_uyari, guven, direction, layer_count, raw_meta
             FROM qtss_v2_confluence
            ORDER BY exchange, symbol, timeframe, computed_at DESC
            LIMIT $1"#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

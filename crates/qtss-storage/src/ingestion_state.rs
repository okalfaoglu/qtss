//! `engine_symbol_ingestion_state` — worker-written health for each `engine_symbols` series.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct EngineSymbolIngestionJoinedRow {
    pub id: Uuid,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
    pub enabled: bool,
    pub sort_order: i32,
    pub label: Option<String>,
    pub bar_row_count: Option<i32>,
    pub min_open_time: Option<DateTime<Utc>>,
    pub max_open_time: Option<DateTime<Utc>>,
    pub gap_count: Option<i32>,
    pub max_gap_seconds: Option<i32>,
    pub last_backfill_at: Option<DateTime<Utc>>,
    pub last_health_check_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub ingestion_updated_at: Option<DateTime<Utc>>,
}

pub async fn count_market_bars_series(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    interval: &str,
) -> Result<(i64, Option<DateTime<Utc>>, Option<DateTime<Utc>>), StorageError> {
    let row: (i64, Option<DateTime<Utc>>, Option<DateTime<Utc>>) = sqlx::query_as(
        r#"SELECT COUNT(*)::bigint,
                  MIN(open_time),
                  MAX(open_time)
           FROM market_bars
           WHERE LOWER(BTRIM(exchange)) = LOWER(BTRIM($1))
             AND LOWER(BTRIM(segment)) = LOWER(BTRIM($2))
             AND BTRIM(symbol) = BTRIM($3)
             AND BTRIM(interval) = BTRIM($4)"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(interval)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Newest-first open times (for gap scan on the trailing window).
pub async fn list_recent_bar_open_times_desc(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    interval: &str,
    limit: i64,
) -> Result<Vec<DateTime<Utc>>, StorageError> {
    let lim = limit.clamp(1, 20_000);
    let rows = sqlx::query_scalar::<_, DateTime<Utc>>(
        r#"SELECT open_time FROM market_bars
           WHERE LOWER(BTRIM(exchange)) = LOWER(BTRIM($1))
             AND LOWER(BTRIM(segment)) = LOWER(BTRIM($2))
             AND BTRIM(symbol) = BTRIM($3)
             AND BTRIM(interval) = BTRIM($4)
           ORDER BY open_time DESC
           LIMIT $5"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(interval)
    .bind(lim)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn upsert_engine_symbol_ingestion_state(
    pool: &PgPool,
    engine_symbol_id: Uuid,
    bar_row_count: i32,
    min_open_time: Option<DateTime<Utc>>,
    max_open_time: Option<DateTime<Utc>>,
    gap_count: i32,
    max_gap_seconds: Option<i32>,
    last_backfill_at: Option<DateTime<Utc>>,
    last_health_check_at: DateTime<Utc>,
    last_error: Option<&str>,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"INSERT INTO engine_symbol_ingestion_state (
               engine_symbol_id, bar_row_count, min_open_time, max_open_time,
               gap_count, max_gap_seconds, last_backfill_at, last_health_check_at,
               last_error, updated_at
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now())
           ON CONFLICT (engine_symbol_id) DO UPDATE SET
               bar_row_count = EXCLUDED.bar_row_count,
               min_open_time = EXCLUDED.min_open_time,
               max_open_time = EXCLUDED.max_open_time,
               gap_count = EXCLUDED.gap_count,
               max_gap_seconds = EXCLUDED.max_gap_seconds,
               last_backfill_at = COALESCE(EXCLUDED.last_backfill_at, engine_symbol_ingestion_state.last_backfill_at),
               last_health_check_at = EXCLUDED.last_health_check_at,
               last_error = EXCLUDED.last_error,
               updated_at = now()"#,
    )
    .bind(engine_symbol_id)
    .bind(bar_row_count)
    .bind(min_open_time)
    .bind(max_open_time)
    .bind(gap_count)
    .bind(max_gap_seconds)
    .bind(last_backfill_at)
    .bind(last_health_check_at)
    .bind(last_error)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_engine_symbols_with_ingestion(
    pool: &PgPool,
) -> Result<Vec<EngineSymbolIngestionJoinedRow>, StorageError> {
    let rows = sqlx::query_as::<_, EngineSymbolIngestionJoinedRow>(
        r#"SELECT
             e.id,
             e.exchange,
             e.segment,
             e.symbol,
             e.interval,
             e.enabled,
             e.sort_order,
             e.label,
             i.bar_row_count,
             i.min_open_time,
             i.max_open_time,
             i.gap_count,
             i.max_gap_seconds,
             i.last_backfill_at,
             i.last_health_check_at,
             i.last_error,
             i.updated_at AS ingestion_updated_at
           FROM engine_symbols e
           LEFT JOIN engine_symbol_ingestion_state i ON i.engine_symbol_id = e.id
           ORDER BY e.enabled DESC, e.sort_order ASC, e.symbol ASC, e.interval ASC"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

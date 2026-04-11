//! CRUD for `pivot_cache` — pre-computed pivot points from historical bars.
//!
//! The pivot cache eliminates redundant re-computation: the detection
//! orchestrator reads cached pivots instead of rebuilding the full
//! PivotTree from bars on every tick. Only new (uncached) bars need
//! pivot extraction.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;

/// Row shape for upserting into `pivot_cache`.
#[derive(Debug, Clone)]
pub struct PivotCacheRow {
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub level: String,
    pub bar_index: i64,
    pub open_time: DateTime<Utc>,
    pub price: Decimal,
    pub kind: String,
    pub prominence: Decimal,
    pub volume_at_pivot: Decimal,
    pub swing_type: Option<String>,
}

/// Upsert a single pivot into the cache (idempotent).
pub async fn upsert_pivot_cache(pool: &PgPool, row: &PivotCacheRow) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO pivot_cache
            (exchange, symbol, timeframe, level, bar_index, open_time,
             price, kind, prominence, volume_at_pivot, swing_type, computed_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, now())
        ON CONFLICT (exchange, symbol, timeframe, level, bar_index)
        DO UPDATE SET
            price = EXCLUDED.price,
            kind = EXCLUDED.kind,
            prominence = EXCLUDED.prominence,
            volume_at_pivot = EXCLUDED.volume_at_pivot,
            swing_type = EXCLUDED.swing_type,
            computed_at = now()
        "#,
    )
    .bind(&row.exchange)
    .bind(&row.symbol)
    .bind(&row.timeframe)
    .bind(&row.level)
    .bind(row.bar_index)
    .bind(row.open_time)
    .bind(row.price)
    .bind(&row.kind)
    .bind(row.prominence)
    .bind(row.volume_at_pivot)
    .bind(&row.swing_type)
    .execute(pool)
    .await?;
    Ok(())
}

/// Batch upsert pivots (uses a transaction for performance).
pub async fn upsert_pivot_cache_batch(
    pool: &PgPool,
    rows: &[PivotCacheRow],
) -> Result<u64, sqlx::Error> {
    if rows.is_empty() {
        return Ok(0);
    }
    let mut tx = pool.begin().await?;
    let mut count = 0u64;
    for row in rows {
        sqlx::query(
            r#"
            INSERT INTO pivot_cache
                (exchange, symbol, timeframe, level, bar_index, open_time,
                 price, kind, prominence, volume_at_pivot, swing_type, computed_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, now())
            ON CONFLICT (exchange, symbol, timeframe, level, bar_index)
            DO UPDATE SET
                price = EXCLUDED.price,
                kind = EXCLUDED.kind,
                prominence = EXCLUDED.prominence,
                volume_at_pivot = EXCLUDED.volume_at_pivot,
                swing_type = EXCLUDED.swing_type,
                computed_at = now()
            "#,
        )
        .bind(&row.exchange)
        .bind(&row.symbol)
        .bind(&row.timeframe)
        .bind(&row.level)
        .bind(row.bar_index)
        .bind(row.open_time)
        .bind(row.price)
        .bind(&row.kind)
        .bind(row.prominence)
        .bind(row.volume_at_pivot)
        .bind(&row.swing_type)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }
    tx.commit().await?;
    Ok(count)
}

/// Read cached pivots for a series, ordered by bar_index ascending.
pub async fn list_pivot_cache(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
    level: &str,
    limit: i64,
) -> Result<Vec<PivotCacheRow>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, String, String, String, i64, DateTime<Utc>, Decimal, String, Decimal, Decimal, Option<String>)>(
        r#"
        SELECT exchange, symbol, timeframe, level, bar_index, open_time,
               price, kind, prominence, volume_at_pivot, swing_type
        FROM pivot_cache
        WHERE exchange = $1 AND symbol = $2 AND timeframe = $3 AND level = $4
        ORDER BY bar_index ASC
        LIMIT $5
        "#,
    )
    .bind(exchange)
    .bind(symbol)
    .bind(timeframe)
    .bind(level)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(exchange, symbol, timeframe, level, bar_index, open_time, price, kind, prominence, volume_at_pivot, swing_type)| {
            PivotCacheRow {
                exchange,
                symbol,
                timeframe,
                level,
                bar_index,
                open_time,
                price,
                kind,
                prominence,
                volume_at_pivot,
                swing_type,
            }
        })
        .collect())
}

/// Get the highest bar_index in cache for a series/level.
/// Returns None if the cache is empty for this series.
pub async fn max_cached_bar_index(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
    level: &str,
) -> Result<Option<i64>, sqlx::Error> {
    let row: (Option<i64>,) = sqlx::query_as(
        r#"
        SELECT MAX(bar_index)
        FROM pivot_cache
        WHERE exchange = $1 AND symbol = $2 AND timeframe = $3 AND level = $4
        "#,
    )
    .bind(exchange)
    .bind(symbol)
    .bind(timeframe)
    .bind(level)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Count cached pivots for a series/level.
pub async fn count_pivot_cache(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
    level: &str,
) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM pivot_cache
        WHERE exchange = $1 AND symbol = $2 AND timeframe = $3 AND level = $4
        "#,
    )
    .bind(exchange)
    .bind(symbol)
    .bind(timeframe)
    .bind(level)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

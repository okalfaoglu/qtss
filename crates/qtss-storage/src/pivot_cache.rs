//! Thin read-shim over the canonical `pivots` table (+ `engine_symbols`
//! for key translation), preserving the `PivotCacheRow` row shape and
//! call signatures used by the orchestrator, API, and backtest sweeps.
//!
//! The old `pivot_cache` physical table has been dropped — everything
//! now reads the LuxAlgo zigzag output written by `pivot_writer_loop`.
//! Key translation:
//!
//!   (exchange, symbol, timeframe) + level text "L0".."L4"
//!      ↕
//!   engine_symbols.id (+ matching segment) × pivots.level SMALLINT 0..4
//!
//! Direction: `pivots.direction` is +1 / -1; this shim maps it back to
//! "High" / "Low" strings so the call sites that still key on text kind
//! don't have to change in this patch.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;

/// Legacy row shape. Only the read path is preserved; there is no
/// upsert — the `pivot_writer_loop` owns all writes to the underlying
/// `pivots` table.
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

/// Parse legacy level string "L0".."L4" to SMALLINT 0..4.
/// Unknown strings map to `None` — callers get an empty result, matching
/// the pre-migration "no rows for that level" behaviour.
fn level_to_smallint(level: &str) -> Option<i16> {
    match level {
        "L0" => Some(0),
        "L1" => Some(1),
        "L2" => Some(2),
        "L3" => Some(3),
        "L4" => Some(4),
        _ => None,
    }
}

fn smallint_to_level(level: i16) -> String {
    format!("L{level}")
}

fn direction_to_kind(direction: i16) -> String {
    if direction >= 1 { "High".to_string() } else { "Low".to_string() }
}

/// Read pivots for a series + level, in ascending bar_index order.
/// Segment defaults to `futures` — matches every call site that
/// previously assumed USDT-M futures. Spot-aware callers should switch
/// to a segment-explicit helper once one exists.
pub async fn list_pivot_cache(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
    level: &str,
    limit: i64,
) -> Result<Vec<PivotCacheRow>, sqlx::Error> {
    let Some(level_i) = level_to_smallint(level) else {
        return Ok(Vec::new());
    };
    let rows = sqlx::query_as::<
        _,
        (i64, DateTime<Utc>, Decimal, i16, Decimal, Decimal, Option<String>),
    >(
        r#"
        SELECT p.bar_index, p.open_time, p.price, p.direction,
               p.prominence, p.volume, p.swing_tag
          FROM pivots p
          JOIN engine_symbols es ON es.id = p.engine_symbol_id
         WHERE es.exchange   = $1
           AND es.symbol     = $2
           AND es."interval" = $3
           AND p.level       = $4
         ORDER BY p.bar_index ASC
         LIMIT $5
        "#,
    )
    .bind(exchange)
    .bind(symbol)
    .bind(timeframe)
    .bind(level_i)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(bar_index, open_time, price, direction, prominence, volume, swing_tag)| {
            PivotCacheRow {
                exchange: exchange.to_string(),
                symbol: symbol.to_string(),
                timeframe: timeframe.to_string(),
                level: smallint_to_level(level_i),
                bar_index,
                open_time,
                price,
                kind: direction_to_kind(direction),
                prominence,
                volume_at_pivot: volume,
                swing_type: swing_tag,
            }
        })
        .collect())
}

/// Max `bar_index` on a series/level. Returns `None` when no pivots
/// exist yet — used by the live orchestrator as a watermark for the
/// retired pivot_cache write path (kept for API compatibility; the
/// writer loop is now authoritative).
pub async fn max_cached_bar_index(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
    level: &str,
) -> Result<Option<i64>, sqlx::Error> {
    let Some(level_i) = level_to_smallint(level) else {
        return Ok(None);
    };
    let row: (Option<i64>,) = sqlx::query_as(
        r#"
        SELECT MAX(p.bar_index)
          FROM pivots p
          JOIN engine_symbols es ON es.id = p.engine_symbol_id
         WHERE es.exchange   = $1
           AND es.symbol     = $2
           AND es."interval" = $3
           AND p.level       = $4
        "#,
    )
    .bind(exchange)
    .bind(symbol)
    .bind(timeframe)
    .bind(level_i)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Count pivots for a series/level.
pub async fn count_pivot_cache(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
    level: &str,
) -> Result<i64, sqlx::Error> {
    let Some(level_i) = level_to_smallint(level) else {
        return Ok(0);
    };
    let row: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
          FROM pivots p
          JOIN engine_symbols es ON es.id = p.engine_symbol_id
         WHERE es.exchange   = $1
           AND es.symbol     = $2
           AND es."interval" = $3
           AND p.level       = $4
        "#,
    )
    .bind(exchange)
    .bind(symbol)
    .bind(timeframe)
    .bind(level_i)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

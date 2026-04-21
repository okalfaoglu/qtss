//! `market_bars_open` — one row per series for the currently forming kline.
//!
//! The worker upserts this table on every unclosed WebSocket frame so the
//! API can paint a live candle on the chart without waiting for the bar to
//! close. When Binance marks the kline final (`k.x == true`), the archive
//! write lands in [`crate::market_bars`] and the next frame simply
//! overwrites the single row here with the new open bar — no growth.
//!
//! Schema is intentionally symmetric to `market_bars` minus the UUID keys;
//! only (exchange, segment, symbol, interval) are needed to identify the
//! series.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct OpenBarRow {
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
    pub open_time: DateTime<Utc>,
    pub close_time: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub trade_count: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct OpenBarUpsert {
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
    pub open_time: DateTime<Utc>,
    pub close_time: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub trade_count: i64,
}

/// Overwrite the live open bar for one series. Called on every non-final
/// WebSocket frame. `(exchange, segment, symbol, interval)` is the primary
/// key — always one row, always the latest.
pub async fn upsert_open_bar(pool: &PgPool, b: &OpenBarUpsert) -> Result<(), StorageError> {
    sqlx::query(
        r#"INSERT INTO market_bars_open (
               exchange, segment, symbol, interval,
               open_time, close_time,
               open, high, low, close, volume, trade_count
           ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
           ON CONFLICT (exchange, segment, symbol, interval) DO UPDATE SET
             open_time   = EXCLUDED.open_time,
             close_time  = EXCLUDED.close_time,
             open        = EXCLUDED.open,
             high        = EXCLUDED.high,
             low         = EXCLUDED.low,
             close       = EXCLUDED.close,
             volume      = EXCLUDED.volume,
             trade_count = EXCLUDED.trade_count,
             updated_at  = now()"#,
    )
    .bind(&b.exchange)
    .bind(&b.segment)
    .bind(&b.symbol)
    .bind(&b.interval)
    .bind(b.open_time)
    .bind(b.close_time)
    .bind(b.open)
    .bind(b.high)
    .bind(b.low)
    .bind(b.close)
    .bind(b.volume)
    .bind(b.trade_count)
    .execute(pool)
    .await?;
    Ok(())
}

/// Fetch the live open bar for one series if present. Callers should
/// compare `open_time` to the latest row in `market_bars` before merging
/// — the row may briefly lag the archive by one tick when a bar just
/// closed and the next frame hasn't arrived yet.
pub async fn get_open_bar(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    interval: &str,
) -> Result<Option<OpenBarRow>, StorageError> {
    let row = sqlx::query_as::<_, OpenBarRow>(
        r#"SELECT exchange, segment, symbol, interval,
                  open_time, close_time,
                  open, high, low, close, volume, trade_count, updated_at
             FROM market_bars_open
            WHERE exchange = $1 AND segment = $2
              AND symbol   = $3 AND interval = $4"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(interval)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

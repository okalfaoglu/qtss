//! `market_bars` — OHLCV serileri.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MarketBarRow {
    pub id: Uuid,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
    pub open_time: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub quote_volume: Option<Decimal>,
    pub trade_count: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct MarketBarUpsert {
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
    pub open_time: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub quote_volume: Option<Decimal>,
    pub trade_count: Option<i64>,
}

pub async fn upsert_market_bar(pool: &PgPool, b: &MarketBarUpsert) -> Result<(), StorageError> {
    sqlx::query(
        r#"INSERT INTO market_bars (
               exchange, segment, symbol, interval, open_time,
               open, high, low, close, volume, quote_volume, trade_count
           ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
           ON CONFLICT (exchange, segment, symbol, interval, open_time) DO UPDATE SET
             open = EXCLUDED.open,
             high = EXCLUDED.high,
             low = EXCLUDED.low,
             close = EXCLUDED.close,
             volume = EXCLUDED.volume,
             quote_volume = EXCLUDED.quote_volume,
             trade_count = EXCLUDED.trade_count,
             updated_at = now()"#,
    )
    .bind(&b.exchange)
    .bind(&b.segment)
    .bind(&b.symbol)
    .bind(&b.interval)
    .bind(b.open_time)
    .bind(b.open)
    .bind(b.high)
    .bind(b.low)
    .bind(b.close)
    .bind(b.volume)
    .bind(b.quote_volume)
    .bind(b.trade_count)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_recent_bars(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    interval: &str,
    limit: i64,
) -> Result<Vec<MarketBarRow>, StorageError> {
    let rows = sqlx::query_as::<_, MarketBarRow>(
        r#"SELECT id, exchange, segment, symbol, interval, open_time,
                  open, high, low, close, volume, quote_volume, trade_count,
                  created_at, updated_at
           FROM market_bars
           WHERE exchange = $1 AND segment = $2 AND symbol = $3 AND interval = $4
           ORDER BY open_time DESC
           LIMIT $5"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(interval)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

//! `market_bars` — OHLCV serileri.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::catalog::resolve_series_catalog_ids;
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
    pub instrument_id: Option<Uuid>,
    pub bar_interval_id: Option<Uuid>,
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
    pub instrument_id: Option<Uuid>,
    pub bar_interval_id: Option<Uuid>,
}

pub async fn upsert_market_bar(pool: &PgPool, b: &MarketBarUpsert) -> Result<(), StorageError> {
    let ids =
        resolve_series_catalog_ids(pool, &b.exchange, &b.segment, &b.symbol, &b.interval).await?;
    let inst_id = b.instrument_id.or(ids.instrument_id);
    let int_id = b.bar_interval_id.or(ids.bar_interval_id);
    sqlx::query(
        r#"INSERT INTO market_bars (
               exchange, segment, symbol, interval, open_time,
               open, high, low, close, volume, quote_volume, trade_count,
               instrument_id, bar_interval_id
           ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
           ON CONFLICT (exchange, segment, symbol, interval, open_time) DO UPDATE SET
             open = EXCLUDED.open,
             high = EXCLUDED.high,
             low = EXCLUDED.low,
             close = EXCLUDED.close,
             volume = EXCLUDED.volume,
             quote_volume = EXCLUDED.quote_volume,
             trade_count = EXCLUDED.trade_count,
             instrument_id = COALESCE(EXCLUDED.instrument_id, market_bars.instrument_id),
             bar_interval_id = COALESCE(EXCLUDED.bar_interval_id, market_bars.bar_interval_id),
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
    .bind(inst_id)
    .bind(int_id)
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
                  instrument_id, bar_interval_id, created_at, updated_at
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

pub async fn list_bars_in_range(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    interval: &str,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<MarketBarRow>, StorageError> {
    let rows = sqlx::query_as::<_, MarketBarRow>(
        r#"SELECT id, exchange, segment, symbol, interval, open_time,
                  open, high, low, close, volume, quote_volume, trade_count,
                  instrument_id, bar_interval_id, created_at, updated_at
           FROM market_bars
           WHERE exchange = $1 AND segment = $2 AND symbol = $3 AND interval = $4
             AND open_time >= $5 AND open_time <= $6
           ORDER BY open_time ASC
           LIMIT $7"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(interval)
    .bind(start_time)
    .bind(end_time)
    .bind(limit.clamp(1, 200_000))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Summary stats over the last `limit` bars (newest first), for AI context / dashboards (FAZ 3.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentBarsStats {
    pub bar_count: usize,
    pub last_close: Option<Decimal>,
    pub oldest_close_in_window: Option<Decimal>,
    pub approx_change_over_window_pct: f64,
    pub high_low_range_pct_of_mean_close: f64,
    pub last_open_time: Option<DateTime<Utc>>,
}

pub async fn fetch_recent_bars_stats(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    interval: &str,
    limit: i64,
) -> Result<Option<RecentBarsStats>, StorageError> {
    use rust_decimal::prelude::ToPrimitive;

    let bars = list_recent_bars(pool, exchange, segment, symbol, interval, limit).await?;
    if bars.is_empty() {
        return Ok(None);
    }
    let mut highs = Vec::new();
    let mut lows = Vec::new();
    let mut closes = Vec::new();
    for b in &bars {
        highs.push(b.high.to_f64().unwrap_or(0.0));
        lows.push(b.low.to_f64().unwrap_or(0.0));
        closes.push(b.close.to_f64().unwrap_or(0.0));
    }
    let last_close = bars.first().map(|b| b.close);
    let oldest_close = bars.last().map(|b| b.close);
    let last_close_f = closes.first().copied().unwrap_or(0.0);
    let oldest_close_f = closes.last().copied().unwrap_or(last_close_f);
    let pct_change = if oldest_close_f.abs() > f64::EPSILON {
        ((last_close_f - oldest_close_f) / oldest_close_f) * 100.0
    } else {
        0.0
    };
    let high_max = highs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let low_min = lows.iter().copied().fold(f64::INFINITY, f64::min);
    let mean_close = closes.iter().sum::<f64>() / closes.len().max(1) as f64;
    let range_pct = if mean_close.abs() > f64::EPSILON {
        ((high_max - low_min) / mean_close) * 100.0
    } else {
        0.0
    };
    Ok(Some(RecentBarsStats {
        bar_count: bars.len(),
        last_close,
        oldest_close_in_window: oldest_close,
        approx_change_over_window_pct: pct_change,
        high_low_range_pct_of_mean_close: range_pct,
        last_open_time: bars.first().map(|b| b.open_time),
    }))
}

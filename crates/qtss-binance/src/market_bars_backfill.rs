//! Public REST klines → `market_bars` (shared by API backfill and worker ingest).
//!
//! Supports resumable backfill: pass `resume_end_ms` to continue from where
//! a previous run was interrupted. Each page is committed to DB immediately
//! so no work is lost on crash.

use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::str::FromStr;

use crate::config::BinanceClientConfig;
use crate::klines::parse_klines_json;
use crate::{BinanceClient, BinanceError};
use qtss_storage::{upsert_market_bar, MarketBarUpsert};

/// Result of a backfill run — includes cursor position for resume.
#[derive(Debug, Clone)]
pub struct BackfillResult {
    /// Total bars upserted in this run.
    pub upserted: i64,
    /// Pages fetched in this run.
    pub pages: u32,
    /// Oldest open_time fetched (millis) — use as resume cursor.
    pub oldest_ms: Option<u64>,
    /// Newest open_time fetched (millis).
    pub newest_ms: Option<u64>,
    /// True if we hit the listing date (no more data available).
    pub reached_listing: bool,
}

/// Upsert up to `target_bars` older klines for Binance spot or USDT-M futures.
///
/// When `target_bars == 0`, fetches the **entire history** from the
/// exchange's listing date to now ("full backfill" mode).
///
/// **Resume support**: pass `resume_end_ms` to start fetching from that
/// timestamp backwards. This is the `oldest_ms` from a previous run.
/// Pass `None` to start from now.
pub async fn backfill_binance_public_klines(
    pool: &PgPool,
    symbol_upper: &str,
    interval: &str,
    segment_ui: &str,
    target_bars: i64,
    resume_end_ms: Option<u64>,
) -> Result<BackfillResult, BinanceError> {
    let sym = symbol_upper.trim().to_uppercase();
    if sym.is_empty() {
        return Err(BinanceError::Other("symbol required".into()));
    }
    let interval = interval.trim();
    if interval.is_empty() {
        return Err(BinanceError::Other("interval required".into()));
    }
    let seg_db = match segment_ui.trim() {
        "future" | "futures" | "usdt_futures" | "fapi" => "futures",
        _ => "spot",
    };
    let unlimited = target_bars == 0;
    let target = if unlimited { i64::MAX } else { target_bars.clamp(1, 50_000) };
    let max_pages: u32 = if unlimited { 5_000 } else { 60 };
    const PAGE: u32 = 1000;

    let cfg = BinanceClientConfig::public_mainnet();
    let client = BinanceClient::new(cfg)?;

    let mut upserted = 0_i64;
    let mut end_time: Option<u64> = resume_end_ms;
    let mut pages = 0_u32;
    let mut oldest_ms: Option<u64> = None;
    let mut newest_ms: Option<u64> = None;

    while upserted < target && pages < max_pages {
        pages += 1;
        let need = (target - upserted) as u32;
        let batch_lim = need.min(PAGE);
        let raw = match seg_db {
            "futures" => client
                .fapi_klines(&sym, interval, None, end_time, Some(batch_lim))
                .await?,
            _ => client
                .spot_klines(&sym, interval, None, end_time, Some(batch_lim))
                .await?,
        };
        let klines = parse_klines_json(&raw)?;
        if klines.is_empty() {
            return Ok(BackfillResult {
                upserted,
                pages,
                oldest_ms,
                newest_ms,
                reached_listing: true,
            });
        }
        let batch_oldest = klines.iter().map(|b| b.open_time).min().unwrap_or(0);
        let batch_newest = klines.iter().map(|b| b.open_time).max().unwrap_or(0);

        // Track global cursors
        oldest_ms = Some(oldest_ms.map_or(batch_oldest, |o: u64| o.min(batch_oldest)));
        newest_ms = Some(newest_ms.map_or(batch_newest, |n: u64| n.max(batch_newest)));

        for b in &klines {
            let open_time = Utc
                .timestamp_millis_opt(b.open_time as i64)
                .single()
                .ok_or_else(|| {
                    BinanceError::Other(format!("invalid open_time: {}", b.open_time))
                })?;
            let quote_volume = if b.quote_asset_volume.trim().is_empty() {
                None
            } else {
                Some(
                    Decimal::from_str(b.quote_asset_volume.trim())
                        .map_err(|e| BinanceError::Other(e.to_string()))?,
                )
            };
            let row = MarketBarUpsert {
                exchange: "binance".into(),
                segment: seg_db.into(),
                symbol: sym.clone(),
                interval: interval.to_string(),
                open_time,
                open: Decimal::from_str(b.open.trim())
                    .map_err(|e| BinanceError::Other(e.to_string()))?,
                high: Decimal::from_str(b.high.trim())
                    .map_err(|e| BinanceError::Other(e.to_string()))?,
                low: Decimal::from_str(b.low.trim())
                    .map_err(|e| BinanceError::Other(e.to_string()))?,
                close: Decimal::from_str(b.close.trim())
                    .map_err(|e| BinanceError::Other(e.to_string()))?,
                volume: Decimal::from_str(b.volume.trim())
                    .map_err(|e| BinanceError::Other(e.to_string()))?,
                quote_volume,
                trade_count: Some(b.number_of_trades as i64),
                instrument_id: None,
                bar_interval_id: None,
            };
            upsert_market_bar(pool, &row)
                .await
                .map_err(|e| BinanceError::Other(e.to_string()))?;
            upserted += 1;
            if upserted >= target {
                break;
            }
        }
        let reached_listing = klines.len() < batch_lim as usize;
        if reached_listing {
            return Ok(BackfillResult {
                upserted,
                pages,
                oldest_ms,
                newest_ms,
                reached_listing: true,
            });
        }
        end_time = Some(batch_oldest.saturating_sub(1));

        // Rate-limit courtesy
        if unlimited && pages % 10 == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    Ok(BackfillResult {
        upserted,
        pages,
        oldest_ms,
        newest_ms,
        reached_listing: false,
    })
}

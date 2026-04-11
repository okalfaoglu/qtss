//! Public REST klines → `market_bars` (shared by API backfill and worker ingest).

use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::str::FromStr;

use crate::config::BinanceClientConfig;
use crate::klines::parse_klines_json;
use crate::{BinanceClient, BinanceError};
use qtss_storage::{upsert_market_bar, MarketBarUpsert};

/// Upsert up to `target_bars` older klines for Binance spot or USDT-M futures (newest-first paging).
///
/// When `target_bars == 0`, fetches the **entire history** from the
/// exchange's listing date to now. This is the "full backfill" mode used
/// by `engine_ingest` when `engine_ingest_full_history` is enabled.
/// Pages are capped at 500 to avoid hitting Binance rate limits.
pub async fn backfill_binance_public_klines(
    pool: &PgPool,
    symbol_upper: &str,
    interval: &str,
    segment_ui: &str,
    target_bars: i64,
) -> Result<i64, BinanceError> {
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
    // target=0 means "fetch everything" — no upper bound on pages.
    let unlimited = target_bars == 0;
    let target = if unlimited { i64::MAX } else { target_bars.clamp(1, 50_000) };
    let max_pages: u32 = if unlimited { 5_000 } else { 60 };
    const PAGE: u32 = 1000;

    let cfg = BinanceClientConfig::public_mainnet();
    let client = BinanceClient::new(cfg)?;

    let mut upserted = 0_i64;
    let mut end_time: Option<u64> = None;
    let mut pages = 0_u32;

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
            break;
        }
        let oldest = klines.iter().map(|b| b.open_time).min().unwrap_or(0);
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
        if klines.len() < batch_lim as usize {
            // Exchange returned fewer than requested — we've hit the
            // listing date; no more data available.
            break;
        }
        end_time = Some(oldest.saturating_sub(1));

        // Rate-limit courtesy: brief pause every 10 pages in full-history
        // mode to avoid 429s from Binance.
        if unlimited && pages % 10 == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    Ok(upserted)
}

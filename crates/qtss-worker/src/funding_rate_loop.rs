//! Binance USDT-M perpetual funding rate ingest loop.
//!
//! Polls Binance funding-rate REST endpoint every 30 minutes and
//! upserts the latest funding period reading into
//! `external_snapshots`. The `qtss-backtest` `funding_oi_signals`
//! scorer reads from this table to detect funding extremes —
//! sustained positive funding signals over-leveraged longs
//! (bearish edge), sustained negative funding signals
//! over-leveraged shorts (bullish edge).
//!
//! No API key needed — Binance funding-rate endpoint is public.
//! Endpoint: GET /fapi/v1/fundingRate?symbol={symbol}&limit=1
//! Returns the most recent 8-hour funding period's rate.
//!
//! Per-symbol fan-out: read enabled futures symbols from
//! engine_symbols, hit the endpoint sequentially with a small
//! delay to stay under Binance's 2400 req/min rate limit.

use anyhow::Context;
use chrono::DateTime;
use reqwest::Client;
use serde::Deserialize;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

const POLL_INTERVAL_SECS: u64 = 1_800; // 30 minutes
const REQUEST_TIMEOUT_SECS: u64 = 10;
/// Pause between symbol fetches so a 100-symbol fan-out doesn't
/// burst the Binance rate limit.
const PER_SYMBOL_DELAY_MS: u64 = 80;

#[derive(Debug, Deserialize)]
struct FundingRow {
    symbol: String,
    #[serde(rename = "fundingRate")]
    funding_rate: String,
    #[serde(rename = "fundingTime")]
    funding_time: i64,
}

async fn fetch_one(
    pool: &PgPool,
    http: &Client,
    exchange: &str,
    segment: &str,
    symbol: &str,
) -> anyhow::Result<()> {
    let url = format!(
        "https://fapi.binance.com/fapi/v1/fundingRate?symbol={symbol}&limit=1"
    );
    let resp = http
        .get(&url)
        .send()
        .await
        .context("Binance funding fetch failed")?;
    if !resp.status().is_success() {
        anyhow::bail!("Binance HTTP {}", resp.status());
    }
    let rows: Vec<FundingRow> =
        resp.json().await.context("funding JSON decode")?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(());
    };
    let rate: f64 = row
        .funding_rate
        .parse()
        .context("funding_rate parse")?;
    let bar_time = DateTime::<chrono::Utc>::from_timestamp_millis(
        row.funding_time,
    )
    .ok_or_else(|| anyhow::anyhow!("invalid funding_time"))?;
    let raw_meta = serde_json::json!({
        "source": "binance_fapi",
        "symbol_returned": row.symbol,
    });
    // Synthesize a per-funding-period snapshot. Backfill across
    // symbols and time relies on funding_time uniqueness in the PK.
    sqlx::query(
        r#"INSERT INTO external_snapshots
              (exchange, segment, symbol, timeframe, bar_time,
               funding_rate, raw_meta)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           ON CONFLICT (exchange, segment, symbol, timeframe, bar_time)
           DO UPDATE SET
               funding_rate = EXCLUDED.funding_rate,
               raw_meta     = EXCLUDED.raw_meta"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind("8h") // funding period anchor; per-bar joiners overlay
    .bind(bar_time)
    .bind(rust_decimal::Decimal::try_from(rate).ok())
    .bind(&raw_meta)
    .execute(pool)
    .await
    .context("funding upsert failed")?;
    Ok(())
}

async fn enabled_perp_symbols(
    pool: &PgPool,
) -> anyhow::Result<Vec<(String, String, String)>> {
    use sqlx::Row;
    let rows = sqlx::query(
        r#"SELECT DISTINCT exchange, segment, symbol
             FROM engine_symbols
            WHERE enabled = true AND segment = 'futures'"#,
    )
    .fetch_all(pool)
    .await
    .context("engine_symbols query failed")?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let ex: String = r.try_get("exchange").unwrap_or_default();
        let sg: String = r.try_get("segment").unwrap_or_default();
        let sy: String = r.try_get("symbol").unwrap_or_default();
        if ex.is_empty() || sg.is_empty() || sy.is_empty() {
            continue;
        }
        out.push((ex, sg, sy));
    }
    Ok(out)
}

/// One-shot historical backfill per symbol. Binance lets us
/// request up to 1000 funding rows per call. We pull the last
/// 1000 (≈ 11 months at 8-hour cadence) for each enabled symbol
/// on startup so the backtest's funding_oi_signals scorer has
/// real historical data to score against.
async fn backfill_history(
    pool: &PgPool,
    http: &Client,
    exchange: &str,
    segment: &str,
    symbol: &str,
) -> anyhow::Result<usize> {
    let url = format!(
        "https://fapi.binance.com/fapi/v1/fundingRate?symbol={symbol}&limit=1000"
    );
    let resp = http
        .get(&url)
        .send()
        .await
        .context("funding backfill request")?;
    if !resp.status().is_success() {
        anyhow::bail!("Binance HTTP {}", resp.status());
    }
    let rows: Vec<FundingRow> =
        resp.json().await.context("funding backfill JSON")?;
    let mut written = 0usize;
    for row in rows {
        let rate: f64 = match row.funding_rate.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let bar_time = match chrono::DateTime::<chrono::Utc>::from_timestamp_millis(
            row.funding_time,
        ) {
            Some(t) => t,
            None => continue,
        };
        let raw_meta = serde_json::json!({
            "source": "binance_fapi_backfill",
        });
        let res = sqlx::query(
            r#"INSERT INTO external_snapshots
                  (exchange, segment, symbol, timeframe, bar_time,
                   funding_rate, raw_meta)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               ON CONFLICT (exchange, segment, symbol, timeframe, bar_time)
               DO NOTHING"#,
        )
        .bind(exchange)
        .bind(segment)
        .bind(symbol)
        .bind("8h")
        .bind(bar_time)
        .bind(rust_decimal::Decimal::try_from(rate).ok())
        .bind(&raw_meta)
        .execute(pool)
        .await;
        if let Ok(r) = res {
            if r.rows_affected() > 0 {
                written += 1;
            }
        }
    }
    Ok(written)
}

pub async fn run(pool: PgPool) {
    let http = match Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "funding_rate_loop: HTTP init failed");
            return;
        }
    };
    // One-shot backfill — every enabled futures symbol gets its
    // last 1000 funding readings (~11 months at 8h cadence).
    if let Ok(symbols) = enabled_perp_symbols(&pool).await {
        let mut total = 0usize;
        for (ex, sg, sy) in &symbols {
            match backfill_history(&pool, &http, ex, sg, sy).await {
                Ok(n) => total += n,
                Err(e) => {
                    warn!(symbol = sy, error = %e, "funding backfill failed")
                }
            }
            sleep(Duration::from_millis(PER_SYMBOL_DELAY_MS)).await;
        }
        info!(
            symbols = symbols.len(),
            rows = total,
            "funding_rate_loop: backfill complete"
        );
    }
    info!("funding_rate_loop: live polling every {POLL_INTERVAL_SECS}s");
    loop {
        match enabled_perp_symbols(&pool).await {
            Ok(symbols) if !symbols.is_empty() => {
                let mut ok = 0;
                let mut err = 0;
                for (ex, sg, sy) in &symbols {
                    match fetch_one(&pool, &http, ex, sg, sy).await {
                        Ok(()) => ok += 1,
                        Err(e) => {
                            err += 1;
                            warn!(symbol = sy, error = %e, "funding fetch failed");
                        }
                    }
                    sleep(Duration::from_millis(PER_SYMBOL_DELAY_MS)).await;
                }
                debug!(ok, err, "funding_rate_loop: tick ok");
            }
            Ok(_) => {
                debug!("funding_rate_loop: no enabled futures symbols");
            }
            Err(e) => {
                warn!(error = %e, "funding_rate_loop: symbol fetch failed");
            }
        }
        sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }
}

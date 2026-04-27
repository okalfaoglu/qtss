//! Fear & Greed Index ingest loop.
//!
//! Polls https://api.alternative.me/fng/ every 4 hours and upserts
//! the latest reading into `fear_greed_snapshots`. The
//! `qtss-backtest` `sentiment_extreme` scorer reads from this table
//! to score Dip / Top setups against extreme sentiment.
//!
//! No API key needed — alternative.me's F&G endpoint is public.
//! Defaults to one global F&G value that applies to all crypto
//! symbols (the index measures the BROAD crypto market mood, not
//! per-symbol sentiment).
//!
//! Cadence: 4 hours. The published index updates once per day, so
//! polling more frequently is wasteful; less frequently leaves the
//! sentiment scorer stale on long downtrends.

use anyhow::Context;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

const POLL_INTERVAL_SECS: u64 = 14_400; // 4 hours
const REQUEST_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Deserialize)]
struct FngResponse {
    data: Vec<FngEntry>,
}

#[derive(Debug, Deserialize)]
struct FngEntry {
    value: String,
    value_classification: String,
    timestamp: String,
}

/// Fetch the latest F&G value and upsert into the snapshot table.
/// Idempotent — `ON CONFLICT(captured_at)` handles repeated polls
/// of the same daily reading.
async fn fetch_and_upsert(
    pool: &PgPool,
    http: &Client,
) -> anyhow::Result<Option<i64>> {
    let resp = http
        .get("https://api.alternative.me/fng/?limit=1")
        .send()
        .await
        .context("alternative.me request failed")?;
    if !resp.status().is_success() {
        anyhow::bail!("alternative.me HTTP {}", resp.status());
    }
    let body: FngResponse = resp.json().await.context("F&G JSON decode")?;
    let Some(entry) = body.data.into_iter().next() else {
        warn!("alternative.me returned empty data array");
        return Ok(None);
    };
    let value: i32 = entry
        .value
        .parse()
        .context("F&G value parse")?;
    let unix: i64 = entry
        .timestamp
        .parse()
        .context("F&G timestamp parse")?;
    let captured_at: DateTime<Utc> =
        DateTime::<Utc>::from_timestamp(unix, 0)
            .ok_or_else(|| anyhow::anyhow!("invalid F&G timestamp"))?;
    let raw_meta = serde_json::json!({
        "label": entry.value_classification,
        "source": "alternative.me",
    });
    sqlx::query(
        r#"INSERT INTO fear_greed_snapshots (captured_at, value, label, raw_meta)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (captured_at) DO UPDATE SET
               value    = EXCLUDED.value,
               label    = EXCLUDED.label,
               raw_meta = EXCLUDED.raw_meta"#,
    )
    .bind(captured_at)
    .bind(value)
    .bind(&entry.value_classification)
    .bind(&raw_meta)
    .execute(pool)
    .await
    .context("F&G upsert failed")?;
    Ok(Some(value as i64))
}

/// One-shot historical backfill. Pulls the last `limit` daily F&G
/// readings (alternative.me default cadence is 1/day) so the
/// backtest's sentiment_extreme scorer has a full historical
/// series to query against, not just a single live tick.
async fn backfill_history(
    pool: &PgPool,
    http: &Client,
    limit: u32,
) -> anyhow::Result<usize> {
    let url = format!("https://api.alternative.me/fng/?limit={limit}");
    let resp = http
        .get(&url)
        .send()
        .await
        .context("F&G backfill request")?;
    if !resp.status().is_success() {
        anyhow::bail!("alternative.me HTTP {}", resp.status());
    }
    let body: FngResponse = resp.json().await.context("F&G backfill JSON")?;
    let mut written = 0usize;
    for entry in body.data {
        let value: i32 = match entry.value.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let unix: i64 = match entry.timestamp.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let captured_at: DateTime<Utc> =
            match DateTime::<Utc>::from_timestamp(unix, 0) {
                Some(t) => t,
                None => continue,
            };
        let raw_meta = serde_json::json!({
            "label": entry.value_classification,
            "source": "alternative.me",
            "ingested_at": Utc::now().to_rfc3339(),
        });
        if sqlx::query(
            r#"INSERT INTO fear_greed_snapshots (captured_at, value, label, raw_meta)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (captured_at) DO NOTHING"#,
        )
        .bind(captured_at)
        .bind(value)
        .bind(&entry.value_classification)
        .bind(&raw_meta)
        .execute(pool)
        .await
        .map(|r| r.rows_affected() > 0)
        .unwrap_or(false)
        {
            written += 1;
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
            error!(error = %e, "fear_greed_loop: HTTP client init failed");
            return;
        }
    };
    // One-shot backfill on startup — pull the last 1500 daily
    // readings (~4 years of history). alternative.me has data
    // back to 2018; 1500 covers everything the backtest scorer
    // is likely to query against.
    match backfill_history(&pool, &http, 1500).await {
        Ok(n) if n > 0 => {
            info!(rows = n, "fear_greed_loop: backfill complete")
        }
        Ok(_) => debug!("fear_greed_loop: backfill found no new rows"),
        Err(e) => warn!(error = %e, "fear_greed_loop: backfill failed"),
    }
    info!("fear_greed_loop: live polling every {POLL_INTERVAL_SECS}s");
    loop {
        match fetch_and_upsert(&pool, &http).await {
            Ok(Some(v)) => debug!(value = v, "fear_greed_loop: tick ok"),
            Ok(None) => debug!("fear_greed_loop: empty response"),
            Err(e) => warn!(error = %e, "fear_greed_loop: tick failed"),
        }
        sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }
}

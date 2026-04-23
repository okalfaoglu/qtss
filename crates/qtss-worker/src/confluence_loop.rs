// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! `confluence_loop` — tick-driven aggregator that rolls recent
//! `detections` into `confluence_snapshots` rows per symbol × TF.
//!
//! Complements the detector engine: detectors publish raw events,
//! this loop produces the "what do all of them say together?" reading
//! the strategy / validator / AI-gate layers actually need.

use std::time::Duration;

use qtss_analysis::{load_confluence_config, ConfluenceScorer};
use serde_json::Value;
use sqlx::{PgPool, Row};
use tracing::{info, warn};

pub async fn confluence_loop(pool: PgPool) {
    info!("confluence_loop: started");
    loop {
        if !load_enabled(&pool).await {
            tokio::time::sleep(Duration::from_secs(load_tick_secs(&pool).await)).await;
            continue;
        }
        let secs = load_tick_secs(&pool).await;
        if let Err(e) = run_tick(&pool).await {
            warn!(%e, "confluence tick failed");
        }
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

async fn run_tick(pool: &PgPool) -> anyhow::Result<()> {
    let cfg = load_confluence_config(pool).await;
    let scorer = ConfluenceScorer::new(cfg);
    let syms = sqlx::query(
        r#"SELECT exchange, segment, symbol, "interval"
             FROM engine_symbols
            WHERE enabled = true"#,
    )
    .fetch_all(pool)
    .await?;
    let mut written = 0usize;
    for r in syms {
        let exchange: String = r.get("exchange");
        let segment: String = r.get("segment");
        let symbol: String = r.get("symbol");
        let timeframe: String = r.get("interval");
        // Read latest regime snapshot (optional) — plain string, falls
        // back to None when the table is empty.
        let regime = load_latest_regime(pool, &exchange, &segment, &symbol, &timeframe).await;
        match scorer
            .compute(pool, &exchange, &segment, &symbol, &timeframe, regime.as_deref())
            .await
        {
            Ok(snap) => {
                // Skip "no detections" snapshots — they just clutter
                // the table. Mixed verdict with zero net and zero raw
                // means there were simply no inputs.
                if snap.bull_score + snap.bear_score <= 0.0 {
                    continue;
                }
                let _ = sqlx::query(
                    r#"INSERT INTO confluence_snapshots
                          (exchange, segment, symbol, timeframe, bull_score, bear_score,
                           net_score, confidence, verdict, contributors, regime)
                       VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
                       ON CONFLICT (exchange, segment, symbol, timeframe, computed_at)
                       DO NOTHING"#,
                )
                .bind(&snap.exchange)
                .bind(&snap.segment)
                .bind(&snap.symbol)
                .bind(&snap.timeframe)
                .bind(snap.bull_score)
                .bind(snap.bear_score)
                .bind(snap.net_score)
                .bind(snap.confidence)
                .bind(snap.verdict.as_str())
                .bind(&snap.contributors)
                .bind(regime.as_deref())
                .execute(pool)
                .await;
                written += 1;
            }
            Err(e) => warn!(%e, "confluence compute failed for {symbol} {timeframe}"),
        }
    }
    if written > 0 {
        info!(rows = written, "confluence_loop tick ok");
    }
    Ok(())
}

async fn load_latest_regime(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    timeframe: &str,
) -> Option<String> {
    let row = sqlx::query(
        r#"SELECT regime_kind FROM regime_snapshots
            WHERE exchange = $1 AND segment = $2 AND symbol = $3 AND timeframe = $4
            ORDER BY snapshot_at DESC LIMIT 1"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(timeframe)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;
    row.try_get::<String, _>("regime_kind").ok()
}

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'confluence' AND config_key = 'enabled'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return true; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

async fn load_tick_secs(pool: &PgPool) -> u64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'confluence' AND config_key = 'tick_secs'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 60; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(60)
        .max(15)
}

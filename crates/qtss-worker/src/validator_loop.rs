// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! `validator_loop` — tick-driven pattern invalidator. For every
//! non-invalidated detection row (Classical/Range/Gap/Harmonic/Motive/
//! SMC/ORB), fetch the latest bar close and ATR, hand both to the
//! family's validator, and flip `invalidated = true` if the verdict
//! comes back `Invalidate`.
//!
//! Complements the writer engine: writers keep emitting fresh rows,
//! this loop garbage-collects dead ones so the chart and strategy
//! layer don't act on stale geometry.

use std::time::Duration;

use qtss_validator::{default_registry, DetectionRow, ValidatorConfig, ValidatorVerdict};
use rust_decimal::prelude::ToPrimitive;
use serde_json::Value;
use sqlx::{PgPool, Row};
use tracing::{info, warn};

pub async fn validator_loop(pool: PgPool) {
    info!("validator_loop: started");
    let registry = default_registry();
    loop {
        if !load_enabled(&pool).await {
            tokio::time::sleep(Duration::from_secs(load_tick_secs(&pool).await)).await;
            continue;
        }
        let secs = load_tick_secs(&pool).await;
        let cfg = load_cfg(&pool).await;
        match run_tick(&pool, &registry, &cfg).await {
            Ok(n) if n > 0 => info!(invalidated = n, "validator_loop tick ok"),
            Ok(_) => {}
            Err(e) => warn!(%e, "validator_loop tick failed"),
        }
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

async fn run_tick(
    pool: &PgPool,
    registry: &qtss_validator::ValidatorRegistry,
    cfg: &ValidatorConfig,
) -> anyhow::Result<usize> {
    // Batch-scan active detections, grouped by (exchange, segment,
    // symbol, timeframe) so we fetch the latest close+ATR once per
    // series.
    let groups = sqlx::query(
        r#"SELECT DISTINCT exchange, segment, symbol, timeframe
             FROM detections
            WHERE invalidated = false"#,
    )
    .fetch_all(pool)
    .await?;
    let mut invalidated = 0usize;
    for g in groups {
        let exchange: String = g.get("exchange");
        let segment: String = g.get("segment");
        let symbol: String = g.get("symbol");
        let timeframe: String = g.get("timeframe");
        // Skip the '*' sentinel — derivatives/orderflow events use
        // their own raw_meta-based invalidation; they aren't part of
        // the bar-close validator loop.
        if timeframe == "*" {
            continue;
        }
        let Some((price, atr)) = load_price_atr(pool, &exchange, &segment, &symbol, &timeframe)
            .await
        else {
            continue;
        };
        let rows = sqlx::query(
            r#"SELECT exchange, segment, symbol, timeframe, slot,
                      pattern_family, subkind, direction, start_time, end_time, mode,
                      anchors, raw_meta
                 FROM detections
                WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND timeframe=$4
                  AND invalidated = false"#,
        )
        .bind(&exchange)
        .bind(&segment)
        .bind(&symbol)
        .bind(&timeframe)
        .fetch_all(pool)
        .await?;
        for r in rows {
            let row = DetectionRow {
                id: format!(
                    "{}:{}:{}:{}:{}:{}",
                    exchange,
                    segment,
                    symbol,
                    timeframe,
                    r.try_get::<i16, _>("slot").unwrap_or(0),
                    r.try_get::<String, _>("subkind").unwrap_or_default(),
                ),
                family: r.try_get("pattern_family").unwrap_or_default(),
                subkind: r.try_get("subkind").unwrap_or_default(),
                direction: r.try_get("direction").unwrap_or(0),
                anchors: r.try_get("anchors").unwrap_or(Value::Null),
                raw_meta: r.try_get("raw_meta").unwrap_or(Value::Null),
            };
            let (verdict, reason) = registry.validate(&row, price, atr, cfg);
            if !matches!(verdict, ValidatorVerdict::Invalidate) {
                continue;
            }
            let slot: i16 = r.try_get("slot").unwrap_or(0);
            let start_time: chrono::DateTime<chrono::Utc> =
                r.try_get("start_time").unwrap_or_else(|_| chrono::Utc::now());
            let end_time: chrono::DateTime<chrono::Utc> =
                r.try_get("end_time").unwrap_or_else(|_| chrono::Utc::now());
            let mode: String = r.try_get("mode").unwrap_or_else(|_| "live".to_string());
            let reason_str = reason.map(|x| x.as_str()).unwrap_or("generic");
            let _ = sqlx::query(
                r#"UPDATE detections
                      SET invalidated = true,
                          raw_meta = COALESCE(raw_meta, '{}'::jsonb)
                                     || jsonb_build_object('invalidation_reason', $7::text,
                                                           'invalidated_at', now()::text),
                          updated_at = now()
                    WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND timeframe=$4
                      AND slot=$5 AND pattern_family=$8 AND subkind=$9
                      AND start_time=$10 AND end_time=$11 AND mode=$6
                      AND invalidated=false"#,
            )
            .bind(&exchange)
            .bind(&segment)
            .bind(&symbol)
            .bind(&timeframe)
            .bind(slot)
            .bind(&mode)
            .bind(reason_str)
            .bind(&row.family)
            .bind(&row.subkind)
            .bind(start_time)
            .bind(end_time)
            .execute(pool)
            .await;
            invalidated += 1;
        }
    }
    Ok(invalidated)
}

async fn load_price_atr(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    timeframe: &str,
) -> Option<(f64, f64)> {
    // Latest bar close.
    let bar = sqlx::query(
        r#"SELECT close FROM market_bars
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND interval=$4
            ORDER BY open_time DESC LIMIT 1"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(timeframe)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;
    let close = bar
        .try_get::<rust_decimal::Decimal, _>("close")
        .ok()?
        .to_f64()?;
    // ATR — read latest indicator_snapshot if present; fallback to 0.
    let atr_row = sqlx::query(
        r#"SELECT values FROM indicator_snapshots
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND timeframe=$4 AND indicator='atr'
            ORDER BY bar_time DESC LIMIT 1"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(timeframe)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let atr = atr_row
        .and_then(|r| r.try_get::<Value, _>("values").ok())
        .and_then(|v| v.get("atr").and_then(|x| x.as_f64()))
        .unwrap_or(0.0);
    Some((close, atr))
}

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'validator' AND config_key = 'enabled'",
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
        "SELECT value FROM system_config WHERE module = 'validator' AND config_key = 'tick_secs'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 60; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("secs").and_then(|v| v.as_u64()).unwrap_or(60).max(15)
}

async fn load_cfg(pool: &PgPool) -> ValidatorConfig {
    let mut cfg = ValidatorConfig::default();
    let rows = sqlx::query(
        r#"SELECT config_key, value FROM system_config
            WHERE module = 'validator' AND config_key LIKE 'thresholds.%'"#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for r in rows {
        let key: String = r.try_get("config_key").unwrap_or_default();
        let val: Value = r.try_get("value").unwrap_or(Value::Null);
        let Some(v) = val.get("value").and_then(|v| v.as_f64()) else { continue };
        let k = key.trim_start_matches("thresholds.");
        match k {
            "harmonic_break_pct" => cfg.harmonic_break_pct = v,
            "range_full_fill_pct" => cfg.range_full_fill_pct = v,
            "gap_close_pct" => cfg.gap_close_pct = v,
            "motive_wave1_buffer_pct" => cfg.motive_wave1_buffer_pct = v,
            "smc_break_buffer_pct" => cfg.smc_break_buffer_pct = v,
            "orb_reentry_bars" => cfg.orb_reentry_bars = v as u32,
            "classical_break_pct" => cfg.classical_break_pct = v,
            _ => {}
        }
    }
    cfg
}

//! backtest_outcome_eval — Faz 12.D (family-agnostic).
//!
//! Forward-walk evaluator for ALL backtest detection families
//! (harmonic, classical, elliott, wyckoff). Uses a uniform R-multiple
//! exit model so one binary covers the whole inventory:
//!
//!   * **Entry**         = last anchor price (D for harmonic, p5 for
//!                         elliott, terminal pivot for classical/wyckoff).
//!                         This is the "pattern completed" price — also
//!                         what the live strategy would fill at.
//!   * **SL**            = `invalidation_price`. Each detector writes its
//!                         own family-correct invalidation (p0 for
//!                         elliott, D-buffered for harmonic, neckline
//!                         for classical, phase-level for wyckoff), so
//!                         we respect it as-is. Optional ATR buffer via
//!                         `backtest.generic.sl_buffer_atr`.
//!   * **R**             = |entry − SL| (risk distance).
//!   * **TP1**           = entry + `tp1_r` × R (direction-aware).
//!   * **TP2**           = entry + `tp2_r` × R.
//!   * **Time stop**     = `time_stop_legs × avg_leg_bars(level)` bars.
//!   * **Direction**     = subkind suffix (`_bear` = short) OR inferred
//!                         from sign(entry − invalidation).
//!
//! Commission applied as 2× `backtest.commission_bps` round-trip.
//! Idempotent: ON CONFLICT (detection_id) DO NOTHING. To re-run after
//! config tuning, truncate `qtss_v2_detection_outcomes` for mode=backtest
//! first (done by the orchestrator script, not this binary).

use std::collections::HashMap;
use std::env;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

fn avg_leg_bars(level: &str) -> u32 {
    match level {
        "L0" => 7,
        "L1" => 16,
        "L2" => 45,
        "L3" => 115,
        _    => 16,
    }
}

#[derive(Debug)]
struct Row1 {
    id: Uuid,
    family: String,
    exchange: String,
    symbol: String,
    interval: String,
    pivot_level: String,
    subkind: String,
    detected_at: DateTime<Utc>,
    invalidation: Decimal,
    anchors: serde_json::Value,
}

#[derive(Debug)]
struct Bar {
    open_time: DateTime<Utc>,
    high: Decimal,
    low: Decimal,
    close: Decimal,
}

#[derive(Debug)]
struct EvalConfig {
    tp1_r: f64,
    tp2_r: f64,
    sl_buffer_atr: f64,
    time_stop_legs: u32,
    commission_bps: f64,
    families: Vec<String>,
    // Faz 13 — tier-aware TP/SL overrides for `pivot_reversal`.
    pr_reactive_tp1_r: f64,
    pr_reactive_tp2_r: f64,
    pr_reactive_expiry_bars_mult: u32,
    pr_major_tp1_r: f64,
    pr_major_tp2_r: f64,
    pr_major_expiry_bars_mult: u32,
}

/// Resolve (tp1_r, tp2_r, expiry_bars_multiplier) for a given row.
/// For `pivot_reversal`, subkind begins with `reactive_` or `major_`
/// → pick the matching tier overrides. Otherwise the generic values.
fn effective_exit_params(row: &Row1, cfg: &EvalConfig) -> (f64, f64, u32) {
    if row.family == "pivot_reversal" {
        if row.subkind.starts_with("major_") {
            return (cfg.pr_major_tp1_r, cfg.pr_major_tp2_r, cfg.pr_major_expiry_bars_mult);
        }
        if row.subkind.starts_with("reactive_") {
            return (cfg.pr_reactive_tp1_r, cfg.pr_reactive_tp2_r, cfg.pr_reactive_expiry_bars_mult);
        }
    }
    (cfg.tp1_r, cfg.tp2_r, cfg.time_stop_legs)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let dsn = env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL is required"))?;
    let pool = PgPoolOptions::new().max_connections(4).connect(&dsn).await?;

    let cfg = load_config(&pool).await?;
    tracing::info!(?cfg, "backtest outcome evaluator starting");

    let batch_size: i64 = 10_000;
    let mut total_eval = 0u64;
    loop {
        let batch = pending_batch(&pool, &cfg.families, batch_size).await?;
        if batch.is_empty() {
            break;
        }
        tracing::info!(batch = batch.len(), "processing batch");
        let mut grouped: HashMap<(String, String, String), Vec<Row1>> = HashMap::new();
        for r in batch {
            grouped
                .entry((r.exchange.clone(), r.symbol.clone(), r.interval.clone()))
                .or_default()
                .push(r);
        }
        for ((exchange, symbol, interval), rows) in grouped {
            let earliest = rows.iter().map(|r| r.detected_at).min().unwrap();
            let bars = load_bars_after(&pool, &exchange, &symbol, &interval, earliest).await?;
            for row in &rows {
                evaluate_and_record(&pool, row, &bars, &cfg).await?;
                total_eval += 1;
            }
        }
    }
    tracing::info!(total_eval, "outcome eval complete");
    Ok(())
}

async fn load_config(pool: &PgPool) -> anyhow::Result<EvalConfig> {
    // Reuse harmonic keys where applicable (tp1/tp2 reinterpreted as
    // R-multiples via new generic keys if present; fall back to
    // sensible Carney-style defaults).
    let fetch = |pool: &PgPool, key: &'static str, fallback: f64| {
        let key = key.to_string();
        let pool = pool.clone();
        async move {
            let row = sqlx::query(
                r#"SELECT (default_value::text)::float8 AS v
                     FROM config_schema WHERE key = $1"#,
            )
            .bind(&key)
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten();
            row.and_then(|r| r.try_get::<f64, _>("v").ok()).unwrap_or(fallback)
        }
    };
    let tp1_r = fetch(pool, "backtest.generic.tp1_r", 1.0).await;
    let tp2_r = fetch(pool, "backtest.generic.tp2_r", 2.0).await;
    let sl_buffer_atr = fetch(pool, "backtest.generic.sl_buffer_atr", 0.0).await;
    let time_stop_legs = fetch(pool, "backtest.harmonic.time_stop_legs", 3.0).await.round() as u32;
    let commission_bps = fetch(pool, "backtest.commission_bps", 4.0).await;

    let families_csv = env::var("EVAL_FAMILIES").unwrap_or_else(|_| {
        "harmonic,classical,elliott,wyckoff,pivot_reversal".to_string()
    });
    let families: Vec<String> = families_csv.split(',').map(|s| s.trim().to_string()).collect();

    // Faz 13 tier-aware pivot_reversal overrides.
    let pr_reactive_tp1_r  = fetch(pool, "eval.pivot_reversal.reactive.tp1_r", 1.0).await;
    let pr_reactive_tp2_r  = fetch(pool, "eval.pivot_reversal.reactive.tp2_r", 2.0).await;
    let pr_reactive_expiry = fetch(pool, "eval.pivot_reversal.reactive.expiry_bars_mult", 20.0).await.round() as u32;
    let pr_major_tp1_r     = fetch(pool, "eval.pivot_reversal.major.tp1_r", 1.5).await;
    let pr_major_tp2_r     = fetch(pool, "eval.pivot_reversal.major.tp2_r", 3.0).await;
    let pr_major_expiry    = fetch(pool, "eval.pivot_reversal.major.expiry_bars_mult", 60.0).await.round() as u32;

    Ok(EvalConfig {
        tp1_r, tp2_r, sl_buffer_atr, time_stop_legs, commission_bps, families,
        pr_reactive_tp1_r, pr_reactive_tp2_r, pr_reactive_expiry_bars_mult: pr_reactive_expiry,
        pr_major_tp1_r,    pr_major_tp2_r,    pr_major_expiry_bars_mult:    pr_major_expiry,
    })
}

async fn pending_batch(pool: &PgPool, families: &[String], limit: i64) -> anyhow::Result<Vec<Row1>> {
    let rows = sqlx::query(
        r#"SELECT d.id, d.family, d.exchange, d.symbol, d.timeframe AS interval,
                  d.pivot_level, d.subkind, d.detected_at,
                  d.invalidation_price AS invalidation, d.anchors
             FROM qtss_v2_detections d
             LEFT JOIN qtss_v2_detection_outcomes o ON o.detection_id = d.id
            WHERE d.family = ANY($1)
              AND d.mode   = 'backtest'
              AND d.pivot_level IS NOT NULL
              AND o.id IS NULL
            ORDER BY d.detected_at ASC
            LIMIT $2"#,
    )
    .bind(families)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Row1 {
            id:           r.get("id"),
            family:       r.get("family"),
            exchange:     r.get("exchange"),
            symbol:       r.get("symbol"),
            interval:     r.get("interval"),
            pivot_level:  r.get("pivot_level"),
            subkind:      r.get("subkind"),
            detected_at:  r.get("detected_at"),
            invalidation: r.get("invalidation"),
            anchors:      r.get("anchors"),
        })
        .collect())
}

async fn load_bars_after(
    pool: &PgPool, exchange: &str, symbol: &str, interval: &str, after: DateTime<Utc>,
) -> anyhow::Result<Vec<Bar>> {
    let rows = sqlx::query(
        r#"SELECT open_time, high, low, close
             FROM market_bars
            WHERE exchange = $1 AND symbol = $2 AND interval = $3
              AND open_time >= $4
            ORDER BY open_time ASC"#,
    )
    .bind(exchange).bind(symbol).bind(interval).bind(after)
    .fetch_all(pool).await?;
    Ok(rows.into_iter().map(|r| Bar {
        open_time: r.get("open_time"),
        high: r.get("high"),
        low:  r.get("low"),
        close: r.get("close"),
    }).collect())
}

async fn atr14_at(
    pool: &PgPool, exchange: &str, symbol: &str, interval: &str, at: DateTime<Utc>,
) -> anyhow::Result<Option<f64>> {
    let rows = sqlx::query(
        r#"SELECT high, low, close FROM market_bars
            WHERE exchange = $1 AND symbol = $2 AND interval = $3
              AND open_time <= $4
            ORDER BY open_time DESC LIMIT 15"#,
    )
    .bind(exchange).bind(symbol).bind(interval).bind(at)
    .fetch_all(pool).await?;
    if rows.len() < 15 { return Ok(None); }
    let bars: Vec<(f64, f64, f64)> = rows.into_iter().rev()
        .map(|r| (
            r.get::<Decimal, _>("high").to_f64().unwrap_or(0.0),
            r.get::<Decimal, _>("low").to_f64().unwrap_or(0.0),
            r.get::<Decimal, _>("close").to_f64().unwrap_or(0.0),
        )).collect();
    let mut trs = Vec::with_capacity(14);
    for i in 1..15 {
        let (h, l, _) = bars[i];
        let prev_c = bars[i - 1].2;
        trs.push((h - l).max((h - prev_c).abs()).max((l - prev_c).abs()));
    }
    Ok(Some(trs.iter().sum::<f64>() / 14.0))
}

/// Last anchor's `price` field (string-encoded Decimal).
fn last_anchor_price(anchors: &serde_json::Value) -> Option<f64> {
    let arr = anchors.as_array()?;
    let last = arr.last()?;
    last.get("price")?.as_str()?.parse::<f64>().ok()
}

async fn evaluate_and_record(
    pool: &PgPool, row: &Row1, bars: &[Bar], cfg: &EvalConfig,
) -> anyhow::Result<()> {
    let Some(entry) = last_anchor_price(&row.anchors) else { return Ok(()); };
    let invalidation = row.invalidation.to_f64().unwrap_or(0.0);
    if invalidation <= 0.0 { return Ok(()); }

    // Direction: subkind suffix is primary. Fallback: invalidation above
    // entry = bearish (invalidation sits on "wrong side", which for
    // short is UP).
    let bearish = row.subkind.ends_with("_bear")
        || row.subkind.contains("bear")
        || row.subkind.contains("top")
        || (invalidation > entry && !row.subkind.ends_with("_bull"));

    let atr = atr14_at(pool, &row.exchange, &row.symbol, &row.interval, row.detected_at)
        .await?.unwrap_or(0.0);

    let sl = if bearish {
        invalidation + cfg.sl_buffer_atr * atr
    } else {
        invalidation - cfg.sl_buffer_atr * atr
    };
    let r_dist = (entry - sl).abs();
    if r_dist <= 0.0 { return Ok(()); }

    let (tp1_r, tp2_r, expiry_mult) = effective_exit_params(row, cfg);
    let tp1 = if bearish { entry - tp1_r * r_dist } else { entry + tp1_r * r_dist };
    let tp2 = if bearish { entry - tp2_r * r_dist } else { entry + tp2_r * r_dist };

    let max_bars = (expiry_mult * avg_leg_bars(&row.pivot_level)) as usize;
    let mut outcome = "expired";
    let mut exit_price = entry;
    let mut exit_time = row.detected_at;

    let start = bars.iter().position(|b| b.open_time > row.detected_at).unwrap_or(bars.len());
    let end = (start + max_bars).min(bars.len());
    for b in &bars[start..end] {
        let hi = b.high.to_f64().unwrap_or(0.0);
        let lo = b.low.to_f64().unwrap_or(0.0);
        let cl = b.close.to_f64().unwrap_or(0.0);
        exit_time = b.open_time;
        let hit_sl  = if bearish { hi >= sl  } else { lo <= sl  };
        let hit_tp1 = if bearish { lo <= tp1 } else { hi >= tp1 };
        if hit_sl { outcome = "loss"; exit_price = sl; break; }
        if hit_tp1 {
            let hit_tp2 = if bearish { lo <= tp2 } else { hi >= tp2 };
            outcome = "win";
            exit_price = if hit_tp2 { tp2 } else { tp1 };
            break;
        }
        exit_price = cl;
    }

    let gross = if bearish {
        (entry - exit_price) / entry * 100.0
    } else {
        (exit_price - entry) / entry * 100.0
    };
    let commission_pct = 2.0 * cfg.commission_bps / 100.0;
    let pnl_pct = gross - commission_pct;
    let duration_secs = (exit_time - row.detected_at).num_seconds().max(0);

    sqlx::query(
        r#"INSERT INTO qtss_v2_detection_outcomes
               (id, detection_id, outcome, close_reason,
                pnl_pct, entry_price, exit_price, duration_secs, resolved_at)
           VALUES
               (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7, now())
           ON CONFLICT (detection_id) DO NOTHING"#,
    )
    .bind(row.id)
    .bind(outcome)
    .bind(match outcome { "win" => "tp_hit", "loss" => "sl_hit", _ => "time_stop" })
    .bind(pnl_pct as f32)
    .bind(entry as f32)
    .bind(exit_price as f32)
    .bind(duration_secs)
    .execute(pool).await?;
    // Family is kept for logging/telemetry extensions.
    let _ = row.family.as_str();
    Ok(())
}

// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! `outcome_tracker_loop` — bar-by-bar historical outcome labeler.
//!
//! For every detection that has no final outcome yet, this loop
//! replays the bars forward from `start_time`:
//!   * TP1/TP2/TP3 touched before SL → `tp{n}_hit`
//!   * SL touched first → `sl_hit`
//!   * Per-family time-expiry cap reached → `expired`
//!   * Evaluation window not yet exhausted → `active`
//!
//! Separate from `validator_loop`: validator answers "is this still
//! actionable RIGHT NOW", outcome tracker answers "what actually
//! happened historically, bar by bar". Both rows coexist — the first
//! is the live chart filter, the second is the ground truth for AI
//! meta-label training, RADAR hit-rate stats, etc.
//!
//! TP/SL resolution happens in-line (not via qtss-targets) so this
//! loop is self-contained: reads the detection's `raw_meta` for
//! invalidation_price, parses harmonic anchors when family = harmonic,
//! falls back to ATR bands when nothing else fits.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::{debug, info, warn};

pub async fn outcome_tracker_loop(pool: PgPool) {
    info!("outcome_tracker_loop: started");
    loop {
        if !load_enabled(&pool).await {
            tokio::time::sleep(Duration::from_secs(load_tick_secs(&pool).await)).await;
            continue;
        }
        let secs = load_tick_secs(&pool).await;
        match run_tick(&pool).await {
            Ok(n) if n > 0 => info!(evaluated = n, "outcome_tracker tick ok"),
            Ok(_) => debug!("outcome_tracker tick: 0 evaluated"),
            Err(e) => warn!(%e, "outcome_tracker tick failed"),
        }
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

#[derive(Debug, Clone)]
struct Cfg {
    batch_size: i64,
    expiry_by_family: HashMap<String, i64>,
    fallback_atr_tp_mult: f64,
    fallback_atr_sl_mult: f64,
}

async fn run_tick(pool: &PgPool) -> anyhow::Result<usize> {
    let cfg = load_cfg(pool).await;
    // Priority: (1) rows where we've recorded `outcome = 'active'`
    // (those need a re-check with new bars) and (2) detections never
    // evaluated yet. Both coalesce into a single ordered query.
    let rows = sqlx::query(
        r#"WITH candidates AS (
             SELECT d.exchange, d.segment, d.symbol, d.timeframe, d.slot,
                    d.pattern_family, d.subkind, d.start_time, d.mode,
                    d.direction, d.anchors, d.raw_meta
               FROM detections d
               LEFT JOIN pattern_outcomes o
                 ON o.exchange=d.exchange AND o.segment=d.segment
                AND o.symbol=d.symbol   AND o.timeframe=d.timeframe
                AND o.slot=d.slot       AND o.pattern_family=d.pattern_family
                AND o.subkind=d.subkind AND o.start_time=d.start_time
                AND o.mode=d.mode
              WHERE d.timeframe != '*'
                AND (o.outcome IS NULL OR o.outcome = 'active')
                AND d.start_time > now() - interval '90 days'
              ORDER BY (o.outcome IS NULL) DESC, d.start_time ASC
              LIMIT $1
           )
           SELECT * FROM candidates"#,
    )
    .bind(cfg.batch_size)
    .fetch_all(pool)
    .await?;

    let mut evaluated = 0usize;
    for r in rows {
        match evaluate_one(pool, &r, &cfg).await {
            Ok(()) => evaluated += 1,
            Err(e) => warn!(%e, "outcome_tracker: detection eval failed"),
        }
    }
    Ok(evaluated)
}

async fn evaluate_one(pool: &PgPool, r: &sqlx::postgres::PgRow, cfg: &Cfg) -> anyhow::Result<()> {
    let exchange: String = r.get("exchange");
    let segment: String = r.get("segment");
    let symbol: String = r.get("symbol");
    let timeframe: String = r.get("timeframe");
    let slot: i16 = r.get("slot");
    let family: String = r.get("pattern_family");
    let subkind: String = r.get("subkind");
    let start_time: DateTime<Utc> = r.get("start_time");
    let mode: String = r.get("mode");
    let direction: i16 = r.get("direction");
    let anchors: Value = r.try_get("anchors").unwrap_or(Value::Null);
    let raw_meta: Value = r.try_get("raw_meta").unwrap_or(Value::Null);

    // Resolve entry + TP/SL from raw_meta / anchors / ATR fallback.
    let entry = anchor_price(&anchors, "D")
        .or_else(|| anchor_price(&anchors, "Break"))
        .or_else(|| anchor_price(&anchors, "G"))
        .or_else(|| {
            anchors
                .as_array()
                .and_then(|a| a.last())
                .and_then(|a| a.get("price"))
                .and_then(|v| v.as_f64())
        });
    let Some(entry) = entry else {
        return Ok(());
    };

    let (tp_levels, sl_price) = resolve_targets(&family, &anchors, &raw_meta, direction, entry, cfg);
    // No SL available → skip (can't grade without it).
    let Some(sl) = sl_price else { return Ok(()); };

    // Fetch forward bars from start_time up to the TTL cap.
    let ttl_bars = *cfg.expiry_by_family.get(&family).unwrap_or(&200);
    let bars = sqlx::query(
        r#"SELECT open_time, high, low, close
             FROM market_bars
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND interval=$4
              AND open_time >= $5
            ORDER BY open_time ASC
            LIMIT $6"#,
    )
    .bind(&exchange)
    .bind(&segment)
    .bind(&symbol)
    .bind(&timeframe)
    .bind(start_time)
    .bind(ttl_bars + 1)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    if bars.is_empty() {
        return Ok(());
    }

    // Walk bars, first hit wins.
    let (outcome_kind, tp_hit_count, outcome_bar, outcome_price, outcome_time, mfe, mae) = {
        let mut mfe: f64 = 0.0;
        let mut mae: f64 = 0.0;
        let mut result: Option<(&'static str, i16, i32, f64, DateTime<Utc>)> = None;
        for (idx, bar) in bars.iter().enumerate() {
            let high: f64 = bar
                .try_get::<rust_decimal::Decimal, _>("high")
                .ok()
                .and_then(|d| d.to_f64())
                .unwrap_or(0.0);
            let low: f64 = bar
                .try_get::<rust_decimal::Decimal, _>("low")
                .ok()
                .and_then(|d| d.to_f64())
                .unwrap_or(0.0);
            let close: f64 = bar
                .try_get::<rust_decimal::Decimal, _>("close")
                .ok()
                .and_then(|d| d.to_f64())
                .unwrap_or(0.0);
            let open_time: DateTime<Utc> = bar.try_get("open_time").unwrap_or_else(|_| Utc::now());
            // Update MFE/MAE in trade direction.
            let favorable = if direction >= 0 { high - entry } else { entry - low };
            let adverse = if direction >= 0 { entry - low } else { high - entry };
            if favorable > mfe {
                mfe = favorable;
            }
            if adverse > mae {
                mae = adverse;
            }
            // Check SL first (conservative) — touches imply fill.
            let sl_touched = if direction >= 0 { low <= sl } else { high >= sl };
            if sl_touched {
                result = Some(("sl_hit", 0, idx as i32, sl, open_time));
                break;
            }
            // Check TP ladder.
            let mut hit_level = 0i16;
            for (tp_idx, tp) in tp_levels.iter().enumerate() {
                let reached = if direction >= 0 { high >= *tp } else { low <= *tp };
                if reached {
                    hit_level = (tp_idx + 1) as i16;
                }
            }
            if hit_level > 0 {
                let tag: &'static str = match hit_level {
                    1 => "tp1_hit",
                    2 => "tp2_hit",
                    3 => "tp3_hit",
                    _ => "tp3_hit",
                };
                let price = tp_levels.get((hit_level - 1) as usize).copied().unwrap_or(close);
                result = Some((tag, hit_level, idx as i32, price, open_time));
                break;
            }
        }
        let (outcome_kind, tp_hit_count, outcome_bar, price, time) = match result {
            Some((o, n, b, p, t)) => (o, n, Some(b), Some(p), Some(t)),
            None => {
                // Ran out of bars without hit.
                if (bars.len() as i64) >= ttl_bars {
                    ("expired", 0, None, None, None)
                } else {
                    ("active", 0, None, None, None)
                }
            }
        };
        (outcome_kind, tp_hit_count, outcome_bar, price, time, mfe, mae)
    };

    // Upsert.
    let bars_to_outcome = outcome_bar;
    let outcome_price = outcome_price;
    let outcome_time = outcome_time;
    sqlx::query(
        r#"INSERT INTO pattern_outcomes
              (exchange, segment, symbol, timeframe, slot,
               pattern_family, subkind, start_time, mode,
               outcome, tp_hit_count, bars_to_outcome, outcome_time, outcome_price,
               mfe, mae, target_json, evaluated_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,now())
           ON CONFLICT (exchange, segment, symbol, timeframe, slot,
                        pattern_family, subkind, start_time, mode)
           DO UPDATE SET
               outcome = EXCLUDED.outcome,
               tp_hit_count = EXCLUDED.tp_hit_count,
               bars_to_outcome = EXCLUDED.bars_to_outcome,
               outcome_time = EXCLUDED.outcome_time,
               outcome_price = EXCLUDED.outcome_price,
               mfe = EXCLUDED.mfe,
               mae = EXCLUDED.mae,
               target_json = EXCLUDED.target_json,
               evaluated_at = now()"#,
    )
    .bind(&exchange)
    .bind(&segment)
    .bind(&symbol)
    .bind(&timeframe)
    .bind(slot)
    .bind(&family)
    .bind(&subkind)
    .bind(start_time)
    .bind(&mode)
    .bind(outcome_kind)
    .bind(tp_hit_count)
    .bind(bars_to_outcome)
    .bind(outcome_time)
    .bind(outcome_price)
    .bind(mfe)
    .bind(mae)
    .bind(json!({
        "entry": entry,
        "take_profits": tp_levels,
        "stop_loss": sl,
        "direction": direction,
    }))
    .execute(pool)
    .await?;
    Ok(())
}

fn anchor_price(anchors: &Value, label: &str) -> Option<f64> {
    anchors
        .as_array()?
        .iter()
        .find(|a| {
            let label_match = |field: &str| {
                a.get(field)
                    .and_then(|v| v.as_str())
                    .map(|s| s.eq_ignore_ascii_case(label))
                    .unwrap_or(false)
            };
            label_match("label_override") || label_match("label")
        })
        .and_then(|a| a.get("price"))
        .and_then(|v| v.as_f64())
}

fn raw_f64(raw: &Value, key: &str) -> Option<f64> {
    raw.get(key).and_then(|v| match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    })
}

/// Build TP ladder + SL for the outcome tracker. Order of preference:
/// harmonic anchors → raw_meta.invalidation → ATR fallback.
fn resolve_targets(
    family: &str,
    anchors: &Value,
    raw_meta: &Value,
    direction: i16,
    entry: f64,
    _cfg: &Cfg,
) -> (Vec<f64>, Option<f64>) {
    if family == "harmonic" {
        // TP ladder from D back toward C at fib ratios 0.382 / 0.618 / 1.0.
        let c = anchor_price(anchors, "C");
        let d = anchor_price(anchors, "D");
        let x = anchor_price(anchors, "X");
        let a = anchor_price(anchors, "A");
        if let (Some(c), Some(d)) = (c, d) {
            let cd = c - d;
            let tps = vec![d + 0.382 * cd, d + 0.618 * cd, d + 1.0 * cd];
            // SL beyond D by 2% of XA.
            let sl = if let (Some(x), Some(a)) = (x, a) {
                let xa = (a - x).abs();
                let sign = if direction >= 0 { -1.0 } else { 1.0 };
                Some(d + sign * xa * 0.02)
            } else {
                None
            };
            return (tps, sl);
        }
    }
    // Generic path: use invalidation_price from raw_meta as SL, build
    // symmetric R-multiple TP ladder (1R / 2R / 3R).
    let sl = raw_f64(raw_meta, "invalidation")
        .or_else(|| raw_f64(raw_meta, "invalidation_price"))
        .or_else(|| raw_f64(raw_meta, "or_low"))
        .or_else(|| raw_f64(raw_meta, "or_high"));
    if let Some(sl) = sl {
        let risk = (entry - sl).abs();
        if risk > 0.0 {
            let sign = if direction >= 0 { 1.0 } else { -1.0 };
            let tps = vec![entry + sign * risk * 1.0, entry + sign * risk * 2.0, entry + sign * risk * 3.0];
            return (tps, Some(sl));
        }
    }
    (vec![], None)
}

// ── Config loaders ─────────────────────────────────────────────────────

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'outcome_tracker' AND config_key = 'enabled'",
    )
    .fetch_optional(pool).await.ok().flatten();
    let Some(row) = row else { return true; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

async fn load_tick_secs(pool: &PgPool) -> u64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'outcome_tracker' AND config_key = 'tick_secs'",
    )
    .fetch_optional(pool).await.ok().flatten();
    let Some(row) = row else { return 300; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("secs").and_then(|v| v.as_u64()).unwrap_or(300).max(60)
}

async fn load_cfg(pool: &PgPool) -> Cfg {
    let mut expiry = HashMap::new();
    let rows = sqlx::query(
        r#"SELECT config_key, value FROM system_config
            WHERE module = 'outcome_tracker' AND config_key LIKE 'expiry.%'"#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for r in rows {
        let key: String = r.try_get("config_key").unwrap_or_default();
        let val: Value = r.try_get("value").unwrap_or(Value::Null);
        if let Some(bars) = val.get("bars").and_then(|v| v.as_i64()) {
            let family = key.trim_start_matches("expiry.").to_string();
            expiry.insert(family, bars);
        }
    }
    let batch_size = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'outcome_tracker' AND config_key = 'batch_size'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .and_then(|r| r.try_get::<Value, _>("value").ok())
    .and_then(|v| v.get("value").and_then(|x| x.as_i64()))
    .unwrap_or(500);
    let fallback_atr_tp_mult = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'outcome_tracker' AND config_key = 'fallback.atr_tp_mult'",
    )
    .fetch_optional(pool).await.ok().flatten()
    .and_then(|r| r.try_get::<Value, _>("value").ok())
    .and_then(|v| v.get("value").and_then(|x| x.as_f64()))
    .unwrap_or(2.0);
    let fallback_atr_sl_mult = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'outcome_tracker' AND config_key = 'fallback.atr_sl_mult'",
    )
    .fetch_optional(pool).await.ok().flatten()
    .and_then(|r| r.try_get::<Value, _>("value").ok())
    .and_then(|v| v.get("value").and_then(|x| x.as_f64()))
    .unwrap_or(1.0);
    Cfg {
        batch_size,
        expiry_by_family: expiry,
        fallback_atr_tp_mult,
        fallback_atr_sl_mult,
    }
}

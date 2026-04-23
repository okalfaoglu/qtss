// Workaround: rustc 1.95 `annotate_snippets` renderer ICE on dead-code
// lint. Silenced module-wide.
#![allow(dead_code)]

//! `indicator_persistence_loop` — materialises technical-indicator
//! values into the `indicator_snapshots` table on each engine tick.
//!
//! Why persist instead of recompute on demand?
//!   * **Backtest replay.** Sim loops re-read indicators millions of
//!     times; avoiding a Pine-port recompute per step is a ~50× win.
//!   * **Feature store.** ML training windows stay deterministic —
//!     `config_hash` lets us drop stale rows when an operator tweaks a
//!     period without breaking prior research.
//!   * **GUI fast-path.** Deep-zoom / long-history chart views can read
//!     cached rows rather than recomputing thousands of bars inline.
//!
//! Dispatch is the same table the `/v2/indicators` endpoint uses —
//! moved into a shared callable would be an improvement (PR-11H), but
//! for now this file intentionally duplicates the match so the worker
//! crate stays dependency-isolated from qtss-api. Adding a new
//! indicator means one row in both places (CLAUDE.md #1 — dispatch
//! table, not central match arm).
//!
//! Config (`system_config`, module = `indicator_persistence`):
//!   * `enabled`        → `{ "enabled": true }`
//!   * `tick_secs`      → `{ "secs": 60 }`
//!   * `bars_per_tick`  → `{ "bars": 300 }`
//!   * `names`          → `{ "names": [...] }`
//!   * `retention_days` → `{ "days": 90 }`

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use qtss_indicators as ind;
use qtss_storage::market_bars;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use tracing::{debug, info, warn};

// ── Engine-symbol listing (copied from qtss-engine) ────────────────────

#[derive(Debug, Clone)]
struct EngineSymbol {
    id: sqlx::types::Uuid,
    exchange: String,
    segment: String,
    symbol: String,
    interval: String,
}

async fn list_enabled_symbols(pool: &PgPool) -> anyhow::Result<Vec<EngineSymbol>> {
    let rows = sqlx::query(
        r#"SELECT id, exchange, segment, symbol, "interval"
             FROM engine_symbols
            WHERE enabled = true
            ORDER BY exchange, segment, symbol, "interval""#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| EngineSymbol {
            id: r.get("id"),
            exchange: r.get("exchange"),
            segment: r.get("segment"),
            symbol: r.get("symbol"),
            interval: r.get("interval"),
        })
        .collect())
}

// ── Outer loop ─────────────────────────────────────────────────────────

pub async fn indicator_persistence_loop(pool: PgPool) {
    info!("indicator_persistence_loop: started");
    loop {
        if !load_master_enabled(&pool).await {
            debug!("indicator_persistence disabled — sleeping");
            tokio::time::sleep(Duration::from_secs(
                load_tick_secs(&pool).await,
            ))
            .await;
            continue;
        }
        let secs = load_tick_secs(&pool).await;
        if let Err(e) = run_tick(&pool).await {
            warn!(%e, "indicator_persistence tick failed");
        }
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

async fn run_tick(pool: &PgPool) -> anyhow::Result<()> {
    let bars_per_tick = load_num_json(pool, "bars_per_tick", "bars", 300)
        .await
        .clamp(50, 2000);
    let names = load_names(pool).await;
    if names.is_empty() {
        return Ok(());
    }
    let syms = list_enabled_symbols(pool).await?;
    let mut rows_written = 0usize;
    for sym in &syms {
        match process_symbol(pool, sym, bars_per_tick, &names).await {
            Ok(n) => rows_written += n,
            Err(e) => warn!(
                exchange = %sym.exchange,
                symbol = %sym.symbol,
                tf = %sym.interval,
                %e,
                "indicator_persistence: symbol failed"
            ),
        }
    }
    if rows_written > 0 {
        info!(
            rows_written,
            symbols = syms.len(),
            indicators = names.len(),
            "indicator_persistence tick ok"
        );
    }
    // Opportunistic purge — cheap when the row count is low, bounded
    // by the daily-ish cadence from the retention guard below.
    purge_expired(pool).await;
    Ok(())
}

async fn process_symbol(
    pool: &PgPool,
    sym: &EngineSymbol,
    bars_per_tick: i64,
    names: &[String],
) -> anyhow::Result<usize> {
    let raw = market_bars::list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        bars_per_tick,
    )
    .await?;
    if raw.is_empty() {
        return Ok(0);
    }
    let chrono: Vec<market_bars::MarketBarRow> = raw.into_iter().rev().collect();
    let highs: Vec<f64> = chrono.iter().map(|b| b.high.to_f64().unwrap_or(0.0)).collect();
    let lows: Vec<f64> = chrono.iter().map(|b| b.low.to_f64().unwrap_or(0.0)).collect();
    let closes: Vec<f64> = chrono.iter().map(|b| b.close.to_f64().unwrap_or(0.0)).collect();
    let volumes: Vec<f64> = chrono.iter().map(|b| b.volume.to_f64().unwrap_or(0.0)).collect();
    let times: Vec<DateTime<Utc>> = chrono.iter().map(|b| b.open_time).collect();

    let ctx = Ctx {
        highs: &highs,
        lows: &lows,
        closes: &closes,
        volumes: &volumes,
    };

    let mut written = 0usize;
    for name in names {
        let (series, cfg_hash) = match compute(pool, name, &ctx).await {
            Some(v) => v,
            None => continue,
        };
        // Persist the last N bars' values so the GUI can render
        // immediately after a zoom-in. We keep "last 150" as a safe
        // bound — less than bars_per_tick so warm-up NaNs at the head
        // never make it to disk.
        let persist_from = series
            .values()
            .map(|v| v.len().saturating_sub(150))
            .min()
            .unwrap_or(0);
        for i in persist_from..times.len() {
            let bar_time = times[i];
            let mut obj = serde_json::Map::new();
            let mut any_value = false;
            for (sub, values) in &series {
                if let Some(v) = values.get(i) {
                    if !v.is_nan() {
                        obj.insert(sub.clone(), json!(*v));
                        any_value = true;
                    }
                }
            }
            if !any_value {
                continue;
            }
            let values_json = Value::Object(obj);
            let _ = upsert_snapshot(
                pool, sym, bar_time, name, &values_json, &cfg_hash,
            )
            .await;
            written += 1;
        }
    }
    Ok(written)
}

struct Ctx<'a> {
    highs: &'a [f64],
    lows: &'a [f64],
    closes: &'a [f64],
    volumes: &'a [f64],
}

// ── Indicator compute dispatcher ───────────────────────────────────────
//
// Mirrors `v2_indicators::compute_indicator` but with a simplified
// return: `(sub → values)` plus a config hash for cache invalidation.
async fn compute(
    pool: &PgPool,
    name: &str,
    c: &Ctx<'_>,
) -> Option<(HashMap<String, Vec<f64>>, String)> {
    let mut out: HashMap<String, Vec<f64>> = HashMap::new();
    let mut cfg_parts: Vec<(String, f64)> = Vec::new();
    // Local helpers inlined to avoid the "two mutable closures borrow
    // the same Vec" error: we use direct pushes below.
    macro_rules! note_num {
        ($k:expr, $v:expr) => { cfg_parts.push(($k.to_string(), $v as f64)); };
    }
    macro_rules! note_f {
        ($k:expr, $v:expr) => { cfg_parts.push(($k.to_string(), $v)); };
    }
    match name {
        "rsi" => {
            let p = load_num(pool, name, "period", 14).await as usize;
            note_num!("period", p as i64);
            out.insert("rsi".into(), ind::rsi(c.closes, p));
        }
        "ema" => {
            let fast = load_num(pool, name, "fast", 9).await as usize;
            let slow = load_num(pool, name, "slow", 21).await as usize;
            note_num!("fast", fast as i64);
            note_num!("slow", slow as i64);
            out.insert(format!("ema_{fast}"), ind::ema(c.closes, fast));
            out.insert(format!("ema_{slow}"), ind::ema(c.closes, slow));
        }
        "bollinger" => {
            let p = load_num(pool, name, "period", 20).await as usize;
            let m = load_f64(pool, name, "stdev", 2.0).await;
            note_num!("period", p as i64);
            note_f!("stdev", m);
            let r = ind::bollinger(c.closes, p, m);
            out.insert("upper".into(), r.upper);
            out.insert("mid".into(), r.middle);
            out.insert("lower".into(), r.lower);
        }
        "macd" => {
            let f = load_num(pool, name, "fast", 12).await as usize;
            let s = load_num(pool, name, "slow", 26).await as usize;
            let sig = load_num(pool, name, "signal", 9).await as usize;
            note_num!("fast", f as i64);
            note_num!("slow", s as i64);
            note_num!("signal", sig as i64);
            let r = ind::macd(c.closes, f, s, sig);
            out.insert("macd".into(), r.macd_line);
            out.insert("signal".into(), r.signal_line);
            out.insert("hist".into(), r.histogram);
        }
        "atr" => {
            let p = load_num(pool, name, "period", 14).await as usize;
            note_num!("period", p as i64);
            out.insert("atr".into(), ind::atr(c.highs, c.lows, c.closes, p));
        }
        "supertrend" => {
            let p = load_num(pool, name, "period", 10).await as usize;
            let f = load_f64(pool, name, "factor", 3.0).await;
            note_num!("period", p as i64);
            note_f!("factor", f);
            let r = ind::supertrend(c.highs, c.lows, c.closes, p, f);
            out.insert("upper".into(), r.upper);
            out.insert("lower".into(), r.lower);
            out.insert(
                "trend".into(),
                r.trend.iter().map(|&x| x as f64).collect(),
            );
        }
        "keltner" => {
            let ep = load_num(pool, name, "ema_period", 20).await as usize;
            let ap = load_num(pool, name, "atr_period", 10).await as usize;
            let m = load_f64(pool, name, "mult", 2.0).await;
            note_num!("ema_period", ep as i64);
            note_num!("atr_period", ap as i64);
            note_f!("mult", m);
            let r = ind::keltner(c.highs, c.lows, c.closes, ep, ap, m);
            out.insert("upper".into(), r.upper);
            out.insert("mid".into(), r.mid);
            out.insert("lower".into(), r.lower);
        }
        "ichimoku" => {
            let t = load_num(pool, name, "tenkan", 9).await as usize;
            let k = load_num(pool, name, "kijun", 26).await as usize;
            let b = load_num(pool, name, "senkou_b", 52).await as usize;
            let sh = load_num(pool, name, "shift", 26).await as usize;
            note_num!("tenkan", t as i64);
            note_num!("kijun", k as i64);
            note_num!("senkou_b", b as i64);
            note_num!("shift", sh as i64);
            let r = ind::ichimoku(c.highs, c.lows, c.closes, t, k, b, sh);
            out.insert("tenkan".into(), r.tenkan);
            out.insert("kijun".into(), r.kijun);
            out.insert("senkou_a".into(), r.senkou_a);
            out.insert("senkou_b".into(), r.senkou_b);
            out.insert("chikou".into(), r.chikou);
        }
        "donchian" => {
            let p = load_num(pool, name, "period", 20).await as usize;
            note_num!("period", p as i64);
            let r = ind::donchian(c.highs, c.lows, p);
            out.insert("upper".into(), r.upper);
            out.insert("lower".into(), r.lower);
            out.insert("mid".into(), r.mid);
        }
        "williams_r" => {
            let p = load_num(pool, name, "period", 14).await as usize;
            note_num!("period", p as i64);
            out.insert(
                "williams_r".into(),
                ind::williams_r(c.highs, c.lows, c.closes, p),
            );
        }
        "cmf" => {
            let p = load_num(pool, name, "period", 20).await as usize;
            note_num!("period", p as i64);
            out.insert(
                "cmf".into(),
                ind::cmf(c.highs, c.lows, c.closes, c.volumes, p),
            );
        }
        "aroon" => {
            let p = load_num(pool, name, "period", 25).await as usize;
            note_num!("period", p as i64);
            let r = ind::aroon(c.highs, c.lows, p);
            out.insert("up".into(), r.up);
            out.insert("down".into(), r.down);
            out.insert("osc".into(), r.osc);
        }
        "psar" => {
            let s = load_f64(pool, name, "acc_start", 0.02).await;
            let step = load_f64(pool, name, "acc_step", 0.02).await;
            let mx = load_f64(pool, name, "acc_max", 0.2).await;
            note_f!("acc_start", s);
            note_f!("acc_step", step);
            note_f!("acc_max", mx);
            let r = ind::psar(c.highs, c.lows, s, step, mx);
            out.insert("sar".into(), r.sar);
            out.insert(
                "trend".into(),
                r.trend.iter().map(|&x| x as f64).collect(),
            );
        }
        "chandelier" => {
            let p = load_num(pool, name, "period", 22).await as usize;
            let m = load_f64(pool, name, "mult", 3.0).await;
            note_num!("period", p as i64);
            note_f!("mult", m);
            let r = ind::chandelier(c.highs, c.lows, c.closes, p, m);
            out.insert("long_exit".into(), r.long_exit);
            out.insert("short_exit".into(), r.short_exit);
        }
        _ => return None,
    }
    // Deterministic hash across the config params we actually read.
    cfg_parts.sort_by(|a, b| a.0.cmp(&b.0));
    let blob: String = cfg_parts
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(";");
    let hash = format!("{:x}", Sha256::digest(blob.as_bytes()))[..16].to_string();
    Some((out, hash))
}

// ── Upsert ─────────────────────────────────────────────────────────────

async fn upsert_snapshot(
    pool: &PgPool,
    sym: &EngineSymbol,
    bar_time: DateTime<Utc>,
    indicator: &str,
    values: &Value,
    cfg_hash: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO indicator_snapshots
              (exchange, segment, symbol, timeframe, bar_time, indicator, values, config_hash)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
           ON CONFLICT (exchange, segment, symbol, timeframe, bar_time, indicator)
           DO UPDATE SET
               values      = EXCLUDED.values,
               config_hash = EXCLUDED.config_hash,
               computed_at = now()"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .bind(bar_time)
    .bind(indicator)
    .bind(values)
    .bind(cfg_hash)
    .execute(pool)
    .await?;
    Ok(())
}

async fn purge_expired(pool: &PgPool) {
    let days = load_num_json(pool, "retention_days", "days", 90).await.max(1);
    let _ = sqlx::query(
        "DELETE FROM indicator_snapshots WHERE computed_at < now() - ($1 || ' days')::interval",
    )
    .bind(days.to_string())
    .execute(pool)
    .await;
}

// ── Config loaders ─────────────────────────────────────────────────────

async fn load_master_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'indicator_persistence' AND config_key = 'enabled'",
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
    load_num_json(pool, "tick_secs", "secs", 60).await.max(15) as u64
}

async fn load_names(pool: &PgPool) -> Vec<String> {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'indicator_persistence' AND config_key = 'names'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else {
        return default_names();
    };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("names")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_ascii_lowercase()))
                .collect()
        })
        .unwrap_or_else(default_names)
}

fn default_names() -> Vec<String> {
    vec![
        "rsi", "ema", "bollinger", "macd", "atr", "supertrend", "keltner", "ichimoku",
        "donchian", "williams_r", "cmf", "aroon", "psar", "chandelier",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

async fn load_num_json(pool: &PgPool, key: &str, field: &str, default: i64) -> i64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'indicator_persistence' AND config_key = $1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return default; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get(field).and_then(|v| v.as_i64()).unwrap_or(default)
}

async fn load_num(pool: &PgPool, name: &str, field: &str, default: i64) -> i64 {
    let key = format!("{name}.{field}");
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'indicators' AND config_key = $1",
    )
    .bind(&key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return default; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("value").and_then(|v| v.as_i64()).unwrap_or(default)
}

async fn load_f64(pool: &PgPool, name: &str, field: &str, default: f64) -> f64 {
    let key = format!("{name}.{field}");
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'indicators' AND config_key = $1",
    )
    .bind(&key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return default; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("value").and_then(|v| v.as_f64()).unwrap_or(default)
}

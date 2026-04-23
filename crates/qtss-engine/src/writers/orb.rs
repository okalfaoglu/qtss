// Workaround: rustc 1.95 `annotate_snippets` renderer ICE on dead-code
// lint. Silenced module-wide.
#![allow(dead_code)]

//! Opening Range Breakout (ORB) writer — eighth engine-dispatch
//! member. Watches every crypto session open (Asia / London / NY),
//! measures the first `N` bars' high–low range as the Opening Range,
//! and emits a `detections` row as soon as a subsequent bar closes
//! outside that range. The detection's anchors carry the OR high, OR
//! low, and breakout bar so the chart can render the three as a
//! classic horizontal-band + trigger marker (like Toby Crabel's
//! original ORB plot).
//!
//! The detector is bar-driven and session-aware — it does not need
//! pivots, which is why it lives on its own rather than being folded
//! into `qtss-classical`. Classical's scallop / rectangle / flag
//! detectors already have their own geometry conventions that don't
//! map cleanly onto time-of-day logic.
//!
//! Config (`system_config`, module = 'orb'):
//!   * `enabled`              → `{ "enabled": true }`
//!   * `bars_per_tick`        → `{ "bars": 2000 }`
//!   * `or_bars`              → `{ "value": 4 }`  — 1-hour OR on 15m TF
//!   * `confirm_lookback`     → `{ "value": 12 }` — bars to watch for breakout
//!   * `breakout_atr_mult`    → `{ "value": 0.10 }` — min magnitude vs ATR
//!   * `volume_spike_mult`    → `{ "value": 1.2 }` — optional volume filter
//!   * `enabled_sessions`     → `{ "names": ["asia","london","new_york"] }`

use async_trait::async_trait;
use chrono::{DateTime, Timelike, Utc};
use qtss_regime::session::{classify_crypto, TradingSession};
use qtss_storage::market_bars::{self, MarketBarRow};
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::{self, EngineSymbol};
use crate::writer::{RunStats, WriterTask};

pub struct OrbWriter;

#[async_trait]
impl WriterTask for OrbWriter {
    fn family_name(&self) -> &'static str {
        "orb"
    }

    async fn run_once(&self, pool: &PgPool) -> anyhow::Result<RunStats> {
        let mut stats = RunStats::default();
        let syms = symbols::list_enabled(pool).await?;
        let cfg = load_config(pool).await;
        for sym in &syms {
            match process_symbol(pool, sym, &cfg).await {
                Ok(n) => {
                    stats.series_processed += 1;
                    stats.rows_upserted += n;
                }
                Err(e) => warn!(
                    exchange = %sym.exchange,
                    symbol = %sym.symbol,
                    tf = %sym.interval,
                    %e,
                    "orb: series failed"
                ),
            }
        }
        Ok(stats)
    }
}

struct OrbCfg {
    bars_per_tick: i64,
    or_bars: usize,
    confirm_lookback: usize,
    breakout_atr_mult: f64,
    volume_spike_mult: f64,
    enabled_sessions: Vec<TradingSession>,
}

async fn process_symbol(
    pool: &PgPool,
    sym: &EngineSymbol,
    cfg: &OrbCfg,
) -> anyhow::Result<usize> {
    let raw = market_bars::list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        cfg.bars_per_tick,
    )
    .await?;
    let min_window = cfg.or_bars + cfg.confirm_lookback + 1;
    if raw.len() < min_window {
        return Ok(0);
    }
    let chrono: Vec<MarketBarRow> = raw.into_iter().rev().collect();

    // Simple ATR-14 over closes for magnitude gate.
    let atr = wilder_atr(&chrono, 14);
    // Average volume for the volume-spike gate.
    let avg_vol = avg_volume(&chrono, 20);

    let mut written = 0usize;
    // Walk bars, spot session-open crossings, project the OR forward,
    // then scan the breakout window.
    for i in 1..chrono.len() {
        let bar = &chrono[i];
        let prev = &chrono[i - 1];
        // Session cross: the previous bar is in a *different* session
        // than this one, and this session is in our enabled set.
        let cur_sess = classify_crypto(bar.open_time).primary;
        let prev_sess = classify_crypto(prev.open_time).primary;
        if cur_sess == prev_sess || !cfg.enabled_sessions.contains(&cur_sess) {
            continue;
        }
        // Need or_bars bars available from here to measure the OR.
        if i + cfg.or_bars >= chrono.len() {
            continue;
        }
        let or_window = &chrono[i..i + cfg.or_bars];
        let or_high = or_window
            .iter()
            .map(|b| b.high.to_f64().unwrap_or(0.0))
            .fold(f64::NEG_INFINITY, f64::max);
        let or_low = or_window
            .iter()
            .map(|b| b.low.to_f64().unwrap_or(0.0))
            .fold(f64::INFINITY, f64::min);
        if !or_high.is_finite() || !or_low.is_finite() || or_high <= or_low {
            continue;
        }
        // Scan up to confirm_lookback bars after the OR window for a
        // breakout close. We keep the *first* confirmed break (a
        // fakeout followed by a legit reversal creates an "orb fakeout"
        // which is its own pattern — deferred to PR-12).
        let scan_start = i + cfg.or_bars;
        let scan_end = (scan_start + cfg.confirm_lookback).min(chrono.len());
        let atr_here = atr.get(scan_start).copied().unwrap_or(0.0).max(1e-9);
        let min_magnitude = atr_here * cfg.breakout_atr_mult;

        for j in scan_start..scan_end {
            let close = chrono[j].close.to_f64().unwrap_or(0.0);
            let vol = chrono[j].volume.to_f64().unwrap_or(0.0);
            let (direction, trigger_price, variant) = if close > or_high + min_magnitude {
                (1i16, or_high, "bull")
            } else if close < or_low - min_magnitude {
                (-1i16, or_low, "bear")
            } else {
                continue;
            };
            let volume_confirmed = vol >= avg_vol * cfg.volume_spike_mult;

            let start_bar = i as i64;
            let end_bar = j as i64;
            let start_time = chrono[i].open_time;
            let end_time = chrono[j].open_time;
            let subkind = format!("orb_{}_{}", cur_sess.as_str(), variant);
            let anchors = json!([
                {
                    "label_override": "OR high",
                    "bar_index": start_bar,
                    "price": or_high,
                    "time": start_time,
                },
                {
                    "label_override": "OR low",
                    "bar_index": start_bar,
                    "price": or_low,
                    "time": start_time,
                },
                {
                    "label_override": "Break",
                    "bar_index": end_bar,
                    "price": close,
                    "time": end_time,
                }
            ]);
            let raw_meta = json!({
                "or_high":          or_high,
                "or_low":           or_low,
                "trigger_price":    trigger_price,
                "breakout_close":   close,
                "session":          cur_sess.as_str(),
                "or_bars":          cfg.or_bars,
                "atr":              atr_here,
                "volume_confirmed": volume_confirmed,
                "score":            if volume_confirmed { 0.75 } else { 0.55 },
            });
            upsert(
                pool, sym, &subkind, direction, start_bar, end_bar, start_time, end_time,
                &anchors, &raw_meta,
            )
            .await?;
            written += 1;
            break; // first confirmed breakout only
        }
    }
    Ok(written)
}

fn wilder_atr(rows: &[MarketBarRow], period: usize) -> Vec<f64> {
    let n = rows.len();
    let mut out = vec![0.0; n];
    if n < 2 || period == 0 {
        return out;
    }
    let mut prev_atr = 0.0;
    let mut sum_tr = 0.0;
    for i in 1..n {
        let h = rows[i].high.to_f64().unwrap_or(0.0);
        let l = rows[i].low.to_f64().unwrap_or(0.0);
        let pc = rows[i - 1].close.to_f64().unwrap_or(0.0);
        let tr = (h - l).max((h - pc).abs()).max((l - pc).abs());
        if i <= period {
            sum_tr += tr;
            if i == period {
                prev_atr = sum_tr / period as f64;
                out[i] = prev_atr;
            }
        } else {
            prev_atr = (prev_atr * (period - 1) as f64 + tr) / period as f64;
            out[i] = prev_atr;
        }
    }
    out
}

fn avg_volume(rows: &[MarketBarRow], window: usize) -> f64 {
    let n = rows.len();
    if n == 0 {
        return 0.0;
    }
    let take = window.min(n);
    let start = n - take;
    let sum: f64 = rows[start..]
        .iter()
        .map(|r| r.volume.to_f64().unwrap_or(0.0))
        .sum();
    sum / take as f64
}

#[allow(clippy::too_many_arguments)]
async fn upsert(
    pool: &PgPool,
    sym: &EngineSymbol,
    subkind: &str,
    direction: i16,
    start_bar: i64,
    end_bar: i64,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    anchors: &Value,
    raw_meta: &Value,
) -> anyhow::Result<usize> {
    sqlx::query(
        r#"INSERT INTO detections
              (exchange, segment, symbol, timeframe, slot,
               pattern_family, subkind, direction,
               start_bar, end_bar, start_time, end_time,
               anchors, invalidated, raw_meta, mode)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,'live')
           ON CONFLICT (exchange, segment, symbol, timeframe, slot,
                        pattern_family, subkind, start_time, end_time, mode)
           DO UPDATE SET
               direction     = EXCLUDED.direction,
               start_bar     = EXCLUDED.start_bar,
               end_bar       = EXCLUDED.end_bar,
               anchors       = EXCLUDED.anchors,
               raw_meta      = EXCLUDED.raw_meta,
               updated_at    = now()"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .bind(0i16)
    .bind("orb")
    .bind(subkind)
    .bind(direction)
    .bind(start_bar)
    .bind(end_bar)
    .bind(start_time)
    .bind(end_time)
    .bind(anchors)
    .bind(false)
    .bind(raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}

// ── Config loading ─────────────────────────────────────────────────────

async fn load_config(pool: &PgPool) -> OrbCfg {
    OrbCfg {
        bars_per_tick: load_i64(pool, "bars_per_tick", "bars", 2000).await.clamp(200, 10_000),
        or_bars: load_i64(pool, "or_bars", "value", 4).await.max(1) as usize,
        confirm_lookback: load_i64(pool, "confirm_lookback", "value", 12).await.max(1) as usize,
        breakout_atr_mult: load_f64(pool, "breakout_atr_mult", "value", 0.10).await,
        volume_spike_mult: load_f64(pool, "volume_spike_mult", "value", 1.2).await,
        enabled_sessions: load_sessions(pool).await,
    }
}

async fn load_i64(pool: &PgPool, key: &str, field: &str, default: i64) -> i64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'orb' AND config_key = $1",
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

async fn load_f64(pool: &PgPool, key: &str, field: &str, default: f64) -> f64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'orb' AND config_key = $1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return default; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get(field).and_then(|v| v.as_f64()).unwrap_or(default)
}

async fn load_sessions(pool: &PgPool) -> Vec<TradingSession> {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'orb' AND config_key = 'enabled_sessions'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let default_sessions = vec![TradingSession::Asia, TradingSession::London, TradingSession::NewYork];
    let Some(row) = row else { return default_sessions; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    let names: Vec<String> = val
        .get("names")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_ascii_lowercase()))
                .collect()
        })
        .unwrap_or_default();
    if names.is_empty() {
        return default_sessions;
    }
    names
        .into_iter()
        .filter_map(|n| match n.as_str() {
            "asia" => Some(TradingSession::Asia),
            "london" => Some(TradingSession::London),
            "new_york" | "ny" | "newyork" => Some(TradingSession::NewYork),
            _ => None,
        })
        .collect()
}

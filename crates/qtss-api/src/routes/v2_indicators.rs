//! `GET /v2/indicators/{venue}/{symbol}/{tf}?names=rsi,supertrend,...`
//!
//! Compute-on-demand endpoint for the 11 new technical indicators (and
//! the handful of existing ones the chart wants to stream). One round
//! trip returns a `{name → values[]}` map aligned to the same bar
//! series the `/v2/zigzag` endpoint uses, so the frontend can overlay
//! SuperTrend / Ichimoku / Keltner / RSI pane data without re-fetching
//! candles.
//!
//! Config (CLAUDE.md #2 — every period lives in `system_config.indicators.*`):
//!   * `indicators.<name>.period`   → `{ "value": 14 }`
//!   * `indicators.<name>.factor`   → `{ "value": 3.0 }` (SuperTrend, Chandelier)
//!   * Ichimoku: `.tenkan`, `.kijun`, `.senkou_b`, `.shift`
//!   * Keltner:  `.ema_period`, `.atr_period`, `.mult`
//!   * PSAR:     `.acc_start`, `.acc_step`, `.acc_max`
//!   * TTM Sq.:  `.bb_period`, `.bb_stdev`, `.kc_period`, `.kc_atr`, `.kc_mult`
//!
//! Each indicator maps through a small closure in `INDICATOR_SPECS` so
//! adding a new one is a single entry (CLAUDE.md #1 — dispatch table).

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use std::collections::HashMap;

use qtss_indicators as ind;
use qtss_storage::market_bars;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct IndicatorsQuery {
    pub segment: Option<String>,
    pub limit: Option<i64>,
    /// Comma-separated indicator names. When absent we default to the
    /// chart's "base" overlay set (ema, bollinger, rsi).
    pub names: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct IndicatorsResponse {
    pub venue: String,
    pub symbol: String,
    pub timeframe: String,
    /// Bar open times + indices — callers align series[i] to bars[i].
    pub bars: Vec<BarStamp>,
    /// Per-indicator: a map of sub-series name → values[] (NaN padded).
    /// Multi-output indicators (MACD, Ichimoku) expose each line as its
    /// own sub-series so the frontend can toggle lines independently.
    pub series: HashMap<String, HashMap<String, Vec<f64>>>,
}

#[derive(Debug, Serialize, Clone)]
pub struct BarStamp {
    pub bar_index: i64,
    pub time: chrono::DateTime<chrono::Utc>,
}

pub fn v2_indicators_router() -> Router<SharedState> {
    Router::new().route(
        "/v2/indicators/{venue}/{symbol}/{tf}",
        get(get_indicators),
    )
}

async fn get_indicators(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<IndicatorsQuery>,
) -> Result<Json<IndicatorsResponse>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(1000).clamp(50, 10_000);
    let names: Vec<String> = q
        .names
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_ascii_lowercase())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_else(|| vec!["rsi".into(), "ema".into(), "bollinger".into()]);

    let rows =
        market_bars::list_recent_bars(&st.pool, &venue, &segment, &symbol, &tf, limit).await?;
    // DB returns newest-first — flip to chronological.
    // Note: we intentionally skip the live-forming open bar here; all
    // indicators compute over closed bars to avoid last-value flicker
    // as the live price updates sub-bar. The chart shows the live bar
    // separately via `/v2/zigzag`.
    let bars_chrono: Vec<market_bars::MarketBarRow> = rows.into_iter().rev().collect();

    let highs: Vec<f64> = bars_chrono
        .iter()
        .map(|b| b.high.to_f64().unwrap_or(0.0))
        .collect();
    let lows: Vec<f64> = bars_chrono
        .iter()
        .map(|b| b.low.to_f64().unwrap_or(0.0))
        .collect();
    let closes: Vec<f64> = bars_chrono
        .iter()
        .map(|b| b.close.to_f64().unwrap_or(0.0))
        .collect();
    let volumes: Vec<f64> = bars_chrono
        .iter()
        .map(|b| b.volume.to_f64().unwrap_or(0.0))
        .collect();

    let bar_stamps: Vec<BarStamp> = bars_chrono
        .iter()
        .enumerate()
        .map(|(i, b)| BarStamp {
            bar_index: i as i64,
            time: b.open_time,
        })
        .collect();

    // Resolve each requested indicator through the dispatch table.
    let ctx = IndicatorCtx {
        highs: &highs,
        lows: &lows,
        closes: &closes,
        volumes: &volumes,
    };
    let mut series: HashMap<String, HashMap<String, Vec<f64>>> = HashMap::new();
    for name in &names {
        if let Some(computed) = compute_indicator(&st, name, &ctx).await {
            series.insert(name.clone(), computed);
        }
    }

    Ok(Json(IndicatorsResponse {
        venue,
        symbol,
        timeframe: tf,
        bars: bar_stamps,
        series,
    }))
}

struct IndicatorCtx<'a> {
    highs: &'a [f64],
    lows: &'a [f64],
    closes: &'a [f64],
    volumes: &'a [f64],
}

/// Per-indicator dispatcher. Each arm reads its config knobs via
/// `load_num` / `load_f64` helpers and returns a `{sub → values}` map.
/// Unknown names return None and are silently skipped.
async fn compute_indicator(
    st: &SharedState,
    name: &str,
    c: &IndicatorCtx<'_>,
) -> Option<HashMap<String, Vec<f64>>> {
    let mut out: HashMap<String, Vec<f64>> = HashMap::new();
    match name {
        "rsi" => {
            let p = load_num(&st.pool, name, "period", 14).await as usize;
            out.insert("rsi".into(), ind::rsi(c.closes, p));
        }
        "ema" => {
            // Multi-period EMA is useful on the chart; expose fast / slow
            // as separate sub-series so the caller can pick which to show.
            let fast = load_num(&st.pool, name, "fast", 9).await as usize;
            let slow = load_num(&st.pool, name, "slow", 21).await as usize;
            out.insert(format!("ema_{fast}"), ind::ema(c.closes, fast));
            out.insert(format!("ema_{slow}"), ind::ema(c.closes, slow));
        }
        "sma" => {
            let p = load_num(&st.pool, name, "period", 200).await as usize;
            out.insert(format!("sma_{p}"), ind::sma(c.closes, p));
        }
        "bollinger" => {
            let p = load_num(&st.pool, name, "period", 20).await as usize;
            let mult = load_f64(&st.pool, name, "stdev", 2.0).await;
            let r = ind::bollinger(c.closes, p, mult);
            out.insert("upper".into(), r.upper);
            out.insert("mid".into(), r.middle.clone());
            out.insert("lower".into(), r.lower);
        }
        "macd" => {
            let fast = load_num(&st.pool, name, "fast", 12).await as usize;
            let slow = load_num(&st.pool, name, "slow", 26).await as usize;
            let signal = load_num(&st.pool, name, "signal", 9).await as usize;
            let r = ind::macd(c.closes, fast, slow, signal);
            out.insert("macd".into(), r.macd_line);
            out.insert("signal".into(), r.signal_line);
            out.insert("hist".into(), r.histogram);
        }
        "stochastic" => {
            let k = load_num(&st.pool, name, "k", 14).await as usize;
            let d = load_num(&st.pool, name, "d", 3).await as usize;
            let r = ind::stochastic(c.highs, c.lows, c.closes, k, d);
            out.insert("k".into(), r.k);
            out.insert("d".into(), r.d);
        }
        "atr" => {
            let p = load_num(&st.pool, name, "period", 14).await as usize;
            out.insert("atr".into(), ind::atr(c.highs, c.lows, c.closes, p));
        }
        "supertrend" => {
            let p = load_num(&st.pool, name, "period", 10).await as usize;
            let f = load_f64(&st.pool, name, "factor", 3.0).await;
            let r = ind::supertrend(c.highs, c.lows, c.closes, p, f);
            out.insert("upper".into(), r.upper);
            out.insert("lower".into(), r.lower);
            out.insert(
                "trend".into(),
                r.trend.iter().map(|&x| x as f64).collect(),
            );
        }
        "ichimoku" => {
            let t = load_num(&st.pool, name, "tenkan", 9).await as usize;
            let k = load_num(&st.pool, name, "kijun", 26).await as usize;
            let b = load_num(&st.pool, name, "senkou_b", 52).await as usize;
            let shift = load_num(&st.pool, name, "shift", 26).await as usize;
            let r = ind::ichimoku(c.highs, c.lows, c.closes, t, k, b, shift);
            out.insert("tenkan".into(), r.tenkan);
            out.insert("kijun".into(), r.kijun);
            out.insert("senkou_a".into(), r.senkou_a);
            out.insert("senkou_b".into(), r.senkou_b);
            out.insert("chikou".into(), r.chikou);
        }
        "donchian" => {
            let p = load_num(&st.pool, name, "period", 20).await as usize;
            let r = ind::donchian(c.highs, c.lows, p);
            out.insert("upper".into(), r.upper);
            out.insert("lower".into(), r.lower);
            out.insert("mid".into(), r.mid);
        }
        "keltner" => {
            let ep = load_num(&st.pool, name, "ema_period", 20).await as usize;
            let ap = load_num(&st.pool, name, "atr_period", 10).await as usize;
            let m = load_f64(&st.pool, name, "mult", 2.0).await;
            let r = ind::keltner(c.highs, c.lows, c.closes, ep, ap, m);
            out.insert("upper".into(), r.upper);
            out.insert("mid".into(), r.mid);
            out.insert("lower".into(), r.lower);
        }
        "williams_r" => {
            let p = load_num(&st.pool, name, "period", 14).await as usize;
            out.insert("williams_r".into(), ind::williams_r(c.highs, c.lows, c.closes, p));
        }
        "cmf" => {
            let p = load_num(&st.pool, name, "period", 20).await as usize;
            out.insert("cmf".into(), ind::cmf(c.highs, c.lows, c.closes, c.volumes, p));
        }
        "aroon" => {
            let p = load_num(&st.pool, name, "period", 25).await as usize;
            let r = ind::aroon(c.highs, c.lows, p);
            out.insert("up".into(), r.up);
            out.insert("down".into(), r.down);
            out.insert("osc".into(), r.osc);
        }
        "ad_line" => {
            out.insert("ad".into(), ind::ad_line(c.highs, c.lows, c.closes, c.volumes));
        }
        "psar" => {
            let s = load_f64(&st.pool, name, "acc_start", 0.02).await;
            let step = load_f64(&st.pool, name, "acc_step", 0.02).await;
            let mx = load_f64(&st.pool, name, "acc_max", 0.2).await;
            let r = ind::psar(c.highs, c.lows, s, step, mx);
            out.insert("sar".into(), r.sar);
            out.insert(
                "trend".into(),
                r.trend.iter().map(|&x| x as f64).collect(),
            );
        }
        "chandelier" => {
            let p = load_num(&st.pool, name, "period", 22).await as usize;
            let m = load_f64(&st.pool, name, "mult", 3.0).await;
            let r = ind::chandelier(c.highs, c.lows, c.closes, p, m);
            out.insert("long_exit".into(), r.long_exit);
            out.insert("short_exit".into(), r.short_exit);
        }
        "ttm_squeeze" => {
            let bp = load_num(&st.pool, name, "bb_period", 20).await as usize;
            let bs = load_f64(&st.pool, name, "bb_stdev", 2.0).await;
            let kp = load_num(&st.pool, name, "kc_period", 20).await as usize;
            let ka = load_num(&st.pool, name, "kc_atr_period", 10).await as usize;
            let km = load_f64(&st.pool, name, "kc_mult", 1.5).await;
            let flags = ind::ttm_squeeze(c.highs, c.lows, c.closes, bp, bs, kp, ka, km);
            out.insert(
                "squeeze".into(),
                flags.iter().map(|&b| if b { 1.0 } else { 0.0 }).collect(),
            );
        }
        "cvd" => {
            // Using MFM-sign proxy for CVD when aggtrade is unavailable;
            // same shape (cumulative series).
            let pseudo_deltas: Vec<f64> = c
                .closes
                .iter()
                .enumerate()
                .map(|(i, &close)| {
                    if i == 0 {
                        0.0
                    } else {
                        let sign = (close - c.closes[i - 1]).signum();
                        sign * c.volumes[i]
                    }
                })
                .collect();
            let mut cum = 0.0;
            let cvd_series: Vec<f64> = pseudo_deltas
                .iter()
                .map(|&d| {
                    cum += d;
                    cum
                })
                .collect();
            out.insert("cvd".into(), cvd_series);
        }
        _ => return None,
    }
    Some(out)
}

// ── Config helpers ─────────────────────────────────────────────────────
//
// `system_config` rows for indicators share module = 'indicators' and
// config_key = '<name>.<field>', value = `{ "value": <number> }`.

async fn load_num(pool: &sqlx::PgPool, name: &str, field: &str, default: i64) -> i64 {
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

async fn load_f64(pool: &sqlx::PgPool, name: &str, field: &str, default: f64) -> f64 {
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

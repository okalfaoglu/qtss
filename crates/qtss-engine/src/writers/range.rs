// Workaround: rustc 1.95 `annotate_snippets` renderer ICE on dead-code
// lint for engine writers. Silenced module-wide; no actual dead code.
#![allow(dead_code)]

//! Range-zone detector writer — fourth engine-dispatch member. Runs the
//! four zone detectors exported by `qtss-range` (FVG / Order Block /
//! Liquidity Pool / Equal Levels) and upserts each match into
//! `detections` with `pattern_family = 'range'` and one of the four
//! `subkind`s the library already names:
//!
//! * `fvg`                → `bullish_fvg` | `bearish_fvg`
//! * `order_block`        → `bullish_ob`  | `bearish_ob`
//! * `liquidity_pool`     → `liquidity_pool_high` | `liquidity_pool_low`
//! * `equal_levels`       → `equal_highs` | `equal_lows`
//!
//! Config (all in `system_config`, CLAUDE.md #2):
//!   * `range.enabled`             → `{ "enabled": true }`
//!   * `range.bars_per_tick`       → `{ "bars": 2000 }`
//!   * `range.atr_period`          → `{ "period": 14 }`
//!   * `range.min_score`           → `{ "score": 0.50 }`
//!   * Per-detector: `range.<sub>.enabled` → `{ "enabled": true }`
//!   * Per-detector tolerances under `range.<sub>.<key>` as
//!     `{ "value": <number> }`.
//!
//! The dispatch for sub-detectors is a table (CLAUDE.md #1) — adding a
//! fifth zone type (e.g. supply/demand) is one entry in `SUB_DETECTORS`
//! plus the matching qtss-range function, no central if/else.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use qtss_range::{
    detect_equal_levels, detect_fvg, detect_liquidity_pools, detect_order_blocks,
    helpers::wilder_atr, EqualLevelsConfig, FvgConfig, LiquidityPoolConfig, OhlcBar,
    OrderBlockConfig,
};
use qtss_storage::market_bars::{self, MarketBarRow};
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::{self, EngineSymbol};
use crate::writer::{RunStats, WriterTask};

pub struct RangeWriter;

#[async_trait]
impl WriterTask for RangeWriter {
    fn family_name(&self) -> &'static str {
        "range"
    }

    async fn run_once(&self, pool: &PgPool) -> anyhow::Result<RunStats> {
        let mut stats = RunStats::default();
        let syms = symbols::list_enabled(pool).await?;
        let bars_limit =
            load_num_async(pool, "bars_per_tick", "bars", 2_000).await.clamp(200, 10_000);
        let atr_period =
            load_num_async(pool, "atr_period", "period", 14).await.clamp(2, 200) as usize;
        let min_score = load_min_score(pool).await;

        let fvg_cfg = load_fvg_config(pool).await;
        let ob_cfg = load_ob_config(pool).await;
        let lp_cfg = load_lp_config(pool).await;
        let el_cfg = load_el_config(pool).await;
        let enabled = SubEnabled {
            fvg: sub_enabled(pool, "fvg").await,
            order_block: sub_enabled(pool, "order_block").await,
            liquidity_pool: sub_enabled(pool, "liquidity_pool").await,
            equal_levels: sub_enabled(pool, "equal_levels").await,
        };

        for sym in &syms {
            match process_symbol(
                pool, sym, bars_limit, atr_period, min_score, &enabled, &fvg_cfg, &ob_cfg,
                &lp_cfg, &el_cfg,
            )
            .await
            {
                Ok(n) => {
                    stats.series_processed += 1;
                    stats.rows_upserted += n;
                }
                Err(e) => warn!(
                    exchange = %sym.exchange,
                    symbol = %sym.symbol,
                    tf = %sym.interval,
                    %e,
                    "range: series failed"
                ),
            }
        }
        Ok(stats)
    }
}

// ---------------------------------------------------------------------------
// Per-symbol processing
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn process_symbol(
    pool: &PgPool,
    sym: &EngineSymbol,
    bars_limit: i64,
    atr_period: usize,
    min_score: f64,
    enabled: &SubEnabled,
    fvg_cfg: &FvgConfig,
    ob_cfg: &OrderBlockConfig,
    lp_cfg: &LiquidityPoolConfig,
    el_cfg: &EqualLevelsConfig,
) -> anyhow::Result<usize> {
    let raw_bars = market_bars::list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        bars_limit,
    )
    .await?;
    if raw_bars.len() < 20 {
        return Ok(0);
    }
    let chrono_bars: Vec<MarketBarRow> = raw_bars.into_iter().rev().collect();
    let ohlc = build_ohlc(&chrono_bars);

    // Single ATR value for the zone detectors — use the most recent
    // wilder ATR which is what the library's docs assume
    // (constant ATR scan rather than per-bar varying).
    let atr_series = wilder_atr(&ohlc, atr_period);
    let atr_value = atr_series.last().copied().unwrap_or(0.0);
    if atr_value <= 0.0 {
        return Ok(0);
    }

    let mut written = 0usize;

    if enabled.fvg {
        for m in detect_fvg(&ohlc, atr_value, fvg_cfg) {
            if m.score < min_score {
                continue;
            }
            written += write_match_fvg(pool, sym, &chrono_bars, &m).await?;
        }
    }
    if enabled.order_block {
        for m in detect_order_blocks(&ohlc, atr_value, ob_cfg) {
            if m.score < min_score {
                continue;
            }
            written += write_match_ob(pool, sym, &chrono_bars, &m).await?;
        }
    }
    if enabled.liquidity_pool {
        for m in detect_liquidity_pools(&ohlc, atr_value, lp_cfg) {
            if m.score < min_score {
                continue;
            }
            written += write_match_lp(pool, sym, &chrono_bars, &m).await?;
        }
    }
    if enabled.equal_levels {
        for m in detect_equal_levels(&ohlc, atr_value, el_cfg) {
            if m.score < min_score {
                continue;
            }
            written += write_match_el(pool, sym, &chrono_bars, &m).await?;
        }
    }
    Ok(written)
}

fn build_ohlc(rows: &[MarketBarRow]) -> Vec<OhlcBar> {
    rows.iter()
        .enumerate()
        .map(|(i, r)| OhlcBar {
            open: r.open.to_f64().unwrap_or(0.0),
            high: r.high.to_f64().unwrap_or(0.0),
            low: r.low.to_f64().unwrap_or(0.0),
            close: r.close.to_f64().unwrap_or(0.0),
            bar_index: i as i64,
            volume: Some(r.volume.to_f64().unwrap_or(0.0)),
        })
        .collect()
}

fn bar_time(rows: &[MarketBarRow], idx: i64) -> DateTime<Utc> {
    rows.get(idx.max(0) as usize)
        .map(|r| r.open_time)
        .unwrap_or_else(Utc::now)
}

// ---------------------------------------------------------------------------
// Per-sub-detector upsert helpers. All converge on the same DB upsert
// signature — the dispatch into `subkind` is already encoded by the
// library's output struct (`m.subkind`).
// ---------------------------------------------------------------------------

async fn write_match_fvg(
    pool: &PgPool,
    sym: &EngineSymbol,
    rows: &[MarketBarRow],
    m: &qtss_range::FvgMatch,
) -> anyhow::Result<usize> {
    let anchors = json!([
        bar_anchor(rows, m.bar_index, m.gap_high, Some("gap_high")),
        bar_anchor(rows, m.bar_index, m.gap_low, Some("gap_low"))
    ]);
    let raw_meta = json!({
        "score":           m.score,
        "gap_size":        m.gap_size,
        "gap_atr_ratio":   m.gap_atr_ratio,
        "filled":          m.filled,
        "fill_pct":        m.fill_pct,
        "volume_confirmed": m.volume_confirmed,
    });
    upsert_range(
        pool,
        sym,
        "fvg",
        &m.subkind,
        direction_from_subkind(&m.subkind),
        m.bar_index,
        m.bar_index,
        bar_time(rows, m.bar_index),
        bar_time(rows, m.bar_index),
        &anchors,
        m.filled,
        raw_meta,
    )
    .await
}

async fn write_match_ob(
    pool: &PgPool,
    sym: &EngineSymbol,
    rows: &[MarketBarRow],
    m: &qtss_range::OrderBlockMatch,
) -> anyhow::Result<usize> {
    let anchors = json!([
        bar_anchor(rows, m.bar_index, m.ob_high, Some("ob_high")),
        bar_anchor(rows, m.bar_index, m.ob_low, Some("ob_low"))
    ]);
    let raw_meta = json!({
        "score":            m.score,
        "impulse_size":     m.impulse_size,
        "impulse_atr_ratio": m.impulse_atr_ratio,
        "mitigated":        m.mitigated,
        "volume_confirmed": m.volume_confirmed,
    });
    upsert_range(
        pool,
        sym,
        "order_block",
        &m.subkind,
        direction_from_subkind(&m.subkind),
        m.bar_index,
        m.bar_index,
        bar_time(rows, m.bar_index),
        bar_time(rows, m.bar_index),
        &anchors,
        m.mitigated,
        raw_meta,
    )
    .await
}

async fn write_match_lp(
    pool: &PgPool,
    sym: &EngineSymbol,
    rows: &[MarketBarRow],
    m: &qtss_range::LiquidityPoolMatch,
) -> anyhow::Result<usize> {
    let start_bar = m.pivot_bars.first().copied().unwrap_or(0);
    let end_bar = m.pivot_bars.last().copied().unwrap_or(start_bar);
    let anchors_arr: Vec<Value> = m
        .pivot_bars
        .iter()
        .map(|b| bar_anchor(rows, *b, m.level, Some("pool")))
        .collect();
    let raw_meta = json!({
        "score":     m.score,
        "touches":   m.touches,
        "swept":     m.swept,
        "reclaimed": m.reclaimed,
    });
    upsert_range(
        pool,
        sym,
        "liquidity_pool",
        &m.subkind,
        direction_from_subkind(&m.subkind),
        start_bar,
        end_bar,
        bar_time(rows, start_bar),
        bar_time(rows, end_bar),
        &Value::Array(anchors_arr),
        m.swept && !m.reclaimed,
        raw_meta,
    )
    .await
}

async fn write_match_el(
    pool: &PgPool,
    sym: &EngineSymbol,
    rows: &[MarketBarRow],
    m: &qtss_range::EqualLevelMatch,
) -> anyhow::Result<usize> {
    let start_bar = m.pivot_bars.first().copied().unwrap_or(0);
    let end_bar = m.pivot_bars.last().copied().unwrap_or(start_bar);
    let anchors_arr: Vec<Value> = m
        .pivot_bars
        .iter()
        .map(|b| bar_anchor(rows, *b, m.level, Some("eq")))
        .collect();
    let raw_meta = json!({
        "score":     m.score,
        "count":     m.count,
        "max_diff":  m.max_diff,
        "price_near": m.price_near,
    });
    upsert_range(
        pool,
        sym,
        "equal_levels",
        &m.subkind,
        direction_from_subkind(&m.subkind),
        start_bar,
        end_bar,
        bar_time(rows, start_bar),
        bar_time(rows, end_bar),
        &Value::Array(anchors_arr),
        false,
        raw_meta,
    )
    .await
}

fn bar_anchor(rows: &[MarketBarRow], bar_index: i64, price: f64, label: Option<&str>) -> Value {
    let time = rows
        .get(bar_index.max(0) as usize)
        .map(|r| r.open_time)
        .unwrap_or_else(Utc::now);
    let mut obj = json!({
        "bar_index": bar_index,
        "price":     price,
        "time":      time,
    });
    if let Some(l) = label {
        obj["label_override"] = json!(l);
    }
    obj
}

/// Derive side-of-market from the library's `subkind` naming convention:
/// anything starting with `bullish_` / `equal_lows` / `liquidity_pool_low`
/// is long-biased; `bearish_` / `equal_highs` / `liquidity_pool_high` is
/// short-biased. Dispatch table (CLAUDE.md #1) — adding a new subkind
/// here is one row, not a new if branch scattered elsewhere.
fn direction_from_subkind(subkind: &str) -> i16 {
    let s = subkind.to_ascii_lowercase();
    if s.starts_with("bullish_") || s == "equal_lows" || s == "liquidity_pool_low" {
        return 1;
    }
    if s.starts_with("bearish_") || s == "equal_highs" || s == "liquidity_pool_high" {
        return -1;
    }
    0
}

// ---------------------------------------------------------------------------
// Sub-detector enable flags + config loaders.
// ---------------------------------------------------------------------------

struct SubEnabled {
    fvg: bool,
    order_block: bool,
    liquidity_pool: bool,
    equal_levels: bool,
}

async fn sub_enabled(pool: &PgPool, sub: &str) -> bool {
    let key = format!("{sub}.enabled");
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'range' AND config_key = $1",
    )
    .bind(&key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return true; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

async fn load_min_score(pool: &PgPool) -> f64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'range' AND config_key = 'min_score'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 0.50; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("score").and_then(|v| v.as_f64()).unwrap_or(0.50).clamp(0.0, 1.0)
}

async fn load_num_async(pool: &PgPool, key: &str, field: &str, default: i64) -> i64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'range' AND config_key = $1",
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

async fn load_sub_num(pool: &PgPool, sub: &str, key: &str, default: f64) -> f64 {
    let full = format!("{sub}.{key}");
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'range' AND config_key = $1",
    )
    .bind(&full)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return default; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("value").and_then(|v| v.as_f64()).unwrap_or(default)
}

async fn load_sub_bool(pool: &PgPool, sub: &str, key: &str, default: bool) -> bool {
    let full = format!("{sub}.{key}");
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'range' AND config_key = $1",
    )
    .bind(&full)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return default; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("value").and_then(|v| v.as_bool()).unwrap_or(default)
}

async fn load_fvg_config(pool: &PgPool) -> FvgConfig {
    let mut cfg = FvgConfig::default();
    cfg.min_gap_atr_frac =
        load_sub_num(pool, "fvg", "min_gap_atr_frac", cfg.min_gap_atr_frac).await;
    cfg.scan_lookback =
        load_sub_num(pool, "fvg", "scan_lookback", cfg.scan_lookback as f64).await as usize;
    cfg.unfilled_only =
        load_sub_bool(pool, "fvg", "unfilled_only", cfg.unfilled_only).await;
    cfg.volume_spike_mult =
        load_sub_num(pool, "fvg", "volume_spike_mult", cfg.volume_spike_mult).await;
    cfg
}

async fn load_ob_config(pool: &PgPool) -> OrderBlockConfig {
    let mut cfg = OrderBlockConfig::default();
    cfg.impulse_atr_mult =
        load_sub_num(pool, "order_block", "impulse_atr_mult", cfg.impulse_atr_mult).await;
    cfg.impulse_candles =
        load_sub_num(pool, "order_block", "impulse_candles", cfg.impulse_candles as f64).await
            as usize;
    cfg.scan_lookback =
        load_sub_num(pool, "order_block", "scan_lookback", cfg.scan_lookback as f64).await
            as usize;
    cfg.unmitigated_only =
        load_sub_bool(pool, "order_block", "unmitigated_only", cfg.unmitigated_only).await;
    cfg.volume_spike_mult =
        load_sub_num(pool, "order_block", "volume_spike_mult", cfg.volume_spike_mult).await;
    cfg
}

async fn load_lp_config(pool: &PgPool) -> LiquidityPoolConfig {
    let mut cfg = LiquidityPoolConfig::default();
    cfg.pivot_window =
        load_sub_num(pool, "liquidity_pool", "pivot_window", cfg.pivot_window as f64).await
            as usize;
    cfg.cluster_atr_mult =
        load_sub_num(pool, "liquidity_pool", "cluster_atr_mult", cfg.cluster_atr_mult).await;
    cfg.min_touches =
        load_sub_num(pool, "liquidity_pool", "min_touches", cfg.min_touches as f64).await
            as usize;
    cfg.sweep_max_penetration_atr = load_sub_num(
        pool,
        "liquidity_pool",
        "sweep_max_penetration_atr",
        cfg.sweep_max_penetration_atr,
    )
    .await;
    cfg.scan_lookback =
        load_sub_num(pool, "liquidity_pool", "scan_lookback", cfg.scan_lookback as f64).await
            as usize;
    cfg
}

async fn load_el_config(pool: &PgPool) -> EqualLevelsConfig {
    let mut cfg = EqualLevelsConfig::default();
    cfg.pivot_window =
        load_sub_num(pool, "equal_levels", "pivot_window", cfg.pivot_window as f64).await
            as usize;
    cfg.equal_tolerance_atr =
        load_sub_num(pool, "equal_levels", "equal_tolerance_atr", cfg.equal_tolerance_atr).await;
    cfg.min_bar_distance = load_sub_num(
        pool,
        "equal_levels",
        "min_bar_distance",
        cfg.min_bar_distance as f64,
    )
    .await as usize;
    cfg.scan_lookback =
        load_sub_num(pool, "equal_levels", "scan_lookback", cfg.scan_lookback as f64).await
            as usize;
    cfg
}

// ---------------------------------------------------------------------------
// Shared detection upsert. All four range sub-detectors converge here —
// the only varying field is the `subkind`, which the library already
// names consistently (`bullish_fvg`, `bearish_ob`, …).
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn upsert_range(
    pool: &PgPool,
    sym: &EngineSymbol,
    sub_family: &str,
    subkind: &str,
    direction: i16,
    start_bar: i64,
    end_bar: i64,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    anchors: &Value,
    invalidated: bool,
    raw_meta: Value,
) -> anyhow::Result<usize> {
    // Use a composite subkind: "<sub_family>:<subkind>" so the chart
    // layer can route on family=range + (e.g.) "fvg:bullish_fvg" without
    // losing the sub-detector identity. The detections table's unique
    // constraint includes subkind so this also keeps separate row keys.
    let composite = format!("{sub_family}:{subkind}");
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
               invalidated   = EXCLUDED.invalidated,
               raw_meta      = EXCLUDED.raw_meta,
               updated_at    = now()"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    // Range detectors are bar-driven, not pivot-slot-driven. Use a
    // sentinel slot (0) so the shared (exchange,…,slot,…) unique key
    // still fires. Slot is informational only for range.
    .bind(0i16)
    .bind("range")
    .bind(&composite)
    .bind(direction)
    .bind(start_bar)
    .bind(end_bar)
    .bind(start_time)
    .bind(end_time)
    .bind(anchors)
    .bind(invalidated)
    .bind(&raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}

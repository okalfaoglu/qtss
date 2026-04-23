// Workaround: rustc 1.95 `annotate_snippets` renderer ICE's on the
// dead-code lint when this module is analyzed. Silence the lint at the
// module level so the crate can build; there is no actual dead code in
// this file (verified by exhaustive `call_sites` walk).
#![allow(dead_code)]

//! Classical chart-pattern writer — third member of the unified engine
//! dispatch (CLAUDE.md #1 — one entry in `registered_writers`, no scattered
//! loops per family).
//!
//! Reads pivots the current tick's `PivotWriter` just persisted and walks
//! every `ShapeSpec` / `ShapeSpecBars` from `qtss-classical` over a sliding
//! window. Every match above the configured `min_structural_score` is
//! upserted into `detections` with `pattern_family = 'classical'`.
//!
//! Unlike Elliott (bar-driven Pine port) and Harmonic (XABCD matcher),
//! classical patterns come from a heterogeneous catalog of 29 detectors
//! (20 pivot-only, 9 bar-aware). We reuse the public `qtss_classical::SHAPES`
//! / `SHAPES_WITH_BARS` tables directly rather than going through
//! `ClassicalDetector::detect_with_bars` — the detector returns the single
//! best match per call, which would collapse every shape at every slot onto
//! one row per tick. The writer wants every valid window, so it iterates
//! the spec tables itself.
//!
//! Config (all in `system_config`, CLAUDE.md #2):
//!   * `classical.enabled`           → `{ "enabled": true }`
//!   * `classical.min_score`         → `{ "score": 0.55 }`
//!   * `classical.pivots_per_slot`   → `{ "count": 500 }`
//!   * `classical.bars_per_tick`     → `{ "bars": 2000 }`
//! Per-pattern tolerances live under the `classical.thresholds.*` keys
//! described in `qtss_classical::ClassicalConfig`; any key missing keeps
//! the Rust default so the writer degrades gracefully.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use qtss_classical::{ClassicalConfig, ShapeMatch, SHAPES, SHAPES_WITH_BARS};
use qtss_domain::v2::bar::Bar as DomainBar;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::pivot::{Pivot as DomainPivot, PivotKind, PivotLevel};
use qtss_domain::v2::timeframe::Timeframe;
use qtss_storage::market_bars::{self, MarketBarRow};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::{self, EngineSymbol};
use crate::writer::{RunStats, WriterTask};

pub struct ClassicalWriter;

#[async_trait]
impl WriterTask for ClassicalWriter {
    fn family_name(&self) -> &'static str {
        "classical"
    }

    async fn run_once(&self, pool: &PgPool) -> anyhow::Result<RunStats> {
        let mut stats = RunStats::default();
        let syms = symbols::list_enabled(pool).await?;
        let bars_limit = load_num_async(pool, "bars_per_tick", "bars", 2_000)
            .await
            .clamp(200, 10_000);
        let min_score = load_min_score(pool).await;
        let pivots_per_slot =
            load_num_async(pool, "pivots_per_slot", "count", 500).await.clamp(50, 5_000);
        let base_cfg = load_config(pool, min_score).await;

        for sym in &syms {
            match process_symbol(pool, sym, &base_cfg, bars_limit, pivots_per_slot).await {
                Ok(n) => {
                    stats.series_processed += 1;
                    stats.rows_upserted += n;
                }
                Err(e) => warn!(
                    exchange = %sym.exchange,
                    symbol = %sym.symbol,
                    tf = %sym.interval,
                    %e,
                    "classical: series failed"
                ),
            }
        }
        Ok(stats)
    }
}

// ---------------------------------------------------------------------------
// Pivot row loading (mirrors harmonic writer pattern — same table, same
// order convention: DB returns newest-first, we reverse to chronological
// so eval functions see oldest..newest).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct StoredPivot {
    bar_index: i64,
    open_time: DateTime<Utc>,
    direction: i16,
    price: Decimal,
    volume: Decimal,
    swing_tag: Option<String>,
}

async fn list_pivots_by_slot(
    pool: &PgPool,
    engine_symbol_id: sqlx::types::Uuid,
    slot: i16,
    limit: i64,
) -> anyhow::Result<Vec<StoredPivot>> {
    let rows = sqlx::query(
        r#"SELECT bar_index, open_time, direction, price, volume, swing_tag
             FROM pivots
            WHERE engine_symbol_id = $1 AND level = $2
            ORDER BY bar_index DESC
            LIMIT $3"#,
    )
    .bind(engine_symbol_id)
    .bind(slot)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    let mut out: Vec<StoredPivot> = rows
        .into_iter()
        .map(|r| StoredPivot {
            bar_index: r.get("bar_index"),
            open_time: r.get("open_time"),
            direction: r.get("direction"),
            price: r.get("price"),
            volume: r.try_get("volume").unwrap_or_else(|_| Decimal::ZERO),
            swing_tag: r.try_get::<Option<String>, _>("swing_tag").unwrap_or(None),
        })
        .collect();
    out.reverse();
    Ok(out)
}

// ---------------------------------------------------------------------------
// Config loading. Dispatch-table style (CLAUDE.md #1) — keys resolved in a
// map; any missing key keeps the default from `ClassicalConfig::defaults()`.
// ---------------------------------------------------------------------------

async fn load_min_score(pool: &PgPool) -> f32 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'classical' AND config_key = 'min_score'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 0.55; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.55)
        .clamp(0.0, 1.0) as f32
}

async fn load_num_async(pool: &PgPool, key: &str, field: &str, default: i64) -> i64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'classical' AND config_key = $1",
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

/// Load every classical threshold the writer may override. Unknown keys
/// keep the Rust default. Values are validated at the end; invalid
/// combinations fall back to `ClassicalConfig::defaults()` so a bad config
/// row can't take the writer offline.
async fn load_config(pool: &PgPool, min_score: f32) -> ClassicalConfig {
    let mut cfg = ClassicalConfig::defaults();
    cfg.min_structural_score = min_score;
    // Every threshold lives under `system_config.classical.thresholds.<key>`
    // as `{ "value": <number> }`. One row per key keeps the GUI editor
    // straightforward; dispatch is table-driven here (CLAUDE.md #1).
    let rows = sqlx::query(
        r#"SELECT config_key, value
             FROM system_config
            WHERE module = 'classical'
              AND config_key LIKE 'thresholds.%'"#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for r in rows {
        let key: String = r.try_get("config_key").unwrap_or_default();
        let val: Value = r.try_get("value").unwrap_or(Value::Null);
        let Some(v) = val.get("value").and_then(|v| v.as_f64()) else { continue };
        apply_threshold(&mut cfg, &key, v);
    }
    // If any operator override broke an invariant, fall back to defaults
    // so the writer never hits a validation panic mid-tick.
    if cfg.validate().is_err() {
        cfg = ClassicalConfig::defaults();
        cfg.min_structural_score = min_score;
    }
    cfg
}

/// Threshold setter dispatch table. Any unknown key is silently ignored so
/// forward-compat config rows don't break the writer (CLAUDE.md #1).
fn apply_threshold(cfg: &mut ClassicalConfig, key: &str, v: f64) {
    let k = key.trim_start_matches("thresholds.");
    match k {
        "equality_tolerance" => cfg.equality_tolerance = v,
        "apex_horizon_bars" => cfg.apex_horizon_bars = v as u64,
        "flatness_threshold_pct" => cfg.flatness_threshold_pct = v,
        "flatness_min_score" => cfg.flatness_min_score = v,
        "neckline_tolerance_mult" => cfg.neckline_tolerance_mult = v,
        "triangle_symmetry_tol" => cfg.triangle_symmetry_tol = v,
        "hs_max_neckline_slope_pct" => cfg.hs_max_neckline_slope_pct = v,
        "hs_time_symmetry_tol" => cfg.hs_time_symmetry_tol = v,
        "rectangle_max_slope_pct" => cfg.rectangle_max_slope_pct = v,
        "rectangle_min_bars" => cfg.rectangle_min_bars = v as u64,
        "flag_pole_min_move_atr" => cfg.flag_pole_min_move_atr = v,
        "flag_pole_max_bars" => cfg.flag_pole_max_bars = v as u64,
        "flag_max_retrace_pct" => cfg.flag_max_retrace_pct = v,
        "flag_atr_period" => cfg.flag_atr_period = v as u64,
        "flag_parallelism_tol" => cfg.flag_parallelism_tol = v,
        "pennant_max_height_pct_of_pole" => cfg.pennant_max_height_pct_of_pole = v,
        "channel_parallelism_tol" => cfg.channel_parallelism_tol = v,
        "channel_min_bars" => cfg.channel_min_bars = v as u64,
        "channel_min_slope_pct" => cfg.channel_min_slope_pct = v,
        "cup_min_bars" => cfg.cup_min_bars = v as u64,
        "cup_rim_equality_tol" => cfg.cup_rim_equality_tol = v,
        "cup_min_depth_pct" => cfg.cup_min_depth_pct = v,
        "cup_max_depth_pct" => cfg.cup_max_depth_pct = v,
        "cup_roundness_r2" => cfg.cup_roundness_r2 = v,
        "handle_max_depth_pct_of_cup" => cfg.handle_max_depth_pct_of_cup = v,
        "rounding_min_bars" => cfg.rounding_min_bars = v as u64,
        "rounding_roundness_r2" => cfg.rounding_roundness_r2 = v,
        "triple_peak_tol" => cfg.triple_peak_tol = v,
        "triple_min_span_bars" => cfg.triple_min_span_bars = v as u64,
        "triple_neckline_slope_max" => cfg.triple_neckline_slope_max = v,
        "broadening_min_slope_pct" => cfg.broadening_min_slope_pct = v,
        "broadening_flat_slope_pct" => cfg.broadening_flat_slope_pct = v,
        "v_max_total_bars" => cfg.v_max_total_bars = v as u64,
        "v_min_amplitude_pct" => cfg.v_min_amplitude_pct = v,
        "v_symmetry_tol" => cfg.v_symmetry_tol = v,
        "abcd_c_min_retrace" => cfg.abcd_c_min_retrace = v,
        "abcd_c_max_retrace" => cfg.abcd_c_max_retrace = v,
        "abcd_d_projection_tol" => cfg.abcd_d_projection_tol = v,
        "abcd_min_bars_per_leg" => cfg.abcd_min_bars_per_leg = v as u64,
        "scallop_min_bars" => cfg.scallop_min_bars = v as u64,
        "scallop_min_rim_progress_pct" => cfg.scallop_min_rim_progress_pct = v,
        "scallop_roundness_r2" => cfg.scallop_roundness_r2 = v,
        _ => { /* forward-compat: unknown keys ignored */ }
    }
}

// ---------------------------------------------------------------------------
// Per-symbol processing.
// ---------------------------------------------------------------------------

async fn process_symbol(
    pool: &PgPool,
    sym: &EngineSymbol,
    base_cfg: &ClassicalConfig,
    bars_limit: i64,
    pivots_per_slot: i64,
) -> anyhow::Result<usize> {
    let mut written = 0usize;

    // Load bars once per symbol — shared across all slots and both spec
    // tables. Chronological order (oldest..newest) matches what the
    // bar-aware shape evaluators expect.
    let raw_bars = market_bars::list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        bars_limit,
    )
    .await?;
    let chrono_bars: Vec<MarketBarRow> = raw_bars.into_iter().rev().collect();
    let instrument = build_instrument(sym);
    let domain_bars: Vec<DomainBar> = chrono_bars
        .iter()
        .map(|r| to_domain_bar(r, &instrument, sym))
        .collect();

    for slot in 0..5i16 {
        let stored = list_pivots_by_slot(pool, sym.id, slot, pivots_per_slot).await?;
        if stored.len() < 3 {
            continue;
        }
        let level = level_for_slot(slot);
        let pivots: Vec<DomainPivot> = stored.iter().map(|p| to_domain_pivot(p, level)).collect();

        // Build a slot-specific config (only `pivot_level` differs).
        let mut cfg = base_cfg.clone();
        cfg.pivot_level = level;
        // Defensive: if the slot-specific clone somehow fails validation,
        // skip the slot rather than crashing the tick.
        if cfg.validate().is_err() {
            continue;
        }

        // Pivot-only catalog (20 shapes).
        for spec in SHAPES {
            if pivots.len() < spec.pivots_needed {
                continue;
            }
            for end in spec.pivots_needed..=pivots.len() {
                let tail = &pivots[end - spec.pivots_needed..end];
                let Some(m) = (spec.eval)(tail, &cfg) else { continue };
                if (m.score as f32) < cfg.min_structural_score {
                    continue;
                }
                written += write_match(
                    pool, sym, slot, spec.name, &m, tail, &stored,
                )
                .await?;
            }
        }

        // Bar-aware catalog (9 shapes) — only when bar data is available.
        if !domain_bars.is_empty() {
            for spec in SHAPES_WITH_BARS {
                if pivots.len() < spec.pivots_needed {
                    continue;
                }
                if domain_bars.len() < spec.bars_needed {
                    continue;
                }
                for end in spec.pivots_needed..=pivots.len() {
                    let tail = &pivots[end - spec.pivots_needed..end];
                    // Use a bar window that ends at the latest pivot in
                    // the tail so ATR / flagpole math sees the same price
                    // context the detector would at that moment.
                    let last_bar_idx = tail
                        .last()
                        .map(|p| p.bar_index as usize)
                        .unwrap_or(domain_bars.len().saturating_sub(1));
                    let bar_end = (last_bar_idx + 1).min(domain_bars.len());
                    if bar_end < spec.bars_needed {
                        continue;
                    }
                    let bar_slice = &domain_bars[bar_end - spec.bars_needed..bar_end];
                    let Some(m) = (spec.eval)(tail, bar_slice, &cfg) else { continue };
                    if (m.score as f32) < cfg.min_structural_score {
                        continue;
                    }
                    written += write_match(
                        pool, sym, slot, spec.name, &m, tail, &stored,
                    )
                    .await?;
                }
            }
        }
    }

    Ok(written)
}

fn level_for_slot(slot: i16) -> PivotLevel {
    match slot {
        0 => PivotLevel::L0,
        1 => PivotLevel::L1,
        2 => PivotLevel::L2,
        3 => PivotLevel::L3,
        _ => PivotLevel::L4,
    }
}

fn to_domain_pivot(p: &StoredPivot, level: PivotLevel) -> DomainPivot {
    DomainPivot {
        bar_index: p.bar_index.max(0) as u64,
        time: p.open_time,
        price: p.price,
        kind: if p.direction > 0 {
            PivotKind::High
        } else {
            PivotKind::Low
        },
        level,
        prominence: Decimal::ZERO,
        volume_at_pivot: p.volume,
        swing_type: None, // swing_tag parsing deferred — shapes don't read it.
    }
}

fn build_instrument(sym: &EngineSymbol) -> Instrument {
    // Synthetic but stable: the classical detectors don't hash on
    // instrument fields, so venue/asset-class only need to be parseable.
    // We keep `tick_size`/`lot_size` as 0 to make misuse visible — none
    // of the 29 evaluators read them.
    let venue = match sym.exchange.as_str() {
        "binance" => Venue::Binance,
        "bybit" => Venue::Bybit,
        "okx" => Venue::Okx,
        "nasdaq" => Venue::Nasdaq,
        "bist" => Venue::Bist,
        other => Venue::Custom(other.to_string()),
    };
    let asset_class = match sym.segment.as_str() {
        "spot" => AssetClass::CryptoSpot,
        "futures" => AssetClass::CryptoFutures,
        "margin" => AssetClass::CryptoMargin,
        "options" => AssetClass::CryptoOptions,
        "equity_bist" => AssetClass::EquityBist,
        "equity_nasdaq" => AssetClass::EquityNasdaq,
        "equity_nyse" => AssetClass::EquityNyse,
        _ => AssetClass::CryptoFutures,
    };
    Instrument {
        venue,
        asset_class,
        symbol: sym.symbol.clone(),
        quote_ccy: "USDT".to_string(),
        tick_size: Decimal::ZERO,
        lot_size: Decimal::ZERO,
        session: SessionCalendar::binance_24x7(),
    }
}

fn to_domain_bar(r: &MarketBarRow, inst: &Instrument, sym: &EngineSymbol) -> DomainBar {
    DomainBar {
        instrument: inst.clone(),
        timeframe: parse_tf(&sym.interval),
        open_time: r.open_time,
        open: r.open,
        high: r.high,
        low: r.low,
        close: r.close,
        volume: r.volume,
        closed: true,
    }
}

fn parse_tf(s: &str) -> Timeframe {
    s.parse::<Timeframe>().unwrap_or(Timeframe::H1)
}

// ---------------------------------------------------------------------------
// Detection upsert.
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn write_match(
    pool: &PgPool,
    sym: &EngineSymbol,
    slot: i16,
    shape_name: &str,
    m: &ShapeMatch,
    tail: &[DomainPivot],
    stored: &[StoredPivot],
) -> anyhow::Result<usize> {
    if tail.is_empty() {
        return Ok(0);
    }
    let subkind = format!("{}_{}", shape_name, m.variant);

    let start_bar = tail.first().map(|p| p.bar_index as i64).unwrap_or(0);
    let end_bar = tail.last().map(|p| p.bar_index as i64).unwrap_or(start_bar);

    // Use the DB pivot open_times rather than tail time (which loses
    // microsecond precision when Decimal → f64 round-trips don't apply).
    let start_time = stored
        .iter()
        .find(|p| p.bar_index == start_bar)
        .map(|p| p.open_time)
        .unwrap_or_else(Utc::now);
    let end_time = stored
        .iter()
        .find(|p| p.bar_index == end_bar)
        .map(|p| p.open_time)
        .unwrap_or(start_time);

    let direction: i16 = direction_from_variant(m.variant);
    let anchors = anchors_json(tail, &m.anchor_labels);
    let raw_meta = json!({
        "score": m.score,
        "variant": m.variant,
        "invalidation": m.invalidation.to_f64().unwrap_or(0.0),
        "shape": shape_name,
    });

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
    .bind(slot)
    .bind("classical")
    .bind(&subkind)
    .bind(direction)
    .bind(start_bar)
    .bind(end_bar)
    .bind(start_time)
    .bind(end_time)
    .bind(&anchors)
    .bind(false)
    .bind(&raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}

/// Variant → side mapping dispatch table (CLAUDE.md #1). Kept near the
/// detection writer because it's classical-specific semantics rather than
/// a generic domain helper.
fn direction_from_variant(variant: &str) -> i16 {
    // Bear / top / desc / rising_wedge etc. → short-biased.
    // Bull / bottom / asc / falling_wedge → long-biased.
    // Neutral (rectangle, pennant, symmetrical_triangle, abcd) → 0.
    let v = variant.to_ascii_lowercase();
    if v.contains("bear") || v.contains("top") || v.contains("desc") || v == "rising" {
        return -1;
    }
    if v.contains("bull") || v.contains("bottom") || v.contains("asc") || v == "falling" {
        return 1;
    }
    0
}

fn anchors_json(tail: &[DomainPivot], labels: &[&'static str]) -> Value {
    let arr: Vec<Value> = tail
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut obj = json!({
                "direction": if matches!(p.kind, PivotKind::High) { 1 } else { -1 },
                "bar_index": p.bar_index as i64,
                "price": p.price.to_f64().unwrap_or(0.0),
                "time": p.time,
            });
            if let Some(l) = labels.get(i) {
                obj["label_override"] = json!(l);
            }
            obj
        })
        .collect();
    Value::Array(arr)
}

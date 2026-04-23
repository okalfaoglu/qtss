// Workaround: rustc 1.95 `annotate_snippets` renderer ICE on dead-code
// lint for engine writers. Silenced module-wide; no actual dead code.
#![allow(dead_code)]

//! Gap writer — fifth engine-dispatch member. Runs `qtss_gap::GapDetector`
//! over a sliding bar window and upserts every qualifying match into
//! `detections` with `pattern_family = 'gap'` and one of the five
//! `subkind`s the library already produces (island_reversal, exhaustion_gap,
//! runaway_gap, breakaway_gap, common_gap — each with `_bull` / `_bear`).
//!
//! Unlike Classical/Range, the GapDetector consumes the v2 `Bar`
//! domain type directly and returns the best `GapSpec` match for the
//! *current* bar window (the last bar is the gap candidate). So the
//! writer scans the bar slice by truncation, one tick per candidate
//! gap bar. This is O(n × |GAP_SPECS|) which is fine for the default
//! 2000-bar window.
//!
//! Config (all in `system_config`, CLAUDE.md #2):
//!   * `gap.enabled`          → `{ "enabled": true }`
//!   * `gap.bars_per_tick`    → `{ "bars": 2000 }`
//!   * `gap.min_score`        → `{ "score": 0.50 }`
//!   * `gap.thresholds.<key>` → `{ "value": <number> }` (GapConfig fields)

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use qtss_domain::v2::bar::Bar as DomainBar;
use qtss_domain::v2::detection::PatternKind;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::regime::{RegimeKind, RegimeSnapshot, TrendStrength};
use qtss_domain::v2::timeframe::Timeframe;
use qtss_gap::{GapConfig, GapDetector};
use qtss_storage::market_bars::{self, MarketBarRow};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::{self, EngineSymbol};
use crate::writer::{RunStats, WriterTask};

pub struct GapWriter;

#[async_trait]
impl WriterTask for GapWriter {
    fn family_name(&self) -> &'static str {
        "gap"
    }

    async fn run_once(&self, pool: &PgPool) -> anyhow::Result<RunStats> {
        let mut stats = RunStats::default();
        let syms = symbols::list_enabled(pool).await?;
        let bars_limit =
            load_num_async(pool, "bars_per_tick", "bars", 2_000).await.clamp(200, 10_000);
        let min_score = load_min_score(pool).await;
        let cfg = load_config(pool, min_score).await;
        let detector = match GapDetector::new(cfg) {
            Ok(d) => d,
            Err(e) => {
                warn!(%e, "gap: invalid config, falling back to defaults");
                GapDetector::new(GapConfig::default())?
            }
        };

        for sym in &syms {
            match process_symbol(pool, sym, bars_limit, &detector).await {
                Ok(n) => {
                    stats.series_processed += 1;
                    stats.rows_upserted += n;
                }
                Err(e) => warn!(
                    exchange = %sym.exchange,
                    symbol = %sym.symbol,
                    tf = %sym.interval,
                    %e,
                    "gap: series failed"
                ),
            }
        }
        Ok(stats)
    }
}

async fn process_symbol(
    pool: &PgPool,
    sym: &EngineSymbol,
    bars_limit: i64,
    detector: &GapDetector,
) -> anyhow::Result<usize> {
    let raw = market_bars::list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        bars_limit,
    )
    .await?;
    let min_window = detector.config().volume_sma_bars + 2;
    if raw.len() < min_window {
        return Ok(0);
    }
    let chrono_rows: Vec<MarketBarRow> = raw.into_iter().rev().collect();
    let instrument = build_instrument(sym);
    let tf = parse_tf(&sym.interval);
    let regime = neutral_regime();
    let bars: Vec<DomainBar> = chrono_rows
        .iter()
        .map(|r| to_domain_bar(r, &instrument, tf))
        .collect();

    // Sliding scan — the detector treats the *last* bar as the gap
    // candidate, so truncating at `end` probes bar[end-1]. Start from
    // `min_window` so the volume-SMA baseline has enough history.
    let mut written = 0usize;
    for end in min_window..=bars.len() {
        let window = &bars[..end];
        let Some(det) = detector.detect(window, &instrument, tf, &regime) else {
            continue;
        };
        written += write_detection(pool, sym, &chrono_rows, &det).await?;
    }
    Ok(written)
}

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

async fn load_num_async(pool: &PgPool, key: &str, field: &str, default: i64) -> i64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'gap' AND config_key = $1",
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

async fn load_min_score(pool: &PgPool) -> f32 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'gap' AND config_key = 'min_score'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 0.50; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.50)
        .clamp(0.0, 1.0) as f32
}

async fn load_config(pool: &PgPool, min_score: f32) -> GapConfig {
    let mut cfg = GapConfig::default();
    cfg.min_structural_score = min_score;
    let rows = sqlx::query(
        r#"SELECT config_key, value
             FROM system_config
            WHERE module = 'gap'
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
    if cfg.validate().is_err() {
        cfg = GapConfig::default();
        cfg.min_structural_score = min_score;
    }
    cfg
}

fn apply_threshold(cfg: &mut GapConfig, key: &str, v: f64) {
    let k = key.trim_start_matches("thresholds.");
    match k {
        "min_gap_pct" => cfg.min_gap_pct = v,
        "volume_sma_bars" => cfg.volume_sma_bars = v as usize,
        "vol_mult_breakaway" => cfg.vol_mult_breakaway = v,
        "vol_mult_runaway" => cfg.vol_mult_runaway = v,
        "vol_mult_exhaustion" => cfg.vol_mult_exhaustion = v,
        "range_flat_pct" => cfg.range_flat_pct = v,
        "consolidation_lookback" => cfg.consolidation_lookback = v as usize,
        "runaway_trend_bars" => cfg.runaway_trend_bars = v as usize,
        "runaway_trend_min_pct" => cfg.runaway_trend_min_pct = v,
        "exhaustion_reversal_bars" => cfg.exhaustion_reversal_bars = v as usize,
        "island_max_bars" => cfg.island_max_bars = v as usize,
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Domain adapters
// ---------------------------------------------------------------------------

fn build_instrument(sym: &EngineSymbol) -> Instrument {
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

fn to_domain_bar(r: &MarketBarRow, inst: &Instrument, tf: Timeframe) -> DomainBar {
    DomainBar {
        instrument: inst.clone(),
        timeframe: tf,
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

/// Synthetic regime — the gap detector embeds a `RegimeSnapshot` in the
/// Detection for downstream analysis but does not branch on its fields,
/// so a neutral placeholder is sufficient.
fn neutral_regime() -> RegimeSnapshot {
    RegimeSnapshot {
        at: Utc::now(),
        kind: RegimeKind::Uncertain,
        trend_strength: TrendStrength::None,
        adx: Decimal::ZERO,
        bb_width: Decimal::ZERO,
        atr_pct: Decimal::ZERO,
        choppiness: Decimal::ZERO,
        confidence: 0.0,
    }
}

// ---------------------------------------------------------------------------
// Detection upsert
// ---------------------------------------------------------------------------

async fn write_detection(
    pool: &PgPool,
    sym: &EngineSymbol,
    chrono_rows: &[MarketBarRow],
    det: &qtss_domain::v2::detection::Detection,
) -> anyhow::Result<usize> {
    let subkind = subkind_from_kind(&det.kind);
    let (start_bar, end_bar) = anchor_bar_range(det);
    let start_time = chrono_rows
        .get(start_bar as usize)
        .map(|r| r.open_time)
        .unwrap_or_else(Utc::now);
    let end_time = chrono_rows
        .get(end_bar as usize)
        .map(|r| r.open_time)
        .unwrap_or(start_time);
    let direction = direction_from_subkind(&subkind);
    let anchors = anchors_to_json(det, chrono_rows);
    let raw_meta = json!({
        "score":              det.structural_score,
        "invalidation_price": det.invalidation_price.to_f64().unwrap_or(0.0),
        "state":              format!("{:?}", det.state).to_lowercase(),
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
    .bind(0i16) // gaps are bar-indexed, slot is informational
    .bind("gap")
    .bind(&subkind)
    .bind(direction)
    .bind(start_bar)
    .bind(end_bar)
    .bind(start_time)
    .bind(end_time)
    .bind(&anchors)
    .bind(matches!(
        det.state,
        qtss_domain::v2::detection::PatternState::Invalidated
    ))
    .bind(&raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}

fn subkind_from_kind(kind: &PatternKind) -> String {
    match kind {
        PatternKind::Gap(s) => s.clone(),
        _ => "unknown".to_string(),
    }
}

fn direction_from_subkind(subkind: &str) -> i16 {
    if subkind.ends_with("_bull") {
        1
    } else if subkind.ends_with("_bear") {
        -1
    } else {
        0
    }
}

fn anchor_bar_range(det: &qtss_domain::v2::detection::Detection) -> (i64, i64) {
    let idxs: Vec<i64> = det.anchors.iter().map(|a| a.bar_index as i64).collect();
    let start = idxs.iter().copied().min().unwrap_or(0);
    let end = idxs.iter().copied().max().unwrap_or(start);
    (start, end)
}

fn anchors_to_json(
    det: &qtss_domain::v2::detection::Detection,
    chrono_rows: &[MarketBarRow],
) -> Value {
    let arr: Vec<Value> = det
        .anchors
        .iter()
        .map(|a| {
            let t = chrono_rows
                .get(a.bar_index as usize)
                .map(|r| r.open_time)
                .unwrap_or_else(Utc::now);
            let mut obj = json!({
                "bar_index": a.bar_index as i64,
                "price":     a.price.to_f64().unwrap_or(0.0),
                "time":      t,
            });
            if let Some(l) = &a.label {
                obj["label_override"] = json!(l);
            }
            obj
        })
        .collect();
    Value::Array(arr)
}

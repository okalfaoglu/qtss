// Workaround: rustc 1.95 `annotate_snippets` renderer ICE on dead-code
// lint for engine writers. Silenced module-wide; no actual dead code.
#![allow(dead_code)]

//! Candlestick-pattern writer — sixth engine-dispatch member. Runs the
//! 43-pattern [`qtss_candles`] catalog (doji / hammer / engulfing /
//! morning-star / three-soldiers / …) over a sliding bar window and
//! upserts every qualifying match into `detections` with
//! `pattern_family = 'candle'`.
//!
//! Candle patterns are high-frequency, low-reliability signals in
//! isolation; the writer publishes them so the Confluence engine
//! (Faz 12) can combine them with Elliott / Harmonic / Classical for a
//! real-entry filter. Anchors are `{open_of_first, close_of_last}` so
//! the chart can render either a per-bar marker or a short "gesture
//! line" without re-reading the bar table.
//!
//! Config (`system_config`, module = 'candle'):
//!   * `enabled`        → `{ "enabled": true }`
//!   * `bars_per_tick`  → `{ "bars": 2000 }`
//!   * `min_score`      → `{ "score": 0.60 }`
//!   * `thresholds.<k>` → `{ "value": <number> }` (CandleConfig fields)

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use qtss_candles::{CandleConfig, CandleDetector};
use qtss_domain::v2::bar::Bar as DomainBar;
use qtss_domain::v2::detection::PatternKind;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::regime::{RegimeKind, RegimeSnapshot, TrendStrength};
use qtss_domain::v2::timeframe::Timeframe;
use qtss_storage::market_bars::{self, MarketBarRow};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::{self, EngineSymbol};
use crate::writer::{RunStats, WriterTask};

pub struct CandlesWriter;

#[async_trait]
impl WriterTask for CandlesWriter {
    fn family_name(&self) -> &'static str {
        "candle"
    }

    async fn run_once(&self, pool: &PgPool) -> anyhow::Result<RunStats> {
        let mut stats = RunStats::default();
        let syms = symbols::list_enabled(pool).await?;
        let bars_limit = load_num(pool, "bars_per_tick", "bars", 2_000).await.clamp(200, 10_000);
        let min_score = load_min_score(pool).await;
        let cfg = load_config(pool, min_score).await;
        let detector = match CandleDetector::new(cfg) {
            Ok(d) => d,
            Err(e) => {
                warn!(%e, "candle: invalid config, using defaults");
                CandleDetector::new(CandleConfig::default())?
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
                    "candle: series failed"
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
    detector: &CandleDetector,
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
    let min_window = detector.config().trend_context_bars + 3;
    if raw.len() < min_window {
        return Ok(0);
    }
    let chrono_rows: Vec<MarketBarRow> = raw.into_iter().rev().collect();
    let instrument = build_instrument(sym);
    let tf = parse_tf(&sym.interval);
    // Timeframe gate mirrors `CandleDetector::detect` — detector will
    // bounce sub-threshold TFs anyway but we can short-circuit here to
    // save the per-bar sliding scan overhead on noisy 1m series.
    if tf.seconds() < detector.config().min_timeframe_seconds {
        return Ok(0);
    }
    let regime = neutral_regime();
    let bars: Vec<DomainBar> = chrono_rows
        .iter()
        .map(|r| to_domain_bar(r, &instrument, tf))
        .collect();

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

// ── Config loading ─────────────────────────────────────────────────────

async fn load_num(pool: &PgPool, key: &str, field: &str, default: i64) -> i64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'candle' AND config_key = $1",
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
        "SELECT value FROM system_config WHERE module = 'candle' AND config_key = 'min_score'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 0.60; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.60)
        .clamp(0.0, 1.0) as f32
}

async fn load_config(pool: &PgPool, min_score: f32) -> CandleConfig {
    let mut cfg = CandleConfig::default();
    cfg.min_structural_score = min_score;
    let rows = sqlx::query(
        r#"SELECT config_key, value
             FROM system_config
            WHERE module = 'candle'
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
        cfg = CandleConfig::default();
        cfg.min_structural_score = min_score;
    }
    cfg
}

fn apply_threshold(cfg: &mut CandleConfig, key: &str, v: f64) {
    let k = key.trim_start_matches("thresholds.");
    match k {
        "doji_body_ratio_max" => cfg.doji_body_ratio_max = v,
        "marubozu_shadow_ratio_max" => cfg.marubozu_shadow_ratio_max = v,
        "hammer_lower_shadow_ratio_min" => cfg.hammer_lower_shadow_ratio_min = v,
        "tweezer_price_tol" => cfg.tweezer_price_tol = v,
        "trend_context_bars" => cfg.trend_context_bars = v as usize,
        "trend_context_min_pct" => cfg.trend_context_min_pct = v,
        "min_timeframe_seconds" => cfg.min_timeframe_seconds = v as i64,
        _ => {}
    }
}

// ── Domain adapters ────────────────────────────────────────────────────

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

// ── Detection upsert ───────────────────────────────────────────────────

async fn write_detection(
    pool: &PgPool,
    sym: &EngineSymbol,
    chrono_rows: &[MarketBarRow],
    det: &qtss_domain::v2::detection::Detection,
) -> anyhow::Result<usize> {
    let subkind = match &det.kind {
        PatternKind::Candle(s) => s.clone(),
        _ => "unknown".to_string(),
    };
    let idxs: Vec<i64> = det.anchors.iter().map(|a| a.bar_index as i64).collect();
    let start_bar = idxs.iter().copied().min().unwrap_or(0);
    let end_bar = idxs.iter().copied().max().unwrap_or(start_bar);
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
    .bind(0i16)
    .bind("candle")
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

fn direction_from_subkind(s: &str) -> i16 {
    // Candle lib uses consistent _bull / _bear suffix for directional
    // patterns; neutral shapes (doji, spinning_top) have no suffix.
    let lower = s.to_ascii_lowercase();
    if lower.ends_with("_bull")
        || lower.starts_with("hammer")
        || lower.starts_with("piercing_line")
        || lower.starts_with("morning_star")
        || lower.starts_with("three_white_soldiers")
        || lower.starts_with("three_inside_up")
        || lower.starts_with("three_outside_up")
        || lower.starts_with("tweezer_bottom")
        || lower.starts_with("dragonfly_doji")
        || lower.starts_with("inverted_hammer")
    {
        return 1;
    }
    if lower.ends_with("_bear")
        || lower.starts_with("hanging_man")
        || lower.starts_with("shooting_star")
        || lower.starts_with("dark_cloud_cover")
        || lower.starts_with("evening_star")
        || lower.starts_with("three_black_crows")
        || lower.starts_with("three_inside_down")
        || lower.starts_with("three_outside_down")
        || lower.starts_with("tweezer_top")
        || lower.starts_with("gravestone_doji")
    {
        return -1;
    }
    0
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

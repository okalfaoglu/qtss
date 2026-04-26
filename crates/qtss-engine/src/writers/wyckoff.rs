// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! Wyckoff writer — 12th engine-dispatch member (Faz 14).
//!
//! Reads bars, runs `qtss_wyckoff::detect_events`, upserts each event
//! into `detections` with `pattern_family = 'wyckoff'` and a
//! `<event>_<variant>` subkind. Phase tracker output (A/B/C/D/E)
//! written to `raw_meta.phase` so the chart overlay can colour the
//! range.

use async_trait::async_trait;
use chrono::Utc;
use qtss_domain::v2::bar::Bar as DomainBar;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::timeframe::Timeframe;
use qtss_storage::market_bars::{self, MarketBarRow};
use qtss_wyckoff::{
    detect_cycles, detect_events, detect_ranges, WyckoffBias, WyckoffConfig,
    WyckoffCycle, WyckoffCyclePhase, WyckoffEvent, WyckoffPhaseTracker, WyckoffRange,
};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::{self, EngineSymbol};
use crate::writer::{RunStats, WriterTask};

pub struct WyckoffWriter;

#[async_trait]
impl WriterTask for WyckoffWriter {
    fn family_name(&self) -> &'static str {
        "wyckoff"
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
                    "wyckoff: series failed"
                ),
            }
        }
        Ok(stats)
    }
}

async fn process_symbol(
    pool: &PgPool,
    sym: &EngineSymbol,
    cfg: &WyckoffConfig,
) -> anyhow::Result<usize> {
    let raw = market_bars::list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        2000,
    )
    .await?;
    if raw.len() < 60 {
        return Ok(0);
    }
    let chrono: Vec<MarketBarRow> = raw.into_iter().rev().collect();
    let instrument = build_instrument(sym);
    let tf = parse_tf(&sym.interval);
    let bars: Vec<DomainBar> = chrono
        .iter()
        .map(|r| to_domain_bar(r, &instrument, tf))
        .collect();

    let events = detect_events(&bars, cfg);
    // Feed the phase tracker in chronological order (already sorted).
    let mut tracker = WyckoffPhaseTracker::new();
    let mut sorted = events.clone();
    sorted.sort_by_key(|e| e.bar_index);
    for e in &sorted {
        tracker.feed(e);
    }
    let phase_str = format!("{:?}", tracker.phase()).to_lowercase();
    let bias_str = format!("{:?}", tracker.bias()).to_lowercase();

    let mut written = 0usize;
    for ev in &events {
        written += write_event(pool, sym, &chrono, ev, &phase_str, &bias_str).await?;
    }

    // FAZ 25.4.B — schematic range boxes. The user asked for the
    // Accumulation / Distribution rectangle that frames events,
    // matching the TradingView Wyckoff annotation convention. We
    // group sorted events through detect_ranges() and persist each
    // as its own detection row with pattern_family='wyckoff' +
    // subkind='range_accumulation' / 'range_distribution'. The
    // chart renders these as a primitive rectangle behind the event
    // markers.
    //
    // Sweep stale ranges + cycles before writing fresh ones so old
    // boxes/segments don't pile up at the same slot.
    let _ = sqlx::query(
        r#"DELETE FROM detections
            WHERE exchange = $1 AND segment = $2
              AND symbol = $3 AND timeframe = $4
              AND pattern_family = 'wyckoff'
              AND mode = 'live'
              AND (subkind LIKE 'range_%' OR subkind LIKE 'cycle_%')"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .execute(pool)
    .await;

    let ranges = detect_ranges(&sorted);
    for r in &ranges {
        written += write_range(pool, sym, &chrono, r).await?;
    }

    // FAZ 25.4.D — four-phase macro cycle (Accumulation → Markup →
    // Distribution → Markdown). User asked: "Bu döngü bizde var mı?"
    // The schematic ranges (Accum / Dist) are persisted above; the
    // trend legs (Markup / Markdown) connecting them were missing.
    // detect_cycles tiles the entire tape into a contiguous segment
    // sequence — every bar belongs to exactly one of the 4 phases.
    let tape_end_bar = chrono.len().saturating_sub(1);
    let tape_end_price = chrono
        .last()
        .and_then(|r| r.close.to_string().parse::<f64>().ok())
        .unwrap_or(0.0);
    let cycles = detect_cycles(&sorted, tape_end_bar, tape_end_price);
    for c in &cycles {
        written += write_cycle(pool, sym, &chrono, c).await?;
    }
    Ok(written)
}

async fn write_cycle(
    pool: &PgPool,
    sym: &EngineSymbol,
    chrono: &[MarketBarRow],
    c: &WyckoffCycle,
) -> anyhow::Result<usize> {
    let phase_str = match c.phase {
        WyckoffCyclePhase::Accumulation => "accumulation",
        WyckoffCyclePhase::Markup => "markup",
        WyckoffCyclePhase::Distribution => "distribution",
        WyckoffCyclePhase::Markdown => "markdown",
    };
    let subkind = format!("cycle_{phase_str}");
    let direction: i16 = match c.phase {
        WyckoffCyclePhase::Accumulation | WyckoffCyclePhase::Markup => 1,
        WyckoffCyclePhase::Distribution | WyckoffCyclePhase::Markdown => -1,
    };
    let start_time = chrono
        .get(c.start_bar)
        .map(|b| b.open_time)
        .unwrap_or_else(Utc::now);
    let end_time = chrono
        .get(c.end_bar)
        .map(|b| b.open_time)
        .unwrap_or_else(Utc::now);
    // anchors = corners of the cycle band the chart renderer needs:
    //   anchors[0] = start at start_price
    //   anchors[1] = end at end_price
    let anchors = json!([
        {
            "label_override": phase_str,
            "bar_index": c.start_bar as i64,
            "price": c.start_price,
            "time": start_time,
        },
        {
            "label_override": phase_str,
            "bar_index": c.end_bar as i64,
            "price": c.end_price,
            "time": end_time,
        }
    ]);
    let raw_meta = json!({
        "phase":       phase_str,
        "start_price": c.start_price,
        "end_price":   c.end_price,
        "completed":   c.completed,
        "kind":        "wyckoff_cycle",
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
               direction  = EXCLUDED.direction,
               anchors    = EXCLUDED.anchors,
               raw_meta   = EXCLUDED.raw_meta,
               updated_at = now()"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .bind(0i16)
    .bind("wyckoff")
    .bind(&subkind)
    .bind(direction)
    .bind(c.start_bar as i64)
    .bind(c.end_bar as i64)
    .bind(start_time)
    .bind(end_time)
    .bind(&anchors)
    .bind(c.completed)
    .bind(&raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}

async fn write_range(
    pool: &PgPool,
    sym: &EngineSymbol,
    chrono: &[MarketBarRow],
    r: &WyckoffRange,
) -> anyhow::Result<usize> {
    let bias_str = match r.bias {
        WyckoffBias::Accumulation => "accumulation",
        WyckoffBias::Distribution => "distribution",
        WyckoffBias::Neutral => return Ok(0),
    };
    let subkind = format!("range_{bias_str}");
    let direction: i16 = match r.bias {
        WyckoffBias::Accumulation => 1,
        WyckoffBias::Distribution => -1,
        _ => 0,
    };
    let start_time = chrono
        .get(r.start_bar)
        .map(|c| c.open_time)
        .unwrap_or_else(Utc::now);
    let end_time = chrono
        .get(r.end_bar)
        .map(|c| c.open_time)
        .unwrap_or_else(Utc::now);
    // anchors = corners of the rectangle so the chart renderer can
    // draw a filled box without an extra fetch:
    //   anchors[0] = start, low corner
    //   anchors[1] = end, high corner
    let anchors = json!([
        {
            "label_override": bias_str,
            "bar_index": r.start_bar as i64,
            "price": r.range_low,
            "time": start_time,
        },
        {
            "label_override": bias_str,
            "bar_index": r.end_bar as i64,
            "price": r.range_high,
            "time": end_time,
        }
    ]);
    let phase_str = format!("{:?}", r.phase).to_lowercase();
    let raw_meta = json!({
        "phase":          phase_str,
        "bias":           bias_str,
        "range_high":     r.range_high,
        "range_low":      r.range_low,
        "event_count":    r.event_indices.len(),
        "completed":      r.completed,
        "kind":           "wyckoff_range",
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
               direction  = EXCLUDED.direction,
               anchors    = EXCLUDED.anchors,
               raw_meta   = EXCLUDED.raw_meta,
               updated_at = now()"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .bind(0i16)
    .bind("wyckoff")
    .bind(&subkind)
    .bind(direction)
    .bind(r.start_bar as i64)
    .bind(r.end_bar as i64)
    .bind(start_time)
    .bind(end_time)
    .bind(&anchors)
    .bind(r.completed)
    .bind(&raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}

fn build_instrument(sym: &EngineSymbol) -> Instrument {
    let venue = match sym.exchange.as_str() {
        "binance" => Venue::Binance,
        "bybit" => Venue::Bybit,
        "okx" => Venue::Okx,
        other => Venue::Custom(other.to_string()),
    };
    let asset_class = match sym.segment.as_str() {
        "spot" => AssetClass::CryptoSpot,
        "futures" => AssetClass::CryptoFutures,
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

async fn write_event(
    pool: &PgPool,
    sym: &EngineSymbol,
    chrono: &[MarketBarRow],
    ev: &WyckoffEvent,
    phase: &str,
    bias: &str,
) -> anyhow::Result<usize> {
    let subkind = format!("{}_{}", ev.kind.as_str(), ev.variant);
    let direction: i16 = match ev.variant {
        "bull" => 1,
        "bear" => -1,
        _ => 0,
    };
    let bar_time = chrono
        .get(ev.bar_index)
        .map(|r| r.open_time)
        .unwrap_or_else(Utc::now);
    let anchors = json!([
        {
            "label_override": ev.kind.as_str().to_uppercase(),
            "bar_index": ev.bar_index as i64,
            "price": ev.reference_price,
            "time": bar_time,
        }
    ]);
    let raw_meta = json!({
        "score":           ev.score,
        "volume_ratio":    ev.volume_ratio,
        "range_ratio":     ev.range_ratio,
        "reference_price": ev.reference_price,
        "note":            ev.note,
        "event_kind":      ev.kind.as_str(),
        "phase":           phase,
        "bias":            bias,
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
               direction  = EXCLUDED.direction,
               anchors    = EXCLUDED.anchors,
               raw_meta   = EXCLUDED.raw_meta,
               updated_at = now()"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .bind(0i16)
    .bind("wyckoff")
    .bind(&subkind)
    .bind(direction)
    .bind(ev.bar_index as i64)
    .bind(ev.bar_index as i64)
    .bind(bar_time)
    .bind(bar_time)
    .bind(&anchors)
    .bind(false)
    .bind(&raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}

async fn load_config(pool: &PgPool) -> WyckoffConfig {
    let mut cfg = WyckoffConfig::default();
    let rows = sqlx::query(
        r#"SELECT config_key, value FROM system_config
            WHERE module = 'wyckoff' AND config_key LIKE 'thresholds.%'"#,
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
            "climax_volume_mult" => cfg.climax_volume_mult = v,
            "climax_range_atr_mult" => cfg.climax_range_atr_mult = v,
            "spring_wick_max_pct" => cfg.spring_wick_max_pct = v,
            "sos_amplifier" => cfg.sos_amplifier = v,
            _ => {}
        }
    }
    cfg
}

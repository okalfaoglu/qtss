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
use qtss_wyckoff::{detect_events, WyckoffConfig, WyckoffEvent, WyckoffPhaseTracker};
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
    for ev in events {
        written += write_event(pool, sym, &chrono, &ev, &phase_str, &bias_str).await?;
    }
    Ok(written)
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

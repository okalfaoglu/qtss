// Workaround: rustc 1.95 `annotate_snippets` renderer ICE on dead-code
// lint. Silenced module-wide.
#![allow(dead_code)]

//! Smart Money Concepts writer — ninth engine-dispatch member. Reads
//! pivots for the configured slot plus recent bars, runs every
//! `qtss_smc::SMC_SPECS` evaluator, and upserts each event into the
//! `detections` table with `pattern_family = 'smc'` and a subkind of
//! `{bos|choch|mss|liquidity_sweep|fvi}_{bull|bear}`.
//!
//! Events are persistence-light (one row per event; evaluators return
//! a bounded Vec). Downstream Confluence engine joins these with
//! Elliott / Harmonic / Classical rows to score strategy entries.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use qtss_domain::v2::bar::Bar as DomainBar;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::pivot::{
    Pivot as DomainPivot, PivotKind, PivotLevel, PivotTree,
};
use qtss_domain::v2::timeframe::Timeframe;
use qtss_smc::{SmcConfig, SmcDetector, SmcEvent, SmcEventKind};
use qtss_storage::market_bars::{self, MarketBarRow};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::{self, EngineSymbol};
use crate::writer::{RunStats, WriterTask};

pub struct SmcWriter;

#[async_trait]
impl WriterTask for SmcWriter {
    fn family_name(&self) -> &'static str {
        "smc"
    }

    async fn run_once(&self, pool: &PgPool) -> anyhow::Result<RunStats> {
        let mut stats = RunStats::default();
        let syms = symbols::list_enabled(pool).await?;
        let bars_limit =
            load_num(pool, "bars_per_tick", "bars", 2_000).await.clamp(200, 10_000);
        let pivots_per_slot =
            load_num(pool, "pivots_per_slot", "count", 500).await.clamp(50, 5_000);
        let cfg = load_config(pool).await;
        let detector = match SmcDetector::new(cfg) {
            Ok(d) => d,
            Err(e) => {
                warn!(%e, "smc: invalid config, using defaults");
                SmcDetector::new(SmcConfig::default())
                    .map_err(|e| anyhow::anyhow!("smc default failed: {e}"))?
            }
        };

        for sym in &syms {
            match process_symbol(pool, sym, bars_limit, pivots_per_slot, &detector).await {
                Ok(n) => {
                    stats.series_processed += 1;
                    stats.rows_upserted += n;
                }
                Err(e) => warn!(
                    exchange = %sym.exchange,
                    symbol = %sym.symbol,
                    tf = %sym.interval,
                    %e,
                    "smc: series failed"
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
    pivots_per_slot: i64,
    detector: &SmcDetector,
) -> anyhow::Result<usize> {
    let slot = detector.config().pivot_level.as_index() as i16;
    let stored = list_pivots_by_slot(pool, sym.id, slot, pivots_per_slot).await?;
    if stored.len() < 4 {
        return Ok(0);
    }

    let raw = market_bars::list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        bars_limit,
    )
    .await?;
    if raw.len() < 30 {
        return Ok(0);
    }
    let chrono: Vec<MarketBarRow> = raw.into_iter().rev().collect();
    let instrument = build_instrument(sym);
    let tf = parse_tf(&sym.interval);
    let bars: Vec<DomainBar> = chrono
        .iter()
        .map(|r| to_domain_bar(r, &instrument, tf))
        .collect();

    let level = detector.config().pivot_level;
    let pivots: Vec<DomainPivot> = stored.iter().map(|p| to_domain_pivot(p, level)).collect();
    let tree = PivotTree::new(
        if level == PivotLevel::L0 { pivots.clone() } else { vec![] },
        if level == PivotLevel::L1 { pivots.clone() } else { vec![] },
        if level == PivotLevel::L2 { pivots.clone() } else { vec![] },
        if level == PivotLevel::L3 { pivots.clone() } else { vec![] },
        if level == PivotLevel::L4 { pivots } else { vec![] },
    );

    let events = detector.detect(&tree, &bars);

    let mut written = 0usize;
    for ev in events {
        written += write_event(pool, sym, slot, &chrono, &bars, &ev).await?;
    }
    Ok(written)
}

#[derive(Debug, Clone)]
struct StoredPivot {
    bar_index: i64,
    open_time: DateTime<Utc>,
    direction: i16,
    price: Decimal,
    volume: Decimal,
}

async fn list_pivots_by_slot(
    pool: &PgPool,
    engine_symbol_id: sqlx::types::Uuid,
    slot: i16,
    limit: i64,
) -> anyhow::Result<Vec<StoredPivot>> {
    let rows = sqlx::query(
        r#"SELECT bar_index, open_time, direction, price, volume
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
        })
        .collect();
    out.reverse();
    Ok(out)
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
        swing_type: None,
    }
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

// ── Config loading ─────────────────────────────────────────────────────

async fn load_num(pool: &PgPool, key: &str, field: &str, default: i64) -> i64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'smc' AND config_key = $1",
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
        "SELECT value FROM system_config WHERE module = 'smc' AND config_key = $1",
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

async fn load_config(pool: &PgPool) -> SmcConfig {
    let mut cfg = SmcConfig::default();
    cfg.min_structural_score =
        load_f64(pool, "min_score", "score", cfg.min_structural_score as f64).await as f32;
    cfg.break_confirm_bars =
        load_num(pool, "thresholds.break_confirm_bars", "value", cfg.break_confirm_bars as i64)
            .await as usize;
    cfg.mss_close_cushion_pct = load_f64(
        pool,
        "thresholds.mss_close_cushion_pct",
        "value",
        cfg.mss_close_cushion_pct,
    )
    .await;
    cfg.sweep_wick_penetration_pct = load_f64(
        pool,
        "thresholds.sweep_wick_penetration_pct",
        "value",
        cfg.sweep_wick_penetration_pct,
    )
    .await;
    cfg.sweep_reject_frac =
        load_f64(pool, "thresholds.sweep_reject_frac", "value", cfg.sweep_reject_frac).await;
    cfg.sweep_reject_bars = load_num(
        pool,
        "thresholds.sweep_reject_bars",
        "value",
        cfg.sweep_reject_bars as i64,
    )
    .await as usize;
    cfg.fvi_min_gap_atr_frac = load_f64(
        pool,
        "thresholds.fvi_min_gap_atr_frac",
        "value",
        cfg.fvi_min_gap_atr_frac,
    )
    .await;
    cfg.fvi_volume_spike_mult = load_f64(
        pool,
        "thresholds.fvi_volume_spike_mult",
        "value",
        cfg.fvi_volume_spike_mult,
    )
    .await;
    cfg.scan_lookback =
        load_num(pool, "thresholds.scan_lookback", "value", cfg.scan_lookback as i64).await
            as usize;
    if cfg.validate().is_err() {
        cfg = SmcConfig::default();
    }
    cfg
}

// ── Upsert ─────────────────────────────────────────────────────────────

async fn write_event(
    pool: &PgPool,
    sym: &EngineSymbol,
    slot: i16,
    chrono_rows: &[MarketBarRow],
    bars: &[DomainBar],
    ev: &SmcEvent,
) -> anyhow::Result<usize> {
    let subkind = format!("{}_{}", ev.kind.as_str(), ev.variant);
    let direction: i16 = match ev.variant {
        "bull" => 1,
        "bear" => -1,
        _ => 0,
    };
    let bar_time = bars
        .get(ev.bar_index)
        .map(|b| b.open_time)
        .unwrap_or_else(Utc::now);
    let start_bar = ev.bar_index as i64;
    let end_bar = start_bar;
    let anchors = json!([
        {
            "label_override": label_for(ev.kind),
            "bar_index": start_bar,
            "price": ev.reference_price.to_f64().unwrap_or(0.0),
            "time": bar_time,
        }
    ]);
    let raw_meta = json!({
        "score":              ev.score,
        "reference_price":    ev.reference_price.to_f64().unwrap_or(0.0),
        "invalidation_price": ev.invalidation_price.to_f64().unwrap_or(0.0),
        "kind":               ev.kind.as_str(),
    });
    let _ = chrono_rows; // time already embedded in bars
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
    .bind(slot)
    .bind("smc")
    .bind(&subkind)
    .bind(direction)
    .bind(start_bar)
    .bind(end_bar)
    .bind(bar_time)
    .bind(bar_time)
    .bind(&anchors)
    .bind(false)
    .bind(&raw_meta)
    .execute(pool)
    .await?;
    Ok(1)
}

fn label_for(kind: SmcEventKind) -> &'static str {
    match kind {
        SmcEventKind::Bos => "BOS",
        SmcEventKind::Choch => "CHoCH",
        SmcEventKind::Mss => "MSS",
        SmcEventKind::LiquiditySweep => "Sweep",
        SmcEventKind::Fvi => "FVI",
    }
}

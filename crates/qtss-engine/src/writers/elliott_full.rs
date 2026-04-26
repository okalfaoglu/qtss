// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! `elliott_full` — engine writer that surfaces every Elliott formation
//! detector that lived dormant in `qtss-elliott` until now.
//!
//! Until this writer landed, only LuxAlgo's Pine port (motive impulse
//! 1-2-3-4-5 + ABC zigzag + contracting/expanding triangle) and the
//! `elliott_early` sibling (nascent / forming impulse) were persisted
//! into `detections`. The richer `ElliottDetectorSet` — leading +
//! ending diagonals, regular / expanded / running flats, W1 / W3 / W5
//! extended impulses, truncated fifth, W-X-Y combinations — was
//! aggregated and configured (migration 0025) but never invoked
//! anywhere outside of unit tests. User flagged the gap directly:
//! "itki ve abc dışında diğer elliott dalgalarını kod tarama yapmıyor
//! mu?" — yes, the code existed, it just had no writer.
//!
//! What this writer does NOT touch:
//! * `pattern_family = 'motive' | 'abc' | 'triangle'` — owned by
//!   [`crate::writers::elliott::ElliottWriter`] (LuxAlgo Pine port).
//! * `pattern_family = 'elliott_early'` — owned by
//!   [`crate::writers::elliott_early`] (4/5-pivot pre-window scans).
//!
//! Output: `pattern_family = 'elliott_full'`, `subkind` carries the
//! detector's own label (e.g. `flat_expanded_bear`,
//! `leading_diagonal_bull`, `combination_wxy_zigzag_zigzag_bull`).
//! GUI dispatches on the subkind prefix to pick a renderer.
//!
//! Toggles default-on per [`ElliottFormationToggles::defaults`] but the
//! impulse / nascent_impulse / forming_impulse / zigzag / triangle /
//! luxalgo flavours are forced OFF here so we never produce duplicate
//! rows for patterns the existing writers already cover.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use qtss_domain::v2::bar::Bar as DomainBar;
use qtss_domain::v2::detection::{Detection, PatternKind, PatternState};
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::pivot::{Pivot as DomainPivot, PivotKind, PivotLevel, PivotTree};
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;
use qtss_elliott::{ElliottConfig, ElliottDetectorSet, ElliottFormationToggles};
use qtss_storage::market_bars::{self, MarketBarRow};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::{self, EngineSymbol};
use crate::writer::{RunStats, WriterTask};

pub struct ElliottFullWriter;

#[async_trait]
impl WriterTask for ElliottFullWriter {
    fn family_name(&self) -> &'static str {
        "elliott_full"
    }

    async fn run_once(&self, pool: &PgPool) -> anyhow::Result<RunStats> {
        let mut stats = RunStats::default();
        if !load_enabled(pool).await {
            return Ok(stats);
        }
        let syms = symbols::list_enabled(pool).await?;
        let bars_limit = load_num(pool, "bars_per_tick", 2_000).clamp_default(2_000, 200, 10_000);
        let pivots_per_slot = load_num(pool, "pivots_per_slot", 500).clamp_default(500, 50, 5_000);
        let bars_limit = bars_limit.fetch(pool).await;
        let pivots_per_slot = pivots_per_slot.fetch(pool).await;

        let toggles = load_toggles(pool).await;
        // FAZ 25.4.E — multi-level detection. The detector previously
        // used a single configured pivot_level (default L1), so higher
        // Z slots (Z3-Z5 = L2-L4) never received flat / diagonal /
        // triangle / combination pattern detection. User flagged this
        // when Z5 ABC corrective at BTC 1d (running-flat Apr→Aug 2025)
        // was missing entirely. We now iterate every L0..L4 level per
        // tick and persist with `slot = level.as_index()` so each Z
        // toggle on the chart shows its own degree's patterns.
        let levels = [
            PivotLevel::L0,
            PivotLevel::L1,
            PivotLevel::L2,
            PivotLevel::L3,
            PivotLevel::L4,
        ];
        let mut sets: Vec<(PivotLevel, ElliottDetectorSet)> = Vec::new();
        for &level in &levels {
            let cfg = build_config(pool, level).await;
            match ElliottDetectorSet::new(cfg, &toggles) {
                Ok(s) if !s.is_empty() => sets.push((level, s)),
                Ok(_) => {}
                Err(e) => warn!(
                    %e,
                    level = level.as_str(),
                    "elliott_full: invalid config for level, skipping",
                ),
            }
        }
        if sets.is_empty() {
            return Ok(stats);
        }

        for sym in &syms {
            for (level, set) in &sets {
                match process_symbol(
                    pool,
                    sym,
                    bars_limit,
                    pivots_per_slot,
                    *level,
                    set,
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
                        level = level.as_str(),
                        %e,
                        "elliott_full: series failed"
                    ),
                }
            }
        }
        Ok(stats)
    }
}

// ── tick body ─────────────────────────────────────────────────────────

async fn process_symbol(
    pool: &PgPool,
    sym: &EngineSymbol,
    bars_limit: i64,
    pivots_per_slot: i64,
    level: PivotLevel,
    set: &ElliottDetectorSet,
) -> anyhow::Result<usize> {
    let stored = list_pivots_by_slot(pool, sym.id, level.as_index() as i16, pivots_per_slot).await?;
    if stored.len() < 6 {
        // Smallest formation here (truncated fifth) needs 6 pivots
        // (W0..W5). Anything thinner has zero chance of matching, skip
        // the heavy detector pass.
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
    let _ = bars; // detectors operate on PivotTree, bars only needed for
                  // future SmcWriter-style adapter (kept for parity).

    let pivots: Vec<DomainPivot> = stored.iter().map(|p| to_domain_pivot(p, level)).collect();
    let tree = pivot_tree_only_at(level, pivots);
    let regime = RegimeSnapshot::neutral_default();

    let detections = set.detect_all(&tree, &instrument, tf, &regime);
    let mut written = 0usize;
    for d in detections {
        if let Some(n) = write_detection(pool, sym, level, &chrono, &d).await? {
            written += n;
        }
    }
    Ok(written)
}

fn pivot_tree_only_at(level: PivotLevel, pivots: Vec<DomainPivot>) -> PivotTree {
    let empty = || Vec::<DomainPivot>::new();
    let (l0, l1, l2, l3, l4) = match level {
        PivotLevel::L0 => (pivots, empty(), empty(), empty(), empty()),
        PivotLevel::L1 => (empty(), pivots, empty(), empty(), empty()),
        PivotLevel::L2 => (empty(), empty(), pivots, empty(), empty()),
        PivotLevel::L3 => (empty(), empty(), empty(), pivots, empty()),
        PivotLevel::L4 => (empty(), empty(), empty(), empty(), pivots),
    };
    PivotTree::new(l0, l1, l2, l3, l4)
}

// ── persistence ───────────────────────────────────────────────────────

/// Map a detection's `kind` to (`pattern_family`, `subkind`). All
/// elliott_full detections share `pattern_family = 'elliott_full'` so
/// frontend filters can opt in/out as a unit; the family already
/// telegraphs "this row came from the dormant detector pass".
fn family_subkind_for(d: &Detection) -> Option<(String, String)> {
    match &d.kind {
        PatternKind::Elliott(s) => {
            // Skip subkinds the LuxAlgo + elliott_early writers already
            // cover so duplicates can't slip through if a toggle is
            // accidentally flipped on.
            let blocked = [
                "impulse_5_",
                "zigzag_",
                "triangle_",
                "impulse_nascent_",
                "impulse_forming_",
                "abc_nascent_",
                "abc_forming_",
            ];
            if blocked.iter().any(|p| s.starts_with(p)) {
                return None;
            }
            Some(("elliott_full".to_string(), s.clone()))
        }
        _ => None,
    }
}

async fn write_detection(
    pool: &PgPool,
    sym: &EngineSymbol,
    level: PivotLevel,
    chrono_rows: &[MarketBarRow],
    d: &Detection,
) -> anyhow::Result<Option<usize>> {
    let Some((family, subkind)) = family_subkind_for(d) else {
        return Ok(None);
    };
    if d.anchors.is_empty() {
        return Ok(None);
    }

    let direction: i16 = direction_from_subkind(&subkind);
    let start_bar = d.anchors.first().map(|a| a.bar_index as i64).unwrap_or(0);
    let end_bar = d.anchors.last().map(|a| a.bar_index as i64).unwrap_or(start_bar);
    let (start_time, end_time) = anchor_time_range(chrono_rows, start_bar, end_bar);
    let anchors = anchors_with_times(d, chrono_rows);
    let projected = projected_with_times(d, chrono_rows);

    let mut raw_meta = json!({
        "structural_score": d.structural_score,
        "invalidation_price": d.invalidation_price.to_f64().unwrap_or(0.0),
        "state": match d.state {
            PatternState::Forming => "forming",
            PatternState::Confirmed => "confirmed",
            PatternState::Invalidated => "invalidated",
            PatternState::Completed => "completed",
        },
    });
    if !projected.as_array().map(|a| a.is_empty()).unwrap_or(true) {
        raw_meta["projected_anchors"] = projected;
    }
    if let Value::Object(extra) = d.raw_meta.clone() {
        if let Value::Object(target) = &mut raw_meta {
            for (k, v) in extra {
                target.entry(k).or_insert(v);
            }
        }
    }
    let invalidated = matches!(d.state, PatternState::Invalidated);

    let slot = level.as_index() as i16;
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
               direction   = EXCLUDED.direction,
               anchors     = EXCLUDED.anchors,
               invalidated = EXCLUDED.invalidated,
               raw_meta    = EXCLUDED.raw_meta,
               updated_at  = now()"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .bind(slot)
    .bind(&family)
    .bind(&subkind)
    .bind(direction)
    .bind(start_bar)
    .bind(end_bar)
    .bind(start_time)
    .bind(end_time)
    .bind(&anchors)
    .bind(invalidated)
    .bind(&raw_meta)
    .execute(pool)
    .await?;
    Ok(Some(1))
}

/// Subkind suffix `_bull` / `_bear` carries the side. Default 0 when a
/// detector emits a non-directional label (e.g. `combination_wxy`
/// without a direction tag — none currently do, but the writer is
/// resilient to future detector additions).
fn direction_from_subkind(s: &str) -> i16 {
    if s.ends_with("_bull") {
        1
    } else if s.ends_with("_bear") {
        -1
    } else {
        0
    }
}

fn anchors_with_times(d: &Detection, chrono: &[MarketBarRow]) -> Value {
    let arr: Vec<Value> = d
        .anchors
        .iter()
        .map(|a| {
            let bar_index = a.bar_index as i64;
            let time = chrono.get(a.bar_index as usize).map(|r| r.open_time);
            let mut obj = json!({
                "bar_index": bar_index,
                "price": a.price.to_f64().unwrap_or(0.0),
                "level": a.level.as_str(),
            });
            if let Some(label) = &a.label {
                obj["label"] = json!(label);
                obj["label_override"] = json!(label);
            }
            if let Some(t) = time {
                obj["time"] = json!(t);
            }
            obj
        })
        .collect();
    Value::Array(arr)
}

fn projected_with_times(d: &Detection, chrono: &[MarketBarRow]) -> Value {
    let arr: Vec<Value> = d
        .projected_anchors
        .iter()
        .map(|a| {
            let bar_index = a.bar_index as i64;
            let time = chrono.get(a.bar_index as usize).map(|r| r.open_time);
            let mut obj = json!({
                "bar_index": bar_index,
                "price": a.price.to_f64().unwrap_or(0.0),
                "level": a.level.as_str(),
                "projected": true,
            });
            if let Some(label) = &a.label {
                obj["label"] = json!(format!("{label}?"));
                obj["label_override"] = json!(format!("{label}?"));
            }
            if let Some(t) = time {
                obj["time"] = json!(t);
            }
            obj
        })
        .collect();
    Value::Array(arr)
}

fn anchor_time_range(
    chrono: &[MarketBarRow],
    start_bar: i64,
    end_bar: i64,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let clamp = |b: i64| -> Option<DateTime<Utc>> { chrono.get(b.max(0) as usize).map(|r| r.open_time) };
    let start = clamp(start_bar)
        .or_else(|| chrono.first().map(|r| r.open_time))
        .unwrap_or_else(Utc::now);
    let end = clamp(end_bar)
        .or_else(|| chrono.last().map(|r| r.open_time))
        .unwrap_or_else(Utc::now);
    (start.min(end), start.max(end))
}

// ── pivot adapter ─────────────────────────────────────────────────────

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
        kind: if p.direction > 0 { PivotKind::High } else { PivotKind::Low },
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

// ── config loading ────────────────────────────────────────────────────

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='elliott_full' AND config_key='enabled'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return true; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

struct LoadedNum {
    field: &'static str,
    default: i64,
    min: i64,
    max: i64,
}

impl LoadedNum {
    fn clamp_default(self, def: i64, min: i64, max: i64) -> Self {
        Self {
            field: self.field,
            default: def,
            min,
            max,
        }
    }
    async fn fetch(self, pool: &PgPool) -> i64 {
        let row = sqlx::query(
            "SELECT value FROM system_config
               WHERE module='elliott_full' AND config_key=$1",
        )
        .bind(self.field)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        let val = match row {
            Some(r) => r
                .try_get::<Value, _>("value")
                .ok()
                .and_then(|v| v.get("value").or_else(|| v.get("bars")).or_else(|| v.get("count")).cloned())
                .and_then(|v| v.as_i64())
                .unwrap_or(self.default),
            None => self.default,
        };
        val.clamp(self.min, self.max)
    }
}

fn load_num(_pool: &PgPool, key: &'static str, default: i64) -> LoadedNum {
    LoadedNum {
        field: key,
        default,
        min: i64::MIN,
        max: i64::MAX,
    }
}

async fn load_pivot_level(pool: &PgPool) -> PivotLevel {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='elliott_full' AND config_key='pivot_level'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return PivotLevel::L1; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    let raw = val
        .get("value")
        .and_then(|v| v.as_str())
        .or_else(|| val.as_str())
        .unwrap_or("L1");
    match raw.to_uppercase().as_str() {
        "L0" => PivotLevel::L0,
        "L2" => PivotLevel::L2,
        "L3" => PivotLevel::L3,
        "L4" => PivotLevel::L4,
        _ => PivotLevel::L1,
    }
}

async fn load_toggles(pool: &PgPool) -> ElliottFormationToggles {
    // Defaults: every dormant detector ON, every detector that already
    // has a writer OFF. Operators flip individual subkinds via
    // `system_config.elliott_full.toggles.<name>`.
    let mut t = ElliottFormationToggles::defaults();
    t.impulse = false; // owned by ElliottWriter (LuxAlgo motive)
    t.nascent_impulse = false; // owned by elliott_early
    t.forming_impulse = false; // owned by elliott_early
    t.zigzag = false; // owned by ElliottWriter (LuxAlgo abc)
    t.triangle = false; // owned by ElliottWriter (LuxAlgo triangle)
    t.luxalgo = false; // would re-emit motive/abc, double-write

    macro_rules! pull {
        ($field:ident) => {{
            let key = concat!("toggles.", stringify!($field));
            if let Ok(Some(row)) = sqlx::query(
                "SELECT value FROM system_config
                   WHERE module='elliott_full' AND config_key=$1",
            )
            .bind(key)
            .fetch_optional(pool)
            .await
            {
                let val: Value = row.try_get("value").unwrap_or(Value::Null);
                if let Some(b) = val.get("enabled").and_then(|v| v.as_bool()).or_else(|| val.as_bool()) {
                    t.$field = b;
                }
            }
        }};
    }
    pull!(leading_diagonal);
    pull!(ending_diagonal);
    pull!(flat);
    pull!(extended_impulse);
    pull!(truncated_fifth);
    pull!(combination);
    t
}

async fn build_config(pool: &PgPool, level: PivotLevel) -> ElliottConfig {
    let mut cfg = ElliottConfig::defaults();
    cfg.pivot_level = level;

    if let Ok(Some(row)) = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='elliott_full' AND config_key='min_structural_score'",
    )
    .fetch_optional(pool)
    .await
    {
        let val: Value = row.try_get("value").unwrap_or(Value::Null);
        if let Some(f) = val.get("value").and_then(|v| v.as_f64()) {
            cfg.min_structural_score = f as f32;
        }
    }
    if cfg.validate().is_err() {
        cfg = ElliottConfig::defaults();
        cfg.pivot_level = level;
    }
    cfg
}

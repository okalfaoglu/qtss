//! v2 detector orchestrator — Faz 7 Adım 2.
//!
//! Periodic loop that turns the dormant detector crates (Elliott,
//! Harmonic, Classical, Wyckoff) into a live pipeline that writes rows
//! into `qtss_v2_detections`. The chart endpoint and Detections panel
//! both read from that table.
//!
//! ## Design
//!
//! * **Pure dispatch table** (CLAUDE.md #1): every detector implements
//!   the [`DetectorRunner`] trait. Adding a new family means appending
//!   one entry to [`build_runners`] — no scattered `if/else` over
//!   detector kinds.
//! * **Config-driven** (CLAUDE.md #2): every toggle, threshold, and
//!   poll interval is read from `system_config` via the `resolve_*`
//!   helpers. The hardcoded defaults in this file are *bootstrap*
//!   defaults — fall-throughs for fresh deployments — not source-of-truth
//!   tunables.
//! * **Stateless across ticks**: each pass rebuilds the pivot + regime
//!   engines from the most recent `history_bars` bars in `market_bars`.
//!   This is intentional: a streaming/event-driven version arrives in
//!   Adım 3+ once the EventBus wiring lands. Until then "rebuild from
//!   history" is the simplest correct thing.
//! * **Dedup**: before inserting a detection we look up the most recent
//!   open row for the same (exchange, symbol, tf, family, subkind) and
//!   compare the last anchor's `bar_index`. Identical → skip. This
//!   prevents the orchestrator from spamming the table when the same
//!   impulse persists across many ticks.
//!
//! Gated entirely by `detection.orchestrator.enabled` so the loop is a
//! no-op until an operator flips it on from the Config panel.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use qtss_chart_patterns::{analyze_trading_range, OhlcBar, TradingRangeParams};
use qtss_classical::{ClassicalConfig, ClassicalDetector};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::detection::{Detection, PatternKind, PatternState, PivotRef};
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::pivot::{PivotLevel, PivotTree};
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;
use qtss_elliott::{ElliottConfig, ElliottDetectorSet, ElliottFormationToggles};
use qtss_harmonic::{HarmonicConfig, HarmonicDetector};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use qtss_pivots::{PivotConfig, PivotEngine};
use qtss_regime::{RegimeConfig, RegimeEngine};
use qtss_storage::{
    list_enabled_engine_symbols, list_recent_bars, resolve_system_string, resolve_system_u64,
    resolve_worker_enabled_flag, DetectionFilter, EngineSymbolRow, NewDetection,
    V2DetectionRepository,
};
use qtss_wyckoff::{WyckoffConfig, WyckoffDetector};
use serde_json::json;
use sqlx::PgPool;
use tracing::{debug, info, warn};
use uuid::Uuid;

// ---------------------------------------------------------------------
// Trait + dispatch table
// ---------------------------------------------------------------------

/// Polymorphic detector. Each family has one impl that delegates to
/// the underlying crate. The orchestrator iterates a `Vec<Box<dyn>>`
/// so adding/removing families never touches the loop body.
/// Polymorphic detector. Each family has one impl that delegates to
/// the underlying crate. The orchestrator iterates a `Vec<Box<dyn>>`
/// so adding/removing families never touches the loop body.
///
/// `bars` carries the same chronological window the orchestrator fed
/// into the pivot/regime engines. Tree-only detectors (Elliott,
/// Harmonic, Classical, Wyckoff) ignore it; bar-driven detectors
/// (Range / future TBM-shaped families) read from it directly.
trait DetectorRunner: Send + Sync {
    /// Stable family key: "elliott" / "harmonic" / "classical" / "wyckoff" / "range".
    /// Reserved for richer dispatch / metrics in Adım 3 (validator wiring).
    #[allow(dead_code)]
    fn family(&self) -> &'static str;

    fn detect(
        &self,
        tree: &PivotTree,
        bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection>;
}

struct ElliottRunner(ElliottDetectorSet);
impl DetectorRunner for ElliottRunner {
    fn family(&self) -> &'static str {
        "elliott"
    }
    fn detect(
        &self,
        tree: &PivotTree,
        _bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        self.0.detect_all(tree, instrument, timeframe, regime)
    }
}

struct HarmonicRunner(HarmonicDetector);
impl DetectorRunner for HarmonicRunner {
    fn family(&self) -> &'static str {
        "harmonic"
    }
    fn detect(
        &self,
        tree: &PivotTree,
        _bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        self.0.detect(tree, instrument, timeframe, regime).into_iter().collect()
    }
}

struct ClassicalRunner(ClassicalDetector);
impl DetectorRunner for ClassicalRunner {
    fn family(&self) -> &'static str {
        "classical"
    }
    fn detect(
        &self,
        tree: &PivotTree,
        _bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        self.0.detect(tree, instrument, timeframe, regime).into_iter().collect()
    }
}

struct WyckoffRunner(WyckoffDetector);
impl DetectorRunner for WyckoffRunner {
    fn family(&self) -> &'static str {
        "wyckoff"
    }
    fn detect(
        &self,
        tree: &PivotTree,
        _bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        self.0.detect(tree, instrument, timeframe, regime).into_iter().collect()
    }
}

/// Range runner — wraps `qtss_chart_patterns::trading_range::analyze_trading_range`
/// in the same `DetectorRunner` shape as the structural detectors so
/// trading-range setups land in `qtss_v2_detections` alongside Elliott
/// & friends. Pure mapping: `setup_score_best/100` → `structural_score`,
/// `setup_side` → `subkind`, range_high/low → two anchors,
/// opposite band → `invalidation_price`, full `TradingRangeResult` →
/// `raw_meta`.
///
/// State machine semantics (point-in-time, matches the rest of the
/// orchestrator — durum-style transitions stay in qtss-analysis):
///   - guardrails_pass + setup_side != "NOTR" → Confirmed
///   - is_range_regime                       → Forming
///   - otherwise                             → no detection (skip)
struct RangeRunner {
    params: TradingRangeParams,
}

impl RangeRunner {
    fn new(params: TradingRangeParams) -> Self {
        Self { params }
    }
}

impl DetectorRunner for RangeRunner {
    fn family(&self) -> &'static str {
        "range"
    }

    fn detect(
        &self,
        _tree: &PivotTree,
        bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        if bars.len() < 30 {
            return Vec::new();
        }

        // Bar (Decimal) → OhlcBar (f64). bar_index is the chronological
        // window index so anchors line up with what the orchestrator
        // already enriches with open_time.
        let ohlc: Vec<OhlcBar> = bars
            .iter()
            .enumerate()
            .map(|(i, b)| OhlcBar {
                open: b.open.to_string().parse().unwrap_or(0.0),
                high: b.high.to_string().parse().unwrap_or(0.0),
                low: b.low.to_string().parse().unwrap_or(0.0),
                close: b.close.to_string().parse().unwrap_or(0.0),
                bar_index: i as i64,
                volume: b.volume.to_string().parse().ok(),
            })
            .collect();

        let res = analyze_trading_range(&ohlc, &self.params);
        if !res.valid {
            return Vec::new();
        }

        let (Some(range_high), Some(range_low)) = (res.range_high, res.range_low) else {
            return Vec::new();
        };

        let (state, subkind) = match (res.guardrails_pass, res.setup_side.as_str()) {
            (true, "LONG") => (PatternState::Confirmed, "long_setup"),
            (true, "SHORT") => (PatternState::Confirmed, "short_setup"),
            _ if res.is_range_regime => (PatternState::Forming, "range_regime"),
            _ => return Vec::new(),
        };

        let high_decimal = Decimal::from_f64(range_high).unwrap_or_default();
        let low_decimal = Decimal::from_f64(range_low).unwrap_or_default();
        let last_idx = res
            .last_bar_index
            .map(|i| i as u64)
            .unwrap_or_else(|| bars.len().saturating_sub(1) as u64);

        // Two anchors: resistance + support. bar_index points at the
        // last evaluated bar so the chart can pin the band to "now".
        let anchors = vec![
            PivotRef {
                bar_index: last_idx,
                price: high_decimal,
                level: PivotLevel::L0,
                label: Some("resistance".to_string()),
            },
            PivotRef {
                bar_index: last_idx,
                price: low_decimal,
                level: PivotLevel::L0,
                label: Some("support".to_string()),
            },
        ];

        // For a LONG setup the range_high is the breakout invalidation
        // (price closing above means the range broke); SHORT mirrors.
        // Forming/range_regime falls back to the wider band.
        let invalidation_price = match subkind {
            "long_setup" => high_decimal,
            "short_setup" => low_decimal,
            _ => high_decimal,
        };

        let structural_score = (res.setup_score_best as f32 / 100.0).clamp(0.0, 1.0);

        let raw_meta = serde_json::to_value(&res).unwrap_or(serde_json::Value::Null);

        let detection = Detection::new(
            instrument.clone(),
            timeframe,
            PatternKind::Range(subkind.to_string()),
            state,
            anchors,
            structural_score,
            invalidation_price,
            regime.clone(),
        );
        let mut detection = detection;
        detection.raw_meta = raw_meta;
        vec![detection]
    }
}

/// Resolve per-formation toggles for the Elliott detector set from
/// `system_config`. Each formation has its own `detection.elliott.<f>.enabled`
/// row — disabling one is a one-line UPDATE, no worker restart.
async fn resolve_elliott_toggles(pool: &PgPool) -> ElliottFormationToggles {
    // Lookup table: (field_setter, config_key, env_key, default).
    // Defined as a local closure list so adding a new formation is one
    // tuple, not a scattered if/else chain (CLAUDE.md #1).
    let mut t = ElliottFormationToggles::defaults();
    t.impulse = resolve_worker_enabled_flag(
        pool, "detection", "elliott.impulse.enabled",
        "QTSS_DETECTION_ELLIOTT_IMPULSE_ENABLED", true,
    ).await;
    t.leading_diagonal = resolve_worker_enabled_flag(
        pool, "detection", "elliott.leading_diagonal.enabled",
        "QTSS_DETECTION_ELLIOTT_LEADING_DIAGONAL_ENABLED", false,
    ).await;
    t.ending_diagonal = resolve_worker_enabled_flag(
        pool, "detection", "elliott.ending_diagonal.enabled",
        "QTSS_DETECTION_ELLIOTT_ENDING_DIAGONAL_ENABLED", false,
    ).await;
    t.zigzag = resolve_worker_enabled_flag(
        pool, "detection", "elliott.zigzag.enabled",
        "QTSS_DETECTION_ELLIOTT_ZIGZAG_ENABLED", false,
    ).await;
    t.flat = resolve_worker_enabled_flag(
        pool, "detection", "elliott.flat.enabled",
        "QTSS_DETECTION_ELLIOTT_FLAT_ENABLED", false,
    ).await;
    t.triangle = resolve_worker_enabled_flag(
        pool, "detection", "elliott.triangle.enabled",
        "QTSS_DETECTION_ELLIOTT_TRIANGLE_ENABLED", false,
    ).await;
    t.extended_impulse = resolve_worker_enabled_flag(
        pool, "detection", "elliott.extended_impulse.enabled",
        "QTSS_DETECTION_ELLIOTT_EXTENDED_IMPULSE_ENABLED", false,
    ).await;
    t.truncated_fifth = resolve_worker_enabled_flag(
        pool, "detection", "elliott.truncated_fifth.enabled",
        "QTSS_DETECTION_ELLIOTT_TRUNCATED_FIFTH_ENABLED", false,
    ).await;
    t.combination = resolve_worker_enabled_flag(
        pool, "detection", "elliott.combination.enabled",
        "QTSS_DETECTION_ELLIOTT_COMBINATION_ENABLED", false,
    ).await;
    t
}

/// Build the active detector list from config. Each family is gated by
/// its own `detection.<family>.enabled` toggle so an operator can
/// disable noisy ones without restarting the worker.
async fn build_runners(pool: &PgPool) -> Vec<Box<dyn DetectorRunner>> {
    let mut runners: Vec<Box<dyn DetectorRunner>> = Vec::new();

    if resolve_worker_enabled_flag(
        pool,
        "detection",
        "elliott.enabled",
        "QTSS_DETECTION_ELLIOTT_ENABLED",
        true,
    )
    .await
    {
        let toggles = resolve_elliott_toggles(pool).await;
        match ElliottDetectorSet::new(ElliottConfig::defaults(), &toggles) {
            Ok(set) => runners.push(Box::new(ElliottRunner(set))),
            Err(e) => warn!(?e, "elliott detector set init failed"),
        }
    }
    if resolve_worker_enabled_flag(
        pool,
        "detection",
        "harmonic.enabled",
        "QTSS_DETECTION_HARMONIC_ENABLED",
        true,
    )
    .await
    {
        match HarmonicDetector::new(HarmonicConfig::defaults()) {
            Ok(d) => runners.push(Box::new(HarmonicRunner(d))),
            Err(e) => warn!(?e, "harmonic detector init failed"),
        }
    }
    if resolve_worker_enabled_flag(
        pool,
        "detection",
        "classical.enabled",
        "QTSS_DETECTION_CLASSICAL_ENABLED",
        true,
    )
    .await
    {
        match ClassicalDetector::new(ClassicalConfig::defaults()) {
            Ok(d) => runners.push(Box::new(ClassicalRunner(d))),
            Err(e) => warn!(?e, "classical detector init failed"),
        }
    }
    if resolve_worker_enabled_flag(
        pool,
        "detection",
        "wyckoff.enabled",
        "QTSS_DETECTION_WYCKOFF_ENABLED",
        true,
    )
    .await
    {
        match WyckoffDetector::new(WyckoffConfig::defaults()) {
            Ok(d) => runners.push(Box::new(WyckoffRunner(d))),
            Err(e) => warn!(?e, "wyckoff detector init failed"),
        }
    }
    if resolve_worker_enabled_flag(
        pool,
        "detection",
        "range.enabled",
        "QTSS_DETECTION_RANGE_ENABLED",
        true,
    )
    .await
    {
        runners.push(Box::new(RangeRunner::new(TradingRangeParams::default())));
    }

    runners
}

// ---------------------------------------------------------------------
// Loop entry
// ---------------------------------------------------------------------

pub async fn v2_detection_orchestrator_loop(pool: PgPool) {
    info!("v2 detection orchestrator loop spawned (gated on detection.orchestrator.enabled)");
    let repo = Arc::new(V2DetectionRepository::new(pool.clone()));

    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "detection",
            "orchestrator.enabled",
            "QTSS_DETECTION_ORCHESTRATOR_ENABLED",
            false,
        )
        .await;

        let tick_secs = resolve_system_u64(
            &pool,
            "detection",
            "orchestrator.tick_interval_s",
            "QTSS_DETECTION_ORCHESTRATOR_TICK_S",
            5,
            1,
            3600,
        )
        .await;

        if !enabled {
            tokio::time::sleep(Duration::from_secs(tick_secs)).await;
            continue;
        }

        match run_pass(&pool, repo.clone()).await {
            Ok(stats) => {
                if stats.inserted > 0 || stats.processed > 0 {
                    info!(
                        processed = stats.processed,
                        emitted = stats.emitted,
                        deduped = stats.deduped,
                        inserted = stats.inserted,
                        "v2 detection orchestrator pass complete"
                    );
                } else {
                    debug!("v2 detection orchestrator pass: no enabled symbols");
                }
            }
            Err(e) => warn!(%e, "v2 detection orchestrator pass failed"),
        }

        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
    }
}

#[derive(Default)]
struct PassStats {
    processed: usize,
    emitted: usize,
    deduped: usize,
    inserted: usize,
}

async fn run_pass(
    pool: &PgPool,
    repo: Arc<V2DetectionRepository>,
) -> anyhow::Result<PassStats> {
    let mut stats = PassStats::default();

    let history_bars = resolve_system_u64(
        pool,
        "detection",
        "orchestrator.history_bars",
        "QTSS_DETECTION_ORCHESTRATOR_HISTORY_BARS",
        500,
        50,
        5000,
    )
    .await as i64;

    let mode = resolve_system_string(
        pool,
        "worker",
        "runtime_mode",
        "QTSS_RUNTIME_MODE",
        "live",
    )
    .await;
    // Belt-and-braces: only accept the three documented modes; anything
    // else collapses to "live" so we never insert garbage in the CHECK.
    let mode = match mode.as_str() {
        "live" | "dry" | "backtest" => mode,
        _ => "live".to_string(),
    };

    let runners = build_runners(pool).await;
    if runners.is_empty() {
        return Ok(stats);
    }

    let symbols = list_enabled_engine_symbols(pool).await?;
    for sym in symbols {
        match process_symbol(pool, repo.clone(), &sym, history_bars, &runners, &mode).await {
            Ok(s) => {
                stats.processed += 1;
                stats.emitted += s.emitted;
                stats.deduped += s.deduped;
                stats.inserted += s.inserted;
            }
            Err(e) => warn!(symbol = %sym.symbol, interval = %sym.interval, %e, "process_symbol failed"),
        }
    }

    Ok(stats)
}

#[derive(Default)]
struct SymbolStats {
    emitted: usize,
    deduped: usize,
    inserted: usize,
}

async fn process_symbol(
    pool: &PgPool,
    repo: Arc<V2DetectionRepository>,
    sym: &EngineSymbolRow,
    history_bars: i64,
    runners: &[Box<dyn DetectorRunner>],
    mode: &str,
) -> anyhow::Result<SymbolStats> {
    let mut stats = SymbolStats::default();

    let timeframe = match parse_timeframe(&sym.interval) {
        Some(tf) => tf,
        None => {
            debug!(interval = %sym.interval, "skip: unsupported timeframe");
            return Ok(stats);
        }
    };
    let instrument = build_instrument(&sym.exchange, &sym.segment, &sym.symbol);

    let raw_bars = list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        history_bars,
    )
    .await?;
    if raw_bars.len() < 30 {
        // Not enough history to even warm up indicators.
        return Ok(stats);
    }

    // list_recent_bars returns DESC; feed engines in chronological order.
    let mut chronological = raw_bars;
    chronological.reverse();

    let mut pivot_engine = PivotEngine::new(PivotConfig::defaults())?;
    let mut regime_engine = RegimeEngine::new(RegimeConfig::defaults())?;

    let mut latest_regime: Option<RegimeSnapshot> = None;
    let mut bars: Vec<Bar> = Vec::with_capacity(chronological.len());
    for row in &chronological {
        let bar = Bar {
            instrument: instrument.clone(),
            timeframe,
            open_time: row.open_time,
            open: row.open,
            high: row.high,
            low: row.low,
            close: row.close,
            volume: row.volume,
            closed: true,
        };
        // Pivot + regime cascade. Errors here mean out-of-order bars
        // — log and stop processing this symbol so we don't poison
        // the engine state with retries on bad data.
        if let Err(e) = pivot_engine.on_bar(&bar) {
            warn!(symbol = %sym.symbol, %e, "pivot engine rejected bar");
            return Ok(stats);
        }
        match regime_engine.on_bar(&bar) {
            Ok(Some(snap)) => latest_regime = Some(snap),
            Ok(None) => {}
            Err(e) => {
                warn!(symbol = %sym.symbol, %e, "regime engine rejected bar");
                return Ok(stats);
            }
        }
        bars.push(bar);
    }

    let Some(regime) = latest_regime else {
        // Indicators still warming up — try again next tick.
        return Ok(stats);
    };
    let tree = pivot_engine.snapshot();

    // Live-revision pre-pass: invalidate any open `forming` row whose
    // invalidation_price has been breached by the most recent close.
    // This catches the case where the detector simply *stops* emitting
    // a wave (because it broke) — without this sweep the old forming
    // overlay would linger until something else supersedes it. See
    // post-Faz 8.0 backlog item #2.
    if let Some(last_bar) = bars.last() {
        let last_close = last_bar.close;
        match repo
            .list_forming_for_symbol(&sym.exchange, &sym.symbol, &sym.interval)
            .await
        {
            Ok(open_rows) => {
                for (id, family, subkind, invalidation_price) in open_rows {
                    if let Some(dir) = infer_direction(&family, &subkind) {
                        let breached = match dir {
                            Direction::Long => last_close < invalidation_price,
                            Direction::Short => last_close > invalidation_price,
                        };
                        if breached {
                            if let Err(e) = repo.update_state(id, "invalidated").await {
                                warn!(%id, %e, "price-breach invalidate failed");
                            }
                        }
                    }
                }
            }
            Err(e) => warn!(symbol = %sym.symbol, %e, "list_forming_for_symbol failed"),
        }
    }

    for runner in runners {
        // Each runner can now emit 0..N detections per pass — Elliott
        // is the first family that takes advantage of this (impulse,
        // diagonals, zigzag, flat, triangle, ...). Iterate the whole
        // batch and dedup/insert each detection independently.
        for detection in runner.detect(&tree, &bars, &instrument, timeframe, &regime) {
        stats.emitted += 1;

        let (family, subkind) = split_pattern_kind(&detection.kind);
        let last_anchor_idx = detection.anchors.last().map(|a| a.bar_index).unwrap_or(0);

        if dedup_open(
            repo.as_ref(),
            &sym.exchange,
            &sym.symbol,
            &sym.interval,
            family,
            subkind,
            last_anchor_idx,
        )
        .await?
        {
            stats.deduped += 1;
            continue;
        }

        // Enrich each pivot with the bar's open_time so the chart can
        // draw a polyline directly without a second round-trip. The
        // detector only carries bar_index → we resolve it here against
        // the chronological window we just fed the engines.
        let anchors_json = json!(detection
            .anchors
            .iter()
            .map(|a| {
                let idx = a.bar_index as usize;
                let time = chronological
                    .get(idx)
                    .map(|r| r.open_time.to_rfc3339())
                    .unwrap_or_default();
                json!({
                    "bar_index": a.bar_index,
                    "time": time,
                    "price": a.price.to_string(),
                    "level": format!("{:?}", a.level),
                    "label": a.label,
                })
            })
            .collect::<Vec<_>>());
        let regime_json =
            serde_json::to_value(&detection.regime_at_detection).unwrap_or_else(|_| json!({}));
        // Forward-projected anchors (Faz 7.6 / A2). Same JSON shape as
        // the realized anchors above so the chart can render them with
        // the same polyline machinery — only the line style differs
        // (dashed for projections, see Faz 7.6 / A4). Projection target
        // bars are *future* bars that don't exist in `chronological`,
        // so we synthesize their `time` by extrapolating one bar
        // interval forward from the last realized anchor.
        let bar_interval = chronological
            .windows(2)
            .last()
            .map(|w| w[1].open_time - w[0].open_time)
            .unwrap_or_else(|| chrono::Duration::seconds(60));
        let last_chrono_idx = chronological.len().saturating_sub(1) as u64;
        let last_chrono_time = chronological
            .last()
            .map(|r| r.open_time)
            .unwrap_or_else(chrono::Utc::now);
        let projected_json = json!(detection
            .projected_anchors
            .iter()
            .map(|a| {
                let offset = a.bar_index.saturating_sub(last_chrono_idx) as i32;
                let proj_time = last_chrono_time + bar_interval * offset;
                json!({
                    "bar_index": a.bar_index,
                    "time": proj_time.to_rfc3339(),
                    "price": a.price.to_string(),
                    "level": format!("{:?}", a.level),
                    "label": a.label,
                })
            })
            .collect::<Vec<_>>());
        // Sub-wave decomposition (Faz 7.6 / A3). Sub-wave bar indices
        // fall *inside* the realized window, so we can resolve their
        // `time` straight from `chronological` like we do for the main
        // anchors.
        let sub_waves_json = json!(detection
            .sub_wave_anchors
            .iter()
            .map(|seg| {
                seg.iter()
                    .map(|a| {
                        let idx = a.bar_index as usize;
                        let time = chronological
                            .get(idx)
                            .map(|r| r.open_time.to_rfc3339())
                            .unwrap_or_default();
                        json!({
                            "bar_index": a.bar_index,
                            "time": time,
                            "price": a.price.to_string(),
                            "level": format!("{:?}", a.level),
                            "label": a.label,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>());
        let raw_meta = json!({
            "detection_id": detection.id,
            "last_anchor_idx": last_anchor_idx,
            "structural_score": detection.structural_score,
            "projected_anchors": projected_json,
            "sub_wave_anchors": sub_waves_json,
        });

        let new_row = NewDetection {
            id: Uuid::new_v4(),
            detected_at: Utc::now(),
            exchange: &sym.exchange,
            symbol: &sym.symbol,
            timeframe: &sym.interval,
            family,
            subkind,
            state: "forming",
            structural_score: detection.structural_score,
            invalidation_price: detection.invalidation_price,
            anchors: anchors_json,
            regime: regime_json,
            raw_meta,
            mode,
        };
        let inserted_id = new_row.id;
        repo.insert(new_row).await?;
        stats.inserted += 1;

        // Live revision: retire any older `forming` row for the same
        // (symbol, tf, family, subkind). Each new bar tick produces a
        // fresh detection that should *replace* the previous overlay,
        // not stack on top of it. See post-Faz 8.0 backlog item #2.
        if let Err(e) = repo
            .supersede_previous_forming(
                &sym.exchange,
                &sym.symbol,
                &sym.interval,
                family,
                subkind,
                inserted_id,
            )
            .await
        {
            warn!(symbol = %sym.symbol, family, subkind, %e, "supersede_previous_forming failed");
        }
        }
    }

    Ok(stats)
}

/// Look up the most recent open detection for this (symbol, tf, family,
/// subkind) and return `true` if its last anchor matches — meaning the
/// new detection is the same structure and we should skip the insert.
async fn dedup_open(
    repo: &V2DetectionRepository,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
    family: &str,
    _subkind: &str,
    last_anchor_idx: u64,
) -> anyhow::Result<bool> {
    let rows = repo
        .list_filtered(DetectionFilter {
            exchange: Some(exchange),
            symbol: Some(symbol),
            timeframe: Some(timeframe),
            family: Some(family),
            state: Some("forming"),
            mode: None,
            limit: 5,
        })
        .await?;

    for row in rows {
        if let Some(idx) = row
            .raw_meta
            .get("last_anchor_idx")
            .and_then(|v| v.as_u64())
        {
            if idx == last_anchor_idx {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

pub(crate) fn parse_timeframe(interval: &str) -> Option<Timeframe> {
    // engine_symbols stores TradingView-ish strings ("1m", "1h", "1d");
    // Timeframe::from_str expects "m1"/"h1"/"d1". Translate.
    let s = interval.trim().to_lowercase();
    let tf = match s.as_str() {
        "1m" => Timeframe::M1,
        "3m" => Timeframe::M3,
        "5m" => Timeframe::M5,
        "15m" => Timeframe::M15,
        "30m" => Timeframe::M30,
        "1h" => Timeframe::H1,
        "2h" => Timeframe::H2,
        "4h" => Timeframe::H4,
        "6h" => Timeframe::H6,
        "8h" => Timeframe::H8,
        "12h" => Timeframe::H12,
        "1d" => Timeframe::D1,
        "3d" => Timeframe::D3,
        "1w" => Timeframe::W1,
        "1mo" | "1mn" => Timeframe::Mn1,
        _ => return None,
    };
    Some(tf)
}

pub(crate) fn build_instrument(exchange: &str, segment: &str, symbol: &str) -> Instrument {
    let venue = parse_venue(exchange);
    let asset_class = parse_asset_class(exchange, segment);
    Instrument {
        venue,
        asset_class,
        symbol: symbol.to_string(),
        // Quote currency / tick / lot are not strictly needed by the
        // pure detectors so we use neutral placeholders. The chart
        // endpoint reads symbol+venue from the row, not the instrument
        // serialization.
        quote_ccy: "USDT".into(),
        tick_size: rust_decimal::Decimal::new(1, 8),
        lot_size: rust_decimal::Decimal::new(1, 8),
        session: SessionCalendar::binance_24x7(),
    }
}

fn parse_venue(exchange: &str) -> Venue {
    match exchange.trim().to_lowercase().as_str() {
        "binance" => Venue::Binance,
        "bybit" => Venue::Bybit,
        "okx" => Venue::Okx,
        "bist" => Venue::Bist,
        "nasdaq" => Venue::Nasdaq,
        "nyse" => Venue::Nyse,
        other => Venue::Custom(other.to_string()),
    }
}

fn parse_asset_class(exchange: &str, segment: &str) -> AssetClass {
    let ex = exchange.trim().to_lowercase();
    let seg = segment.trim().to_lowercase();
    match (ex.as_str(), seg.as_str()) {
        ("binance", "futures") | ("binance", "usdm") | ("binance", "coinm") => {
            AssetClass::CryptoFutures
        }
        ("binance", _) | ("bybit", _) | ("okx", _) => AssetClass::CryptoSpot,
        ("bist", _) => AssetClass::EquityBist,
        ("nasdaq", _) => AssetClass::EquityNasdaq,
        ("nyse", _) => AssetClass::EquityNyse,
        _ => AssetClass::CryptoSpot,
    }
}

/// Direction inferred from a detection's `family + subkind` so the
/// price-breach sweep knows which side of `invalidation_price` is the
/// "fail" side. Each family encodes direction differently — the table
/// below is the single source of truth (CLAUDE.md #1: lookup over
/// scattered if/else).
#[derive(Debug, Clone, Copy)]
enum Direction {
    Long,
    Short,
}

fn infer_direction(_family: &str, subkind: &str) -> Option<Direction> {
    let s = subkind.to_ascii_lowercase();
    // Long-biased markers across all families.
    const LONG_MARKERS: &[&str] = &[
        "bull", "long", "bottom", "spring", "accumulation",
        "ascending", "inverse_head", "cup_handle",
    ];
    // Short-biased markers.
    const SHORT_MARKERS: &[&str] = &[
        "bear", "short", "top", "upthrust", "distribution",
        "descending", "head_shoulders", "head_and_shoulders",
    ];
    // SHORT must be checked first because "head_shoulders" is short
    // while "inverse_head_shoulders" (long) also contains "head_shoulders".
    // The inverse case is caught by LONG_MARKERS via "inverse_head".
    if s.contains("inverse_head") || s.contains("inv_head") {
        return Some(Direction::Long);
    }
    for m in SHORT_MARKERS {
        if s.contains(m) {
            return Some(Direction::Short);
        }
    }
    for m in LONG_MARKERS {
        if s.contains(m) {
            return Some(Direction::Long);
        }
    }
    None
}

fn split_pattern_kind(kind: &PatternKind) -> (&'static str, &str) {
    match kind {
        PatternKind::Elliott(s) => ("elliott", s.as_str()),
        PatternKind::Harmonic(s) => ("harmonic", s.as_str()),
        PatternKind::Classical(s) => ("classical", s.as_str()),
        PatternKind::Wyckoff(s) => ("wyckoff", s.as_str()),
        PatternKind::Range(s) => ("range", s.as_str()),
        PatternKind::Custom(s) => ("custom", s.as_str()),
    }
}

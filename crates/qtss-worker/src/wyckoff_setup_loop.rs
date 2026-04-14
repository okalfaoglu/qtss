//! Wyckoff Setup Engine — periodic signal loop (Faz 10 / 8.0a-wyckoff).
//!
//! Every tick (default 30s), for each enabled engine symbol matching the
//! configured Wyckoff timeframes:
//!   1. Load the active `wyckoff_structures` row (built by
//!      `v2_detection_orchestrator`). No active structure → skip.
//!   2. Rebuild a `WyckoffStructureTracker` from the persisted events.
//!   3. Pull the most recent bars, compute ATR(14) + 20-bar avg volume.
//!   4. Run `signal_emitter::emit()` → 0..N `WyckoffSignal`s passing the
//!      composite score gate.
//!   5. For every mode in `wyckoff.scan.modes` (dry|live|backtest), upsert
//!      a row into `qtss_v2_setups` keyed by the deterministic
//!      `idempotency_key` (migration 0064).
//!
//! CLAUDE.md compliance:
//!   - #1: small helpers (no central match). Profile map is a lookup table.
//!   - #2: every knob — enabled, tick, timeframes, modes, min_score —
//!         resolved from `system_config` (`wyckoff.*`). Defaults exist only
//!         for fast bootstrap; DB values win.
//!   - #5: mode is runtime context, not a feature flag.

use std::collections::HashSet;
use std::time::Duration;

use rust_decimal::prelude::ToPrimitive;
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use tracing::{debug, info, warn};

use qtss_storage::{
    find_active_wyckoff_structure, list_enabled_engine_symbols, list_recent_bars,
    resolve_system_f64, resolve_system_string, resolve_system_u64, resolve_worker_enabled_flag,
    upsert_wyckoff_setup, WyckoffSetupUpsert,
};
use qtss_wyckoff::{
    persistence::signal_to_payload,
    setup_builder::{SetupBar, SetupContext, WyckoffSetupConfig},
    signal_emitter::{default_profile_map, emit, EmitterConfig},
    trade_planner::TradePlannerConfig,
    RecordedEvent, WyckoffPhase, WyckoffSchematic, WyckoffStructureTracker,
};

// =========================================================================
// Entry point — launched from main.rs via `tokio::spawn`
// =========================================================================

pub async fn wyckoff_setup_loop(pool: PgPool) {
    let default_tick: u64 = 30;
    loop {
        // enabled flag / tick resolved each tick — lets operators hot-toggle.
        let enabled = resolve_worker_enabled_flag(
            &pool, "setup", "wyckoff.enabled", "", true,
        )
        .await;
        let tick_secs = resolve_system_u64(
            &pool,
            "setup",
            "wyckoff.scan.interval_seconds",
            "",
            default_tick,
            5,
            3600,
        )
        .await;

        if !enabled {
            debug!("wyckoff_setup_loop: disabled via setup.wyckoff.enabled");
            tokio::time::sleep(Duration::from_secs(tick_secs)).await;
            continue;
        }

        match run_pass(&pool).await {
            Ok(n) => {
                if n > 0 {
                    info!(emitted = n, "wyckoff_setup_loop: pass complete");
                }
            }
            Err(e) => warn!(%e, "wyckoff_setup_loop: pass failed"),
        }

        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
    }
}

// =========================================================================
// Single pass over the enabled symbol universe
// =========================================================================

async fn run_pass(pool: &PgPool) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let timeframes = load_timeframes(pool).await;
    let modes = load_modes(pool).await;
    if timeframes.is_empty() || modes.is_empty() {
        return Ok(0);
    }

    let min_score = resolve_system_f64(
        pool, "setup", "wyckoff.setup.min_score", "", 60.0,
    )
    .await;

    let emit_cfg = EmitterConfig {
        min_score,
        profile_map: default_profile_map(),
        planner: TradePlannerConfig::default(),
        ..EmitterConfig::default()
    };
    let setup_cfg = WyckoffSetupConfig::default();

    let symbols = list_enabled_engine_symbols(pool).await?;
    let mut emitted = 0usize;
    for s in symbols {
        if !timeframes.contains(&s.interval) {
            continue;
        }
        for mode in &modes {
            match process_symbol(pool, &s.exchange, &s.segment, &s.symbol, &s.interval,
                                 mode, &setup_cfg, &emit_cfg).await
            {
                Ok(n) => emitted += n,
                Err(e) => warn!(symbol=%s.symbol, tf=%s.interval, mode=%mode, %e,
                                "wyckoff symbol failed"),
            }
        }
    }
    Ok(emitted)
}

// =========================================================================
// Per (symbol, tf, mode) processing — core of the loop
// =========================================================================

async fn process_symbol(
    pool: &PgPool,
    exchange: &str,
    _segment: &str,
    symbol: &str,
    timeframe: &str,
    mode: &str,
    setup_cfg: &WyckoffSetupConfig,
    emit_cfg: &EmitterConfig,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    // 1. Active Wyckoff structure must exist — orchestrator builds it from
    //    detections. Without one, no phase/range to evaluate setups on.
    let structure = match find_active_wyckoff_structure(pool, symbol, timeframe).await? {
        Some(s) => s,
        None => return Ok(0),
    };
    let tracker = rebuild_tracker(&structure);

    // 2. Load recent bars → SetupBar list (ascending order: oldest first).
    let segment = structure.segment.as_str();
    let raw_bars = list_recent_bars(pool, exchange, segment, symbol, timeframe, 500).await?;
    if raw_bars.len() < 25 {
        return Ok(0); // too few bars for ATR/vol avg
    }
    let mut bars: Vec<SetupBar> = raw_bars
        .iter()
        .rev() // list_recent_bars returns DESC; flip to ASC
        .map(|b| SetupBar {
            ts_ms: b.open_time.timestamp_millis(),
            open:  b.open.to_f64().unwrap_or(0.0),
            high:  b.high.to_f64().unwrap_or(0.0),
            low:   b.low.to_f64().unwrap_or(0.0),
            close: b.close.to_f64().unwrap_or(0.0),
            volume: b.volume.to_f64().unwrap_or(0.0),
        })
        .collect();
    // Drop any zero-priced rows defensively (bad data).
    bars.retain(|b| b.high > 0.0 && b.low > 0.0);
    if bars.len() < 25 { return Ok(0); }

    let atr = compute_atr(&bars, 14);
    let vol_avg_20 = compute_sma_volume(&bars, 20);

    let ctx = SetupContext {
        tracker: &tracker,
        bars: &bars,
        atr,
        vol_avg_20,
        cfg: setup_cfg,
    };

    // 3. Emit — range id = structure uuid so rescans hit the same key.
    let range_id = structure.id.to_string();
    let signals = emit(&ctx, emit_cfg, symbol, timeframe, &range_id);
    if signals.is_empty() {
        return Ok(0);
    }

    // 4. Persist each signal (idempotent upsert on migration 0064 column).
    let venue_class = classify_venue(exchange, segment);
    let mut n = 0;
    for sig in &signals {
        let payload = signal_to_payload(sig, &venue_class, exchange, symbol, timeframe, mode);
        let upsert = WyckoffSetupUpsert {
            idempotency_key: payload.idempotency_key,
            venue_class: payload.venue_class,
            exchange: payload.exchange,
            symbol: payload.symbol,
            timeframe: payload.timeframe,
            mode: payload.mode,
            profile: payload.profile,
            alt_type: payload.alt_type,
            direction: payload.direction,
            entry_price: payload.entry_price,
            entry_sl: payload.entry_sl,
            target_ref: payload.target_ref,
            tp_ladder_json: payload.tp_ladder_json,
            wyckoff_classic_json: payload.wyckoff_classic_json,
            raw_meta_json: payload.raw_meta_json,
        };
        match upsert_wyckoff_setup(pool, &upsert).await {
            Ok(id) => {
                n += 1;
                debug!(%id, symbol, timeframe, mode,
                       alt_type=%sig.candidate.setup_type.as_str(),
                       score=%sig.composite_score,
                       "wyckoff setup upserted");
            }
            Err(e) => warn!(%e, symbol, timeframe, mode, "wyckoff upsert failed"),
        }
    }
    Ok(n)
}

// =========================================================================
// Helpers — small, single-purpose (CLAUDE.md #1)
// =========================================================================

fn rebuild_tracker(row: &qtss_storage::WyckoffStructureRow) -> WyckoffStructureTracker {
    let schematic = schematic_from_str(&row.schematic);
    let mut t = WyckoffStructureTracker::new(
        schematic,
        row.range_top.unwrap_or(0.0),
        row.range_bottom.unwrap_or(0.0),
    );
    t.current_phase = phase_from_str(&row.current_phase);
    t.creek = row.creek_level;
    t.ice = row.ice_level;
    t.slope_deg = row.slope_deg.unwrap_or(0.0);
    t.events = parse_events(&row.events_json);
    t
}

fn schematic_from_str(s: &str) -> WyckoffSchematic {
    match s {
        "accumulation"   => WyckoffSchematic::Accumulation,
        "distribution"   => WyckoffSchematic::Distribution,
        "reaccumulation" => WyckoffSchematic::ReAccumulation,
        "redistribution" => WyckoffSchematic::ReDistribution,
        _                => WyckoffSchematic::Accumulation,
    }
}

fn phase_from_str(s: &str) -> WyckoffPhase {
    match s {
        "A" => WyckoffPhase::A,
        "B" => WyckoffPhase::B,
        "C" => WyckoffPhase::C,
        "D" => WyckoffPhase::D,
        "E" => WyckoffPhase::E,
        _   => WyckoffPhase::A,
    }
}

fn parse_events(v: &JsonValue) -> Vec<RecordedEvent> {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

/// Maps (exchange, segment) → `venue_class` string. Keeps the dispatch
/// localised so adding a new venue is one table entry (CLAUDE.md #1).
fn classify_venue(exchange: &str, segment: &str) -> String {
    match (exchange, segment) {
        ("binance", "futures")  => "binance_futures".to_string(),
        ("binance", "spot")     => "binance_spot".to_string(),
        (ex, seg)               => format!("{ex}_{seg}"),
    }
}

/// Wilder ATR with period `n`. Uses simple TR for first bar.
fn compute_atr(bars: &[SetupBar], n: usize) -> f64 {
    if bars.len() < 2 { return 0.0; }
    let mut trs: Vec<f64> = Vec::with_capacity(bars.len());
    for (i, b) in bars.iter().enumerate() {
        let tr = if i == 0 {
            b.high - b.low
        } else {
            let pc = bars[i - 1].close;
            (b.high - b.low)
                .max((b.high - pc).abs())
                .max((b.low - pc).abs())
        };
        trs.push(tr);
    }
    if trs.len() < n { return trs.iter().sum::<f64>() / trs.len() as f64; }
    // seed with SMA of first n TRs, then Wilder smoothing
    let seed: f64 = trs[..n].iter().sum::<f64>() / n as f64;
    let mut atr = seed;
    for tr in &trs[n..] {
        atr = (atr * (n as f64 - 1.0) + tr) / n as f64;
    }
    atr
}

/// Simple moving average of the last `n` volumes.
fn compute_sma_volume(bars: &[SetupBar], n: usize) -> f64 {
    if bars.is_empty() { return 0.0; }
    let take = n.min(bars.len());
    let tail = &bars[bars.len() - take..];
    tail.iter().map(|b| b.volume).sum::<f64>() / take as f64
}

// --- config loaders -----------------------------------------------------

async fn load_timeframes(pool: &PgPool) -> HashSet<String> {
    let s = resolve_system_string(
        pool,
        "setup",
        "wyckoff.scan.timeframes",
        "",
        r#"["1h","4h"]"#,
    )
    .await;
    parse_json_string_array(&s)
        .unwrap_or_else(|| vec!["1h".to_string(), "4h".to_string()])
        .into_iter()
        .collect()
}

async fn load_modes(pool: &PgPool) -> Vec<String> {
    let s = resolve_system_string(
        pool,
        "setup",
        "wyckoff.scan.modes",
        "",
        r#"["dry"]"#,
    )
    .await;
    parse_json_string_array(&s).unwrap_or_else(|| vec!["dry".to_string()])
}

/// Accept both `["1h","4h"]` JSON array and a plain `1h,4h` csv fallback.
fn parse_json_string_array(raw: &str) -> Option<Vec<String>> {
    let trimmed = raw.trim();
    if let Ok(arr) = serde_json::from_str::<Vec<String>>(trimmed) {
        if !arr.is_empty() { return Some(arr); }
    }
    let csv: Vec<String> = trimmed
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if csv.is_empty() { None } else { Some(csv) }
}

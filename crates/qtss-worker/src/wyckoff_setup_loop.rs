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
    resolve_commission_bps, resolve_system_csv, resolve_system_f64, resolve_system_string,
    resolve_system_u64, resolve_worker_enabled_flag, upsert_wyckoff_setup, CommissionSide,
    WyckoffSetupUpsert,
};
use qtss_wyckoff::{
    persistence::signal_to_payload,
    setup_builder::{SetupBar, SetupContext, WyckoffSetupConfig, WyckoffSetupType},
    signal_emitter::{default_profile_map, emit_with_vp, EmitterConfig},
    trade_planner::{TradePlannerConfig, VpTargetsHint},
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

    // P7.3 — Dual SL policy (tight/adaptive/structural dispatch).
    let sl_policy_raw = resolve_system_string(
        pool, "setup", "wyckoff.plan.sl_policy", "", "tighter",
    )
    .await;
    let use_vprofile_tp = resolve_worker_enabled_flag(
        pool, "setup", "wyckoff.plan.use_vprofile_tp", "", true,
    )
    .await;
    let mut planner_cfg = TradePlannerConfig::default();
    planner_cfg.sl_policy = qtss_wyckoff::trade_planner::SlPolicy::from_str(&sl_policy_raw);
    planner_cfg.use_vprofile_tp = use_vprofile_tp;

    let emit_cfg = EmitterConfig {
        min_score,
        profile_map: default_profile_map(),
        planner: planner_cfg,
        ..EmitterConfig::default()
    };
    // P7.2 — SOS/SOW confirmation gate for Phase-D continuation setups.
    let require_sos_sow = resolve_worker_enabled_flag(
        pool, "setup", "wyckoff.setup.require_sos_sow_trigger", "", true,
    )
    .await;
    let sos_sow_max_bars_ago = resolve_system_u64(
        pool, "setup", "wyckoff.setup.sos_sow_max_bars_ago", "", 50, 5, 500,
    )
    .await;
    let sos_sow_required_for_csv = resolve_system_csv(
        pool, "setup", "wyckoff.setup.sos_sow_required_for", "",
        "lps,buec,lpsy,ice_retest",
    )
    .await;
    let sos_sow_required_for: Vec<WyckoffSetupType> = sos_sow_required_for_csv
        .iter()
        .filter_map(|s| wyckoff_setup_type_from_str(s))
        .collect();

    // P7.3 — ATR buffer for the structural (wide) SL.
    let sl_wide_buffer_atr = resolve_system_f64(
        pool, "setup", "wyckoff.setup.sl_wide_buffer_atr", "", 0.5,
    )
    .await;

    // P7.5 — Per-setup entry filter rules.
    let require_directional_trigger = resolve_worker_enabled_flag(
        pool, "setup", "wyckoff.setup.require_directional_trigger", "", true,
    )
    .await;
    let trigger_bar_min_close_pos = resolve_system_f64(
        pool, "setup", "wyckoff.setup.trigger_bar_min_close_pos", "", 0.5,
    )
    .await;
    let jac_min_volume_ratio = resolve_system_f64(
        pool, "setup", "wyckoff.setup.jac_min_volume_ratio", "", 1.5,
    )
    .await;
    let jac_min_range_atr = resolve_system_f64(
        pool, "setup", "wyckoff.setup.jac_min_range_atr", "", 1.2,
    )
    .await;
    let jac_buffer_atr = resolve_system_f64(
        pool, "setup", "wyckoff.setup.jac_buffer_atr", "", 0.4,
    )
    .await;

    let mut setup_cfg = WyckoffSetupConfig::default();
    setup_cfg.sl_wide_buffer_atr = sl_wide_buffer_atr;
    setup_cfg.require_sos_sow_trigger = require_sos_sow;
    setup_cfg.sos_sow_max_bars_ago = sos_sow_max_bars_ago;
    if !sos_sow_required_for.is_empty() {
        setup_cfg.sos_sow_required_for = sos_sow_required_for;
    }
    setup_cfg.require_directional_trigger = require_directional_trigger;
    setup_cfg.trigger_bar_min_close_pos = trigger_bar_min_close_pos;
    setup_cfg.jac_min_volume_ratio = jac_min_volume_ratio;
    setup_cfg.jac_min_range_atr = jac_min_range_atr;
    setup_cfg.jac_buffer_atr = jac_buffer_atr;

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

    // Faz 8 step 1 — commission gate. Inject the venue-resolved
    // taker bps + `wyckoff.plan.min_net_rr` into a per-symbol clone
    // of the emit config. Without this the planner fell back to a
    // hardcoded 7.5 bps / 1.5 R default, silently diverging from the
    // D/T/Q loop which reads `setup.commission.*` (MEMORY gap list).
    let venue_class_for_commission = classify_venue(exchange, segment);
    let commission_bps = resolve_commission_bps(
        pool,
        &venue_class_for_commission,
        CommissionSide::Taker,
        emit_cfg.planner.commission_bps,
    )
    .await;
    let min_net_rr = resolve_system_f64(
        pool,
        "setup",
        "wyckoff.plan.min_net_rr",
        "",
        emit_cfg.planner.min_net_rr,
    )
    .await;
    let mut emit_cfg_owned = emit_cfg.clone();
    emit_cfg_owned.planner.commission_bps = commission_bps;
    emit_cfg_owned.planner.min_net_rr = min_net_rr;

    // 3. Emit — range id = structure uuid so rescans hit the same key.
    let range_id = structure.id.to_string();
    let vp_hint = build_vp_hint_from_setup_bars(&bars);
    let raw_signals = emit_with_vp(&ctx, &emit_cfg_owned, symbol, timeframe, &range_id, vp_hint.as_ref());
    if raw_signals.is_empty() {
        return Ok(0);
    }

    // 3b. Multi-TF phase harmony gate (Faz 10 P5). If an active HTF
    //     Wyckoff structure with an advanced phase (>=C) carries a bias
    //     opposite to the LTF setup's direction, veto the signal. HTF
    //     still in Phase A/B (or absent) means insufficient evidence —
    //     the LTF setup is allowed through.
    let htf_veto = resolve_htf_veto(pool, symbol, timeframe).await;
    let signals: Vec<_> = raw_signals
        .into_iter()
        .filter(|s| match htf_veto {
            Some(HtfVeto::BlockLong) => s.candidate.direction != qtss_wyckoff::setup_builder::SetupDirection::Long,
            Some(HtfVeto::BlockShort) => s.candidate.direction != qtss_wyckoff::setup_builder::SetupDirection::Short,
            None => true,
        })
        .collect();
    if signals.is_empty() {
        debug!(symbol, timeframe, ?htf_veto, "wyckoff setups vetoed by HTF gate");
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

// =========================================================================
// Multi-TF phase harmony — HTF gate (Faz 10 P5)
// =========================================================================
//
// Rule: if the configured HTF counterpart for this LTF has an active
// Wyckoff structure with phase >= C and a bias opposite to a candidate
// LTF setup, veto that setup. HTF in Phase A/B (range not yet
// confirmed) or missing altogether → no veto.
//
// Why phase C as the cutoff: phases A/B only establish the range and
// are directionally ambiguous. By Phase C the Spring/UTAD/Shakeout has
// declared the structure's true intent, and Phase D/E confirms it —
// those are the phases that should override LTF counter-trades.

#[derive(Debug, Clone, Copy)]
enum HtfVeto {
    BlockLong,
    BlockShort,
}

async fn resolve_htf_veto(pool: &PgPool, symbol: &str, ltf: &str) -> Option<HtfVeto> {
    let enabled = resolve_worker_enabled_flag(
        pool, "setup", "wyckoff.htf_gate.enabled", "", true,
    )
    .await;
    if !enabled { return None; }

    let htf = match htf_for(pool, ltf).await {
        Some(h) if h != ltf => h,
        _ => return None,
    };

    let row = match find_active_wyckoff_structure(pool, symbol, &htf).await.ok()? {
        Some(r) => r,
        None => return None,
    };
    let phase = phase_from_str(&row.current_phase);
    if phase < WyckoffPhase::C {
        return None; // HTF range not yet committed directionally
    }
    match row.schematic.as_str() {
        "distribution" | "redistribution" => Some(HtfVeto::BlockLong),
        "accumulation" | "reaccumulation" => Some(HtfVeto::BlockShort),
        _ => None,
    }
}

async fn htf_for(pool: &PgPool, ltf: &str) -> Option<String> {
    let raw = resolve_system_string(
        pool,
        "setup",
        "wyckoff.htf_gate.mapping",
        "",
        r#"{"15m":"1h","1h":"4h","4h":"1d","1d":"1w"}"#,
    )
    .await;
    let map: std::collections::HashMap<String, String> =
        serde_json::from_str(&raw).unwrap_or_default();
    map.get(ltf).cloned()
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

/// P7.4 — Build a Volume-Profile TP hint from the ASC-ordered SetupBar
/// window. Uses a 50-bin price histogram weighted by bar volume; HVNs
/// are midpoints whose volume exceeds 130% of the local 7-bin mean
/// (mirrors `qtss_vprofile::profile::derive` semantics but operates on
/// f64 so we avoid a Decimal round-trip for the worker hot path).
/// Returns `None` on degenerate input.
fn build_vp_hint_from_setup_bars(bars: &[SetupBar]) -> Option<VpTargetsHint> {
    const BINS: usize = 50;
    const HALF_WIN: usize = 3;
    const HVN_THR: f64 = 1.30;

    if bars.len() < 20 { return None; }
    let (mut lo, mut hi) = (bars[0].low, bars[0].high);
    for b in &bars[1..] {
        if b.low < lo { lo = b.low; }
        if b.high > hi { hi = b.high; }
    }
    if !(hi > lo) { return None; }
    let bin_size = (hi - lo) / BINS as f64;
    if !(bin_size > 0.0) { return None; }

    let mut vols = [0.0f64; BINS];
    for b in bars {
        if b.volume <= 0.0 || b.high <= b.low { continue; }
        let bar_range = b.high - b.low;
        let first = (((b.low - lo) / bin_size).floor() as isize).max(0) as usize;
        let last = (((b.high - lo) / bin_size).floor() as isize).min(BINS as isize - 1) as usize;
        for i in first..=last {
            let p_lo = lo + bin_size * i as f64;
            let p_hi = if i == BINS - 1 { hi } else { lo + bin_size * (i + 1) as f64 };
            let ov_lo = b.low.max(p_lo);
            let ov_hi = b.high.min(p_hi);
            if ov_hi <= ov_lo { continue; }
            vols[i] += (ov_hi - ov_lo) * b.volume / bar_range;
        }
    }

    // HVN detection: bin volume / local-window mean ≥ 1.30.
    let mut hvns: Vec<f64> = Vec::new();
    for i in 0..BINS {
        let lo_n = i.saturating_sub(HALF_WIN);
        let hi_n = (i + HALF_WIN).min(BINS - 1);
        if hi_n - lo_n < 2 { continue; }
        let mut sum = 0.0;
        let mut cnt = 0;
        for j in lo_n..=hi_n {
            if j == i { continue; }
            sum += vols[j];
            cnt += 1;
        }
        if cnt == 0 { continue; }
        let mean = sum / cnt as f64;
        if !(mean > 0.0) { continue; }
        if vols[i] / mean >= HVN_THR {
            let mid = lo + bin_size * (i as f64 + 0.5);
            hvns.push(mid);
        }
    }
    if hvns.is_empty() { return None; }
    Some(VpTargetsHint {
        hvns,
        naked_vpocs: Vec::new(),
        prior_swings: Vec::new(),
    })
}

/// Map a CSV token → `WyckoffSetupType`. Uses the canonical `as_str()`
/// suffix ("wyckoff_<name>") plus the short form users type in config.
fn wyckoff_setup_type_from_str(raw: &str) -> Option<WyckoffSetupType> {
    let t = raw.trim().to_lowercase();
    match t.as_str() {
        "spring" | "wyckoff_spring" => Some(WyckoffSetupType::Spring),
        "lps" | "wyckoff_lps" => Some(WyckoffSetupType::Lps),
        "buec" | "wyckoff_buec" => Some(WyckoffSetupType::Buec),
        "ut" | "wyckoff_ut" => Some(WyckoffSetupType::Ut),
        "utad" | "wyckoff_utad" => Some(WyckoffSetupType::Utad),
        "lpsy" | "wyckoff_lpsy" => Some(WyckoffSetupType::Lpsy),
        "ice_retest" | "ice" | "wyckoff_ice_retest" => Some(WyckoffSetupType::IceRetest),
        "jac" | "wyckoff_jac" | "jump_across_creek" => Some(WyckoffSetupType::Jac),
        _ => None,
    }
}

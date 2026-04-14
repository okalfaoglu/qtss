//! Wyckoff signal → storage payload mapper.
//!
//! Bridges `signal_emitter::WyckoffSignal` to primitive fields + JSONB blobs
//! that `qtss-storage::wyckoff_signals::upsert_wyckoff_setup` consumes. Kept
//! here (not in qtss-storage) so storage stays free of a qtss-wyckoff dep.
//!
//! Persisted column map:
//!   * `alt_type`         ← setup_type.as_str()        (e.g. "wyckoff_spring")
//!   * `direction`        ← "long" | "short"
//!   * `profile`          ← "d" | "q" | "t"
//!   * `entry_price`      ← plan.entry
//!   * `entry_sl`         ← plan.entry_sl              (policy-picked)
//!   * `target_ref`       ← last TP in ladder          (for legacy readers)
//!   * `tp_ladder`        ← JSON array of TpRung       (migration 0062)
//!   * `wyckoff_classic`  ← JSON L1 audit payload      (migration 0063)
//!   * `idempotency_key`  ← signal.idempotency_key     (migration 0064)
//!   * `raw_meta`         ← full plan + breakdown for deep audit
//!
//! CLAUDE.md: #1 no scattered if/else (two small dispatch helpers), #2 no
//! hardcoded business constants (caller still owns all config).

use serde_json::{json, Value as JsonValue};

use crate::setup_builder::SetupDirection;
use crate::signal_emitter::WyckoffSignal;
use crate::trade_planner::Profile;

/// Everything storage needs to materialise a `qtss_v2_setups` row for a
/// Wyckoff signal. Primitive fields only — no crate-local types — so that
/// `qtss-storage` can consume without pulling `qtss-wyckoff`.
#[derive(Debug, Clone)]
pub struct WyckoffSetupPayload {
    pub idempotency_key: String,
    pub venue_class: String,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub mode: String,          // "dry" | "live" | "backtest"
    pub profile: String,       // "d" | "q" | "t"
    pub alt_type: String,      // e.g. "wyckoff_spring"
    pub direction: String,     // "long" | "short"
    pub entry_price: f32,
    pub entry_sl: f32,
    pub target_ref: f32,       // last TP for legacy readers
    pub tp_ladder_json: JsonValue,
    pub wyckoff_classic_json: JsonValue,
    pub raw_meta_json: JsonValue,
    pub composite_score: f64,
}

/// Map a `WyckoffSignal` to a storage payload.
///
/// Arguments:
///   * `signal`      — emitter output (candidate + plan + score)
///   * `venue_class` — e.g. "binance_futures"
///   * `exchange`    — e.g. "binance"
///   * `symbol`      — upper-cased market symbol (BTCUSDT)
///   * `timeframe`   — "1h" | "4h"
///   * `mode`        — "dry" | "live" | "backtest"
pub fn signal_to_payload(
    signal: &WyckoffSignal,
    venue_class: &str,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
    mode: &str,
) -> WyckoffSetupPayload {
    let cand = &signal.candidate;
    let plan = &signal.plan;

    let tp_ladder_json = serde_json::to_value(&plan.tp_ladder).unwrap_or(json!([]));
    let wyckoff_classic_json = wyckoff_classic_payload(signal);
    let raw_meta_json = json!({
        "plan":            plan,
        "score_breakdown": signal.score_breakdown,
        "schematic":       cand.schematic,
        "phase":           cand.phase,
        "trigger_event":   cand.trigger_event,
        "trigger_bar_ts_ms": cand.trigger_bar_ts_ms,
        "atr_at_trigger":  cand.atr_at_trigger,
    });

    let target_ref = plan
        .tp_ladder
        .last()
        .map(|r| r.price)
        .unwrap_or(plan.entry) as f32;

    WyckoffSetupPayload {
        idempotency_key: signal.idempotency_key.clone(),
        venue_class: venue_class.to_string(),
        exchange: exchange.to_string(),
        symbol: symbol.to_string(),
        timeframe: timeframe.to_string(),
        mode: mode.to_string(),
        profile: plan.profile.as_str().to_string(),
        alt_type: cand.setup_type.as_str().to_string(),
        direction: direction_str(cand.direction).to_string(),
        entry_price: plan.entry as f32,
        entry_sl: plan.entry_sl as f32,
        target_ref,
        tp_ladder_json,
        wyckoff_classic_json,
        raw_meta_json,
        composite_score: signal.composite_score,
    }
}

/// Build the `wyckoff_classic` JSONB blob — L1 geometry audit per migration 0063.
fn wyckoff_classic_payload(signal: &WyckoffSignal) -> JsonValue {
    let c = &signal.candidate;
    json!({
        "setup_type":   c.setup_type.as_str(),
        "phase":        c.phase,
        "range_top":    c.range_top,
        "range_bottom": c.range_bottom,
        "range_height": c.range_height,
        "pnf_target":   c.pnf_target,
        "entry":        c.entry,
        "sl":           c.sl,
        "tp_targets":   c.tp_targets,
        "trigger_event":      c.trigger_event,
        "trigger_bar_index":  c.trigger_bar_index,
        "trigger_bar_ts_ms":  c.trigger_bar_ts_ms,
        "trigger_price":      c.trigger_price,
        "volume_ratio":  c.climax.volume_ratio,
        "spread_atr":    c.climax.spread_atr,
        "close_pos_pct": c.climax.close_pos_pct,
        "wick_pct":      c.climax.wick_pct,
    })
}

fn direction_str(d: SetupDirection) -> &'static str {
    match d { SetupDirection::Long => "long", SetupDirection::Short => "short" }
}

/// Convenience: parse a Wyckoff profile string back to `Profile`. Mirrors
/// the `as_str` table — kept close to the mapper so the two never drift.
pub fn profile_from_str(s: &str) -> Option<Profile> {
    match s { "d" => Some(Profile::D), "q" => Some(Profile::Q), "t" => Some(Profile::T), _ => None }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod test {
    use super::*;
    use crate::setup_builder::{SetupBar, WyckoffSetupConfig, SetupContext};
    use crate::signal_emitter::{emit, EmitterConfig};
    use crate::structure::{WyckoffEvent, WyckoffPhase, WyckoffSchematic, WyckoffStructureTracker};

    #[test]
    fn payload_has_all_required_fields() {
        let mut tr = WyckoffStructureTracker::new(
            WyckoffSchematic::Accumulation, 10_500.0, 9_500.0);
        tr.current_phase = WyckoffPhase::D;
        tr.record_event(WyckoffEvent::SC, 1, 9_400.0, 80.0);
        tr.record_event(WyckoffEvent::AR, 5, 10_400.0, 70.0);
        tr.record_event(WyckoffEvent::Spring, 30, 9_400.0, 90.0);
        tr.record_event(WyckoffEvent::LPS, 60, 9_700.0, 85.0);

        let bars: Vec<SetupBar> = (0..80).map(|i| SetupBar {
            ts_ms: i as i64 * 60_000,
            open: 10_000.0, high: 10_050.0, low: 9_950.0, close: 10_010.0,
            volume: 1_000.0,
        }).collect();
        let cfg_b = WyckoffSetupConfig::default();
        let ctx = SetupContext {
            tracker: &tr, bars: &bars, atr: 80.0, vol_avg_20: 1_000.0, cfg: &cfg_b,
        };
        let mut ec = EmitterConfig::default();
        ec.min_score = 0.0;
        let sigs = emit(&ctx, &ec, "BTCUSDT", "1h", "rng-1");
        assert!(!sigs.is_empty());

        let p = signal_to_payload(&sigs[0], "binance_futures", "binance",
                                  "BTCUSDT", "1h", "dry");
        assert_eq!(p.symbol, "BTCUSDT");
        assert_eq!(p.timeframe, "1h");
        assert_eq!(p.mode, "dry");
        assert!(p.alt_type.starts_with("wyckoff_"));
        assert!(p.direction == "long" || p.direction == "short");
        assert!(p.idempotency_key.contains("wy:BTCUSDT:1h:rng-1"));
        assert!(p.wyckoff_classic_json.get("range_top").is_some());
        assert!(p.tp_ladder_json.is_array());
    }
}

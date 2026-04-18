//! Signal Emitter — orchestrates `setup_builder` + `trade_planner` and
//! computes the final composite score that gates persistence.
//!
//! Pipeline:
//!   tracker + bars + cfg
//!     │
//!     ▼  setup_builder::build_all  (L1 candidates)
//!     │
//!     ▼  trade_planner::plan       (L1 + L2 → TradePlan)
//!     │
//!     ▼  composite_score(plan, candidate)  (final 0–100)
//!     │
//!     ▼  emit if score >= cfg.min_score AND plan.rejected.is_none()
//!
//! Composite score weighted breakdown:
//!   * 30 — raw_score (detector confidence)
//!   * 25 — climax quality (volume_ratio, spread_atr, close_pos)
//!   * 25 — trade-plan quality (net_rr, ladder length, no rejection)
//!   * 20 — phase quality (D > C > B; matches structure maturity)
//!
//! CLAUDE.md compliance:
//!   #1 — small dispatch helpers, no central match
//!   #2 — `EmitterConfig` carries every threshold (caller loads from config)

use serde::{Deserialize, Serialize};

use crate::setup_builder::{
    build_all, ClimaxMetrics, SetupContext, WyckoffSetupCandidate, WyckoffSetupType,
};
use crate::structure::WyckoffPhase;
use crate::trade_planner::{
    plan_with_vp, Profile, TradePlan, TradePlannerConfig, VpTargetsHint,
};

// =========================================================================
// Profile resolver — maps setup_type → D/Q (loaded from config by caller)
// =========================================================================

pub type ProfileMap = std::collections::HashMap<WyckoffSetupType, Profile>;

/// Default mapping mirroring `wyckoff.profile_map.*` config seed
/// (D = swing/decision-TF setups; Q = pullback/trigger-TF setups).
pub fn default_profile_map() -> ProfileMap {
    use Profile::*;
    use WyckoffSetupType::*;
    let mut m = ProfileMap::new();
    m.insert(Spring, D);
    m.insert(Ut, D);
    m.insert(Utad, D);
    m.insert(Lps, Q);
    m.insert(Buec, Q);
    m.insert(Lpsy, Q);
    m.insert(IceRetest, Q);
    m
}

// =========================================================================
// Emitter config
// =========================================================================

#[derive(Debug, Clone)]
pub struct EmitterConfig {
    pub min_score: f64,        // composite score gate (0..100)
    pub profile_map: ProfileMap,
    pub planner: TradePlannerConfig,
    pub w_raw: f64,            // weight: detector raw_score      (default 0.30)
    pub w_climax: f64,         // weight: climax quality          (default 0.25)
    pub w_plan: f64,           // weight: trade plan quality      (default 0.25)
    pub w_phase: f64,          // weight: phase maturity          (default 0.20)
}

impl Default for EmitterConfig {
    fn default() -> Self {
        Self {
            min_score: 60.0,
            profile_map: default_profile_map(),
            planner: TradePlannerConfig::default(),
            w_raw: 0.30,
            w_climax: 0.25,
            w_plan: 0.25,
            w_phase: 0.20,
        }
    }
}

// =========================================================================
// Emitted signal — the final object the worker upserts into qtss_setups
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WyckoffSignal {
    pub candidate: WyckoffSetupCandidate,
    pub plan: TradePlan,
    pub composite_score: f64,
    pub score_breakdown: ScoreBreakdown,
    pub idempotency_key: String,  // for upsert (deterministic)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub raw: f64,         // 0..100
    pub climax: f64,      // 0..100
    pub plan: f64,        // 0..100
    pub phase: f64,       // 0..100
    pub composite: f64,   // weighted sum
}

// =========================================================================
// Public API
// =========================================================================

/// Run all builders, plan, score, and return signals that pass `min_score`.
/// Returned signals are sorted by composite_score DESC.
pub fn emit(
    ctx: &SetupContext<'_>,
    cfg: &EmitterConfig,
    symbol: &str,
    timeframe: &str,
    range_id: &str,
) -> Vec<WyckoffSignal> {
    emit_with_vp(ctx, cfg, symbol, timeframe, range_id, None)
}

/// P7.4 — same as `emit` but routes Volume-Profile TP hints into the
/// planner. `vp_hint = None` falls back to pure adaptive R-multiple
/// TPs (legacy path).
pub fn emit_with_vp(
    ctx: &SetupContext<'_>,
    cfg: &EmitterConfig,
    symbol: &str,
    timeframe: &str,
    range_id: &str,
    vp_hint: Option<&VpTargetsHint>,
) -> Vec<WyckoffSignal> {
    let candidates = build_all(ctx);
    let mut out: Vec<WyckoffSignal> = candidates
        .into_iter()
        .filter_map(|cand| {
            let profile = *cfg.profile_map.get(&cand.setup_type)?;
            let plan = plan_with_vp(&cand, profile, &cfg.planner, vp_hint);
            if plan.rejected.is_some() { return None; }

            let breakdown = score(&cand, &plan, cfg);
            if breakdown.composite < cfg.min_score { return None; }

            let key = idempotency_key(symbol, timeframe, range_id, cand.setup_type, profile);
            Some(WyckoffSignal {
                candidate: cand,
                plan,
                composite_score: breakdown.composite,
                score_breakdown: breakdown,
                idempotency_key: key,
            })
        })
        .collect();
    out.sort_by(|a, b| b.composite_score.partial_cmp(&a.composite_score).unwrap_or(std::cmp::Ordering::Equal));
    out
}

// =========================================================================
// Composite scoring (small helpers, no central match — CLAUDE.md #1)
// =========================================================================

fn score(cand: &WyckoffSetupCandidate, plan: &TradePlan, cfg: &EmitterConfig) -> ScoreBreakdown {
    let raw = cand.raw_score.clamp(0.0, 100.0);
    let climax = climax_score(&cand.climax);
    let plan_q = plan_score(plan);
    let phase = phase_score(cand.phase);

    let composite = raw * cfg.w_raw
        + climax * cfg.w_climax
        + plan_q * cfg.w_plan
        + phase * cfg.w_phase;

    ScoreBreakdown { raw, climax, plan: plan_q, phase, composite }
}

/// 0..100 — high volume + wide spread + extreme close position.
fn climax_score(m: &ClimaxMetrics) -> f64 {
    // volume_ratio: 1.0×=neutral, 3.0×=excellent
    let vol = ((m.volume_ratio - 1.0) * 33.0).clamp(0.0, 100.0);
    // spread_atr: 0.5×=narrow, 2.0×=excellent
    let spread = ((m.spread_atr - 0.5) * 40.0).clamp(0.0, 100.0);
    // close_pos: extremes (near 0 or near 1) are best — symmetric U-curve
    let cp_dist = (m.close_pos_pct - 0.5).abs();      // 0..0.5
    let cp = (cp_dist * 200.0).clamp(0.0, 100.0);
    (vol + spread + cp) / 3.0
}

/// 0..100 — based on net RR and ladder shape.
fn plan_score(plan: &TradePlan) -> f64 {
    // net_rr: 1.0=floor, 3.0=excellent
    let rr = ((plan.net_rr_tp1 - 1.0) * 50.0).clamp(0.0, 100.0);
    // ladder length bonus: more rungs = more flexibility (cap at 4)
    let rungs = (plan.tp_ladder.len() as f64).min(4.0) * 25.0;
    (rr + rungs) / 2.0
}

/// 0..100 — D=100, C=70, earlier=30.
fn phase_score(p: WyckoffPhase) -> f64 {
    match p {
        WyckoffPhase::D => 100.0,
        WyckoffPhase::C => 70.0,
        WyckoffPhase::B => 30.0,
        WyckoffPhase::A | WyckoffPhase::E => 0.0,
    }
}

/// Deterministic key for upsert idempotency. Two emit() calls on the same
/// (symbol, tf, range, setup_type, profile) produce the same key — DB upsert
/// updates the existing row instead of inserting a duplicate.
fn idempotency_key(symbol: &str, tf: &str, range_id: &str, t: WyckoffSetupType, p: Profile) -> String {
    format!("wy:{symbol}:{tf}:{range_id}:{}:{}", t.as_str(), p.as_str())
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod test {
    use super::*;
    use crate::setup_builder::{SetupBar, WyckoffSetupConfig};
    use crate::structure::{WyckoffEvent, WyckoffSchematic, WyckoffStructureTracker};

    fn mk_bars(n: usize, base: f64) -> Vec<SetupBar> {
        (0..n).map(|i| SetupBar {
            ts_ms: i as i64 * 60_000,
            open: base, high: base + 50.0, low: base - 50.0, close: base + 10.0,
            volume: 1_000.0,
        }).collect()
    }

    #[test]
    fn idempotency_key_is_deterministic() {
        let k1 = idempotency_key("BTCUSDT", "1h", "abc", WyckoffSetupType::Spring, Profile::D);
        let k2 = idempotency_key("BTCUSDT", "1h", "abc", WyckoffSetupType::Spring, Profile::D);
        assert_eq!(k1, k2);
        assert!(k1.contains("wyckoff_spring"));
        assert!(k1.contains("BTCUSDT"));
    }

    #[test]
    fn emit_returns_signals_sorted_desc() {
        let mut tr = WyckoffStructureTracker::new(
            WyckoffSchematic::Accumulation, 10_500.0, 9_500.0);
        tr.current_phase = WyckoffPhase::D;
        tr.record_event(WyckoffEvent::SC, 1, 9_400.0, 80.0);
        tr.record_event(WyckoffEvent::AR, 5, 10_400.0, 70.0);
        tr.record_event(WyckoffEvent::Spring, 30, 9_400.0, 90.0);
        tr.record_event(WyckoffEvent::LPS, 60, 9_700.0, 85.0);

        let bars = mk_bars(80, 10_000.0);
        let cfg_b = WyckoffSetupConfig::default();
        let ctx = SetupContext { tracker: &tr, bars: &bars, atr: 80.0, vol_avg_20: 1_000.0, cfg: &cfg_b };

        let mut emit_cfg = EmitterConfig::default();
        emit_cfg.min_score = 0.0;        // drop gate so we see all
        let signals = emit(&ctx, &emit_cfg, "BTCUSDT", "1h", "rng-1");
        assert!(!signals.is_empty(), "should emit at least one signal");
        for w in signals.windows(2) {
            assert!(w[0].composite_score >= w[1].composite_score, "DESC sort");
        }
    }

    #[test]
    fn min_score_gate_filters_weak_signals() {
        let mut tr = WyckoffStructureTracker::new(
            WyckoffSchematic::Accumulation, 10_500.0, 9_500.0);
        tr.current_phase = WyckoffPhase::C;
        tr.record_event(WyckoffEvent::Spring, 30, 9_400.0, 5.0); // weak score

        let bars = mk_bars(40, 10_000.0);
        let cfg_b = WyckoffSetupConfig::default();
        let ctx = SetupContext { tracker: &tr, bars: &bars, atr: 80.0, vol_avg_20: 1_000.0, cfg: &cfg_b };

        let mut emit_cfg = EmitterConfig::default();
        emit_cfg.min_score = 90.0;       // very strict
        let signals = emit(&ctx, &emit_cfg, "BTCUSDT", "1h", "rng-1");
        assert!(signals.is_empty(), "weak signal should be filtered");
    }

    #[test]
    fn climax_score_rewards_extremes() {
        let s_neutral = climax_score(&ClimaxMetrics { volume_ratio: 1.0, spread_atr: 0.5, close_pos_pct: 0.5, wick_pct: 0.0 });
        let s_strong  = climax_score(&ClimaxMetrics { volume_ratio: 3.0, spread_atr: 2.0, close_pos_pct: 0.95, wick_pct: 0.2 });
        assert!(s_strong > s_neutral, "strong climax should score higher");
        assert!(s_strong >= 60.0);
    }
}

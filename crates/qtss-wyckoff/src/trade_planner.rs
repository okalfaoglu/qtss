//! Trade Planner — converts a `WyckoffSetupCandidate` (L1, classical) into
//! a final `TradePlan` (L1 + L2 adaptive Q-RADAR layer) ready for persistence.
//!
//! Layering (per user spec):
//!   L1 — classical Wyckoff levels (from `setup_builder`) — geometry-based
//!   L2 — adaptive Q-RADAR scaling (this module) — volatility/profile-based
//!
//! Combination rules:
//!   * `entry`     = L1.entry (immutable — pattern's trigger price)
//!   * `entry_sl`  = per `wyckoff.sl.policy` (default "tighter" → smallest
//!                   distance from entry → lower risk)
//!   * `tp_ladder` = L2 multipliers × R, **capped** by L1 P&F target when
//!                   `wyckoff.tp.classical_cap_enabled = true`
//!   * commission gate: reject when net RR(TP1) < `min_net_rr`
//!   * range cap: TP cannot exceed `range_height × range_cap_factor`
//!
//! CLAUDE.md compliance:
//!   #1 — no scattered if/else: bucket lookup tables + dispatch fns
//!   #2 — every constant comes from `TradePlannerConfig` (caller loads from
//!        system_config); defaults only used in tests

use serde::{Deserialize, Serialize};

use crate::setup_builder::{
    SetupDirection, WyckoffSetupCandidate, WyckoffSetupType,
};

// =========================================================================
// Profile (D / Q / T) — mirrors qtss-setup-engine but kept local to avoid
// cross-crate cycle. Caller maps `WyckoffSetupType → Profile` from config.
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile { D, Q, T }

impl Profile {
    pub fn as_str(self) -> &'static str {
        match self { Self::D => "d", Self::Q => "q", Self::T => "t" }
    }
}

// =========================================================================
// Adaptive TP bucket (loaded from `wyckoff.tp.adaptive.buckets` config)
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptiveBucket {
    pub atr_pct_max: f64,            // upper bound (inclusive) on ATR% for this bucket
    pub r_multipliers: Vec<f64>,     // each TP at entry +/- (R × multiplier)
    pub qty_split_pct: Vec<f64>,     // % of position to close at each TP (must sum to ~100)
    pub label: String,
}

// =========================================================================
// Config — caller fills from system_config (`wyckoff.tp.*`, `wyckoff.sl.*`)
// =========================================================================

#[derive(Debug, Clone)]
pub struct TradePlannerConfig {
    pub adaptive_buckets: Vec<AdaptiveBucket>, // sorted by atr_pct_max asc
    pub range_cap_factor: f64,
    pub classical_cap_enabled: bool,
    pub score_boost_threshold: f64,
    pub score_boost_r: f64,
    pub min_net_rr: f64,
    pub commission_bps: f64,                   // e.g. 7.5 → 0.075% per side (Binance taker)
    pub sl_policy: SlPolicy,
    pub d_entry_sl_atr_mult: f64,              // L2 SL distance for D profile
    pub q_entry_sl_atr_mult: f64,              // L2 SL distance for Q profile
    /// P17 — minimum risk as a fraction of entry price. Guards against
    /// near-zero ATR (Gemini review #3) producing microscopic `risk =
    /// |entry - sl|` that then makes `comm_r = 2*comm/risk` explode and
    /// TP ladder rungs collapse onto entry. Default 0.001 (= 0.1% of
    /// entry). Config key: `wyckoff.plan.min_risk_frac`.
    pub min_risk_frac: f64,
    /// Hard floor for TP/SL prices as a fraction of entry. Guards against
    /// the R-multiple projection (entry ± k*R) on wide-SL short setups
    /// pushing target prices to zero or below — physically impossible
    /// in any spot/perp market. See `docs/notes/bug_negative_target_price.md`
    /// (RAVEUSDT SHORT 1h: entry 1.18, SL 1.747, k=2.5 → TP1 = -0.234).
    /// Default 0.001 = 0.1% of entry. Config key:
    /// `wyckoff.plan.min_target_price_frac`.
    pub min_target_price_frac: f64,
    /// P7.4 — when true, overwrite TP1 with the nearest HVN / naked VPOC
    /// / prior-swing level if one exists between entry and the range
    /// cap. Villahermosa *Wyckoff 2.0* §7.4.3: "targets cluster at
    /// mean-volume reference points — trade to the next node, not to an
    /// R-multiple."
    pub use_vprofile_tp: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlPolicy {
    Tighter,        // min distance from entry (lower risk)
    Looser,         // max distance (more breathing room)
    ClassicalOnly,  // L1 only (tight)
    AdaptiveOnly,   // L2 only
    StructuralOnly, // P7.3 — candidate.sl_wide (range-boundary stop)
    TightestOfAll,  // P7.3 — min distance across tight/adaptive/structural
    WidestOfAll,    // P7.3 — max distance across tight/adaptive/structural
}

impl SlPolicy {
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "looser"          => Self::Looser,
            "classical_only"  => Self::ClassicalOnly,
            "adaptive_only"   => Self::AdaptiveOnly,
            "structural_only" => Self::StructuralOnly,
            "tightest_of_all" => Self::TightestOfAll,
            "widest_of_all"   => Self::WidestOfAll,
            _                 => Self::Tighter,
        }
    }
}

impl Default for TradePlannerConfig {
    fn default() -> Self {
        Self {
            adaptive_buckets: vec![
                AdaptiveBucket { atr_pct_max: 1.0,  r_multipliers: vec![0.8, 1.5, 2.5, 4.0], qty_split_pct: vec![25.0, 25.0, 25.0, 25.0], label: "low_vol".into() },
                AdaptiveBucket { atr_pct_max: 3.0,  r_multipliers: vec![1.0, 1.8, 3.0],      qty_split_pct: vec![33.0, 33.0, 34.0],       label: "mid_vol".into() },
                AdaptiveBucket { atr_pct_max: 99.0, r_multipliers: vec![1.2, 2.5],           qty_split_pct: vec![50.0, 50.0],             label: "high_vol".into() },
            ],
            // Literature-grade measured-move caps (Weis/Pruden).
            // range_cap_factor = 2.0 → max TP = breakout ± 2 × range_height
            // (canonical TP3 "extended" measured move). Was 1.5 — bumped
            // to match literature while the classical P&F cap still
            // provides the upper bound in low-TF ranges.
            range_cap_factor: 2.0,
            classical_cap_enabled: true,
            score_boost_threshold: 75.0,
            score_boost_r: 5.0,
            // Literature minimum R:R for Wyckoff entries is 1.5 (Pruden);
            // 1.0 was too permissive and let thin setups through the gate.
            min_net_rr: 1.5,
            commission_bps: 7.5,
            sl_policy: SlPolicy::Tighter,
            d_entry_sl_atr_mult: 2.5,
            q_entry_sl_atr_mult: 1.5,
            min_risk_frac: 0.001,
            min_target_price_frac: 0.001,
            use_vprofile_tp: true,
        }
    }
}

// =========================================================================
// Output: TradePlan — what the worker writes to qtss_setups
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TpRung {
    pub r: f64,           // R multiple (distance / risk)
    pub price: f64,
    pub qty_pct: f64,     // portion of position to close at this TP
    pub source: TpSource, // adaptive | classical | classical_capped
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TpSource { Adaptive, Classical, ClassicalCapped, VolumeProfile }

/// P7.4 — Volume-Profile TP hint passed into `plan_with_vp`. Plain
/// data so qtss-wyckoff stays free of a `qtss-vprofile` dep (CLAUDE.md
/// #4 — crate remains asset-class agnostic). The caller builds the
/// profile and fills these vectors; priority order is HVN → naked
/// VPOC → prior swing.
#[derive(Debug, Clone, Default)]
pub struct VpTargetsHint {
    pub hvns: Vec<f64>,
    pub naked_vpocs: Vec<f64>,
    pub prior_swings: Vec<f64>,
}

impl VpTargetsHint {
    /// First (priority-ordered, nearest-first within priority) target
    /// beyond `price` in the given direction (`up=true` = above).
    pub fn first_target_in_dir(&self, price: f64, up: bool) -> Option<f64> {
        let pick = |list: &[f64]| -> Option<f64> {
            list.iter().copied()
                .filter(|p| if up { *p > price } else { *p < price })
                .min_by(|a, b| (a - price).abs().partial_cmp(&(b - price).abs())
                    .unwrap_or(std::cmp::Ordering::Equal))
        };
        pick(&self.hvns)
            .or_else(|| pick(&self.naked_vpocs))
            .or_else(|| pick(&self.prior_swings))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradePlan {
    pub setup_type: WyckoffSetupType,
    pub direction: SetupDirection,
    pub profile: Profile,
    pub entry: f64,
    pub entry_sl: f64,             // final SL after policy
    pub classical_sl: f64,         // L1 tight SL for audit
    pub adaptive_sl: f64,          // L2 SL for audit
    pub structural_sl: f64,        // P7.3 — wide (range-boundary) SL for audit
    pub tp_ladder: Vec<TpRung>,
    pub net_rr_tp1: f64,           // weighted expected net R across full ladder (kept name for compat)
    pub bucket_label: String,
    pub atr_pct: f64,              // what selected the bucket
    pub commission_bps: f64,
    pub raw_score: f64,
    pub rejected: Option<RejectReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    BelowMinNetRr,
    NoBucket,
    InvalidGeometry,    // entry == sl, etc.
    EmptyTpLadder,
    /// All TPs (including TP1) projected below the price floor (e.g.
    /// short-side R-multiple ladder produced sub-zero or near-zero
    /// targets). Distinct from `EmptyTpLadder` so telemetry can detect
    /// chronically over-aggressive cause-effect / R multipliers per
    /// symbol+TF. See bug_negative_target_price.md.
    NegativeTargetProjection,
}

// =========================================================================
// Public API
// =========================================================================

/// Plan a trade from a candidate. `profile` is resolved by caller via
/// `wyckoff.profile_map.<setup>` config (D for spring/ut, Q for lps/buec/...).
pub fn plan(
    cand: &WyckoffSetupCandidate,
    profile: Profile,
    cfg: &TradePlannerConfig,
) -> TradePlan {
    plan_with_vp(cand, profile, cfg, None)
}

/// P7.4 — same as `plan` but allows overriding TP1 with a volume-profile
/// target (HVN / naked VPOC / prior swing). `None` hint = legacy path.
pub fn plan_with_vp(
    cand: &WyckoffSetupCandidate,
    profile: Profile,
    cfg: &TradePlannerConfig,
    vp_hint: Option<&VpTargetsHint>,
) -> TradePlan {
    let dir = cand.direction;

    // ── L2 adaptive SL ────────────────────────────────────────
    let atr = cand.atr_at_trigger.max(1e-9);
    let sl_mult = match profile {
        Profile::D => cfg.d_entry_sl_atr_mult,
        Profile::Q | Profile::T => cfg.q_entry_sl_atr_mult,
    };
    let adaptive_sl = cand.entry - dir.sign() * sl_mult * atr;
    let classical_sl = cand.sl;
    let structural_sl = cand.sl_wide;

    // ── SL policy dispatch (no scattered if/else — table) ─────
    let final_sl = pick_sl(
        cfg.sl_policy, cand.entry, classical_sl, adaptive_sl, structural_sl, dir,
    );

    // ── ATR% bucket selection ─────────────────────────────────
    let atr_pct = (atr / cand.entry.max(1e-9)) * 100.0;
    let bucket_opt = cfg.adaptive_buckets.iter().find(|b| atr_pct <= b.atr_pct_max);

    let Some(bucket) = bucket_opt else {
        return TradePlan {
            setup_type: cand.setup_type,
            direction: dir,
            profile,
            entry: cand.entry,
            entry_sl: final_sl,
            classical_sl, adaptive_sl, structural_sl,
            tp_ladder: vec![],
            net_rr_tp1: 0.0,
            bucket_label: "none".into(),
            atr_pct,
            commission_bps: cfg.commission_bps,
            raw_score: cand.raw_score,
            rejected: Some(RejectReason::NoBucket),
        };
    };

    let risk = (cand.entry - final_sl).abs();
    // P17 — risk floor: reject not just risk == 0 but risk < floor% of
    // entry. Zero-ATR / stablecoin / weekend low-vol instruments
    // otherwise produce absurd position sizes and infinite RR.
    let min_risk = cand.entry.abs() * cfg.min_risk_frac;
    if risk <= 0.0 || risk < min_risk {
        return reject(cand, profile, final_sl, classical_sl, adaptive_sl, structural_sl, atr_pct,
                      cfg.commission_bps, RejectReason::InvalidGeometry);
    }

    // ── Build adaptive TP ladder ──────────────────────────────
    let mut ladder: Vec<TpRung> = bucket.r_multipliers.iter().enumerate()
        .map(|(i, mult)| {
            let price = cand.entry + dir.sign() * mult * risk;
            let qty = bucket.qty_split_pct.get(i).copied().unwrap_or(0.0);
            TpRung { r: *mult, price, qty_pct: qty, source: TpSource::Adaptive }
        })
        .collect();

    // ── P7.4 — VP-priority TP1 override ───────────────────────
    // If a VP hint is supplied, replace TP1 with the nearest HVN /
    // naked VPOC / prior swing that lies beyond entry in direction.
    // Subsequent rungs are kept as adaptive ATR targets.
    if cfg.use_vprofile_tp {
        if let Some(hint) = vp_hint {
            let up = matches!(dir, SetupDirection::Long);
            if let Some(vp_price) = hint.first_target_in_dir(cand.entry, up) {
                if let Some(first) = ladder.first_mut() {
                    let r_new = ((vp_price - cand.entry) * dir.sign()) / risk;
                    if r_new > 0.0 {
                        first.price = vp_price;
                        first.r = r_new;
                        first.source = TpSource::VolumeProfile;
                    }
                }
            }
        }
    }

    // ── Score boost: append a runner if score >= threshold ────
    if cand.raw_score >= cfg.score_boost_threshold {
        let runner_price = cand.entry + dir.sign() * cfg.score_boost_r * risk;
        ladder.push(TpRung { r: cfg.score_boost_r, price: runner_price, qty_pct: 0.0,
                             source: TpSource::Adaptive });
    }

    // ── Range cap (range_height × range_cap_factor) ───────────
    let range_cap = match dir {
        SetupDirection::Long  => cand.range_top    + cand.range_height * cfg.range_cap_factor,
        SetupDirection::Short => cand.range_bottom - cand.range_height * cfg.range_cap_factor,
    };
    cap_ladder_to(&mut ladder, range_cap, dir);

    // ── Classical L1 cap (P&F target) ─────────────────────────
    // For short setups, pnf_target = range_bottom - cause_effect*range_h
    // can dive below zero on wide ranges — clamp to the price floor so
    // the cap itself never injects a sub-floor target.
    let price_floor = cand.entry.abs() * cfg.min_target_price_frac;
    let pnf_capped = match dir {
        SetupDirection::Long  => cand.pnf_target,
        SetupDirection::Short => cand.pnf_target.max(price_floor),
    };
    if cfg.classical_cap_enabled {
        cap_ladder_to(&mut ladder, pnf_capped, dir);
    }

    // ── Drop rungs that ended up on wrong side of entry ───────
    ladder.retain(|t| (t.price - cand.entry) * dir.sign() > 0.0);

    // ── Hard floor: physically valid prices only. R-multiple ──
    // projection on wide-SL shorts (entry - k*R) can push TP below
    // zero — the chart then renders e.g. "TP1 -0.234". See
    // bug_negative_target_price.md (RAVEUSDT 1h SHORT).
    let pre_floor_len = ladder.len();
    ladder.retain(|t| t.price >= price_floor);
    let dropped_to_floor = pre_floor_len > ladder.len();

    if ladder.is_empty() {
        let reason = if dropped_to_floor {
            RejectReason::NegativeTargetProjection
        } else {
            RejectReason::EmptyTpLadder
        };
        return reject(cand, profile, final_sl, classical_sl, adaptive_sl, structural_sl, atr_pct,
                      cfg.commission_bps, reason);
    }

    // ── Net RR gate (commission, weighted by qty_split) ───────
    // Gate on EXPECTED net R across the full ladder, not just TP1.
    // This handles scalp-style buckets (TP1=0.8R) correctly when later
    // rungs and the runner provide the bulk of expected reward.
    let comm_per_side = cand.entry * (cfg.commission_bps / 10_000.0);
    let total_qty: f64 = ladder.iter().map(|r| r.qty_pct).sum::<f64>().max(1.0);
    let weighted_gross_r: f64 = ladder.iter()
        .map(|r| r.r * (r.qty_pct / total_qty))
        .sum();
    let comm_r = (2.0 * comm_per_side) / risk;     // commission expressed in R
    let net_rr = weighted_gross_r - comm_r;        // weighted net R after commission
    let net_rr_tp1 = net_rr;                       // field name kept for backward compat
    let rejected = (net_rr < cfg.min_net_rr).then_some(RejectReason::BelowMinNetRr);

    TradePlan {
        setup_type: cand.setup_type,
        direction: dir,
        profile,
        entry: cand.entry,
        entry_sl: final_sl,
        classical_sl, adaptive_sl, structural_sl,
        tp_ladder: ladder,
        net_rr_tp1,
        bucket_label: bucket.label.clone(),
        atr_pct,
        commission_bps: cfg.commission_bps,
        raw_score: cand.raw_score,
        rejected,
    }
}

// =========================================================================
// Internal helpers (small, single-purpose — CLAUDE.md #1)
// =========================================================================

fn pick_sl(
    policy: SlPolicy,
    entry: f64,
    classical: f64,
    adaptive: f64,
    structural: f64,
    dir: SetupDirection,
) -> f64 {
    let dc = (entry - classical).abs();
    let da = (entry - adaptive).abs();
    let ds = (entry - structural).abs();
    let pick_by = |cmp: fn(f64, f64) -> bool| {
        // (dist, price) tuples scanned by cmp — cmp(a,b)==true means a wins.
        let mut best = (dc, classical);
        if cmp(da, best.0) { best = (da, adaptive); }
        if cmp(ds, best.0) { best = (ds, structural); }
        best.1
    };
    match policy {
        SlPolicy::ClassicalOnly  => classical,
        SlPolicy::AdaptiveOnly   => adaptive,
        SlPolicy::StructuralOnly => structural,
        SlPolicy::Tighter        => if dc < da { classical } else { adaptive },
        SlPolicy::Looser         => if dc > da { classical } else { adaptive },
        SlPolicy::TightestOfAll  => pick_by(|a, b| a < b),
        SlPolicy::WidestOfAll    => pick_by(|a, b| a > b),
    }.min_max_clamp(entry, dir)  // ensure SL stays on the correct side of entry
}

fn cap_ladder_to(ladder: &mut [TpRung], cap: f64, dir: SetupDirection) {
    for rung in ladder.iter_mut() {
        let beyond = match dir {
            SetupDirection::Long  => rung.price > cap,
            SetupDirection::Short => rung.price < cap,
        };
        if beyond {
            rung.price = cap;
            rung.source = TpSource::ClassicalCapped;
        }
    }
}

fn reject(
    cand: &WyckoffSetupCandidate,
    profile: Profile,
    sl: f64, csl: f64, asl: f64, ssl: f64,
    atr_pct: f64, commission_bps: f64,
    reason: RejectReason,
) -> TradePlan {
    TradePlan {
        setup_type: cand.setup_type,
        direction: cand.direction,
        profile,
        entry: cand.entry,
        entry_sl: sl,
        classical_sl: csl,
        adaptive_sl: asl,
        structural_sl: ssl,
        tp_ladder: vec![],
        net_rr_tp1: 0.0,
        bucket_label: "rejected".into(),
        atr_pct,
        commission_bps,
        raw_score: cand.raw_score,
        rejected: Some(reason),
    }
}

// Tiny extension trait so we can keep pick_sl one-liner-ish.
trait MinMaxClamp { fn min_max_clamp(self, entry: f64, dir: SetupDirection) -> f64; }
impl MinMaxClamp for f64 {
    fn min_max_clamp(self, entry: f64, dir: SetupDirection) -> f64 {
        match dir {
            SetupDirection::Long  => self.min(entry - 1e-9),
            SetupDirection::Short => self.max(entry + 1e-9),
        }
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod test {
    use super::*;
    use crate::setup_builder::{ClimaxMetrics, WyckoffSetupCandidate, WyckoffSetupType};
    use crate::structure::{WyckoffEvent, WyckoffPhase, WyckoffSchematic};

    fn long_cand(entry: f64, sl: f64, atr: f64, score: f64) -> WyckoffSetupCandidate {
        // Use proportional levels so helper works at any price scale
        let risk = (entry - sl).abs();
        let h = risk * 5.0;
        WyckoffSetupCandidate {
            setup_type: WyckoffSetupType::Spring,
            direction: SetupDirection::Long,
            phase: WyckoffPhase::C,
            schematic: WyckoffSchematic::Accumulation,
            range_top: entry + h * 0.5,
            range_bottom: entry - h * 0.5,
            range_height: h,
            pnf_target: entry + h * 1.5,
            entry, sl,
            sl_wide: sl - atr * 0.5,
            tp_targets: vec![entry + h * 0.5, entry + h * 1.5],
            trigger_event: WyckoffEvent::Spring,
            trigger_bar_index: 0, trigger_bar_ts_ms: 0, trigger_price: entry,
            climax: ClimaxMetrics::default(),
            atr_at_trigger: atr,
            raw_score: score,
        }
    }

    #[test]
    fn long_plan_produces_rising_ladder() {
        // Use realistic entry (BTC-like) so commission % is negligible
        let cand = long_cand(10_000.0, 9_900.0, 75.0, 70.0);
        let plan = plan(&cand, Profile::Q, &TradePlannerConfig::default());
        assert!(plan.rejected.is_none(), "got reject: {:?}", plan.rejected);
        assert!(!plan.tp_ladder.is_empty());
        for w in plan.tp_ladder.windows(2) {
            assert!(w[1].price >= w[0].price, "ladder must be monotonic for long");
        }
        assert!(plan.entry_sl < plan.entry);
    }

    #[test]
    fn classical_cap_clips_runaway_tp() {
        let mut cand = long_cand(100.0, 99.0, 0.5, 80.0); // very tight risk
        cand.pnf_target = 102.0;                          // tight cap (forces clipping)
        let mut cfg = TradePlannerConfig::default();
        cfg.classical_cap_enabled = true;
        cfg.range_cap_factor = 99.0;                      // disable range cap
        let plan = plan(&cand, Profile::D, &cfg);
        for r in &plan.tp_ladder {
            assert!(r.price <= 105.0 + 1e-6, "no TP above pnf cap");
        }
        assert!(plan.tp_ladder.iter().any(|r| r.source == TpSource::ClassicalCapped));
    }

    #[test]
    fn min_net_rr_rejects_thin_setup() {
        // huge risk, tiny TP1 → fail RR gate
        let cand = long_cand(100.0, 90.0, 5.0, 60.0);
        let mut cfg = TradePlannerConfig::default();
        cfg.min_net_rr = 5.0;        // very strict
        let plan = plan(&cand, Profile::D, &cfg);
        assert_eq!(plan.rejected, Some(RejectReason::BelowMinNetRr));
    }

    #[test]
    fn short_plan_descends() {
        let mut cand = long_cand(10_000.0, 10_100.0, 75.0, 70.0);
        cand.direction = SetupDirection::Short;
        cand.setup_type = WyckoffSetupType::Lpsy;
        cand.pnf_target = 8_500.0;
        cand.range_top = 10_500.0;
        cand.range_bottom = 9_500.0;
        cand.range_height = 1_000.0;
        let plan = plan(&cand, Profile::Q, &TradePlannerConfig::default());
        assert!(plan.rejected.is_none());
        for w in plan.tp_ladder.windows(2) {
            assert!(w[1].price <= w[0].price, "short ladder descends");
        }
        assert!(plan.entry_sl > plan.entry);
    }

    #[test]
    fn raveusdt_short_negative_tp_is_rejected_or_clamped() {
        // Repro of bug_negative_target_price.md — RAVEUSDT 1h SHORT
        // Q-profile setup: entry 1.18133, SL 1.747431 → R≈0.566.
        // Adaptive bucket TP3 = entry - 4*R = -1.083, even TP1 = -0.235.
        // Before the fix the chart rendered TP1 = -0.234. After the fix:
        // every rung must be > 0, otherwise the setup is rejected with
        // NegativeTargetProjection.
        let mut cand = long_cand(1.18133, 0.61527, 0.05, 60.0); // long shape first
        cand.direction = SetupDirection::Short;
        cand.setup_type = WyckoffSetupType::Lpsy;
        cand.entry = 1.18133;
        cand.sl = 1.747431;       // SL above entry for short → risk = 0.566
        cand.sl_wide = 1.80;
        cand.range_top = 1.80;
        cand.range_bottom = 1.0;
        cand.range_height = 0.8;
        cand.pnf_target = 1.0 - 0.8 * 1.5; // -0.2 — replicates the leak

        let plan = plan(&cand, Profile::Q, &TradePlannerConfig::default());
        // Either: every surviving rung is strictly positive, OR the
        // setup was rejected because the floor wiped them all.
        for r in &plan.tp_ladder {
            assert!(r.price > 0.0, "TP rung leaked sub-zero price: {:?}", r);
        }
        if plan.tp_ladder.is_empty() {
            assert_eq!(plan.rejected, Some(RejectReason::NegativeTargetProjection));
        }
    }

    #[test]
    fn score_boost_appends_runner() {
        let cand = long_cand(100.0, 98.0, 1.5, 90.0); // high score
        let cfg = TradePlannerConfig::default();
        let plan = plan(&cand, Profile::Q, &cfg);
        // Boost r=5.0; with risk=2 → runner price=110, but range_cap=10*1.5+105=120 → ok,
        // but pnf_target=115 caps it (classical_cap_enabled=true)
        let max_r = plan.tp_ladder.iter().map(|r| r.r).fold(0.0_f64, f64::max);
        assert!(max_r >= 5.0, "score boost runner present");
    }
}

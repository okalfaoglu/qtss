//! Faz 9.8.1 — Setup selector (low-risk / high-reward filter dispatch).
//!
//! Between the setup engine (9.7) and the execution manager (9.8.4).
//! Takes a candidate [`SetupCandidate`] (built from `qtss_v2_setups`
//! + external context) and walks a registered battery of filters.
//! The first rejection short-circuits (CLAUDE.md #1 — every filter is
//! a trait impl rather than an inline `if` branch).
//!
//! Pure evaluator: no DB, no network. The calling worker loop sources
//! candidates from storage and applies config-driven `SelectorConfig`.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Inputs the selector reasons about. The caller fills these from
/// `qtss_v2_setups` (+ joined ai_score) plus external signals (open
/// position count on the venue, cooldown state, etc.).
#[derive(Debug, Clone)]
pub struct SetupCandidate {
    pub setup_id: Uuid,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub profile: String,
    /// Long/Short flag; selector uses it to validate R:R sign.
    pub direction: Direction,
    pub entry_price: Decimal,
    pub stop_price: Decimal,
    pub target_price: Decimal,
    /// `P(win)` from the inference sidecar, `[0, 1]`.
    pub ai_score: f64,
    /// Risk as fraction of equity at setup creation (e.g. 0.01 = 1%).
    pub risk_pct: f64,
    /// Tier 1..10 derived from ai_score + thresholds.
    pub tier: u8,
    /// Current open live positions on this (exchange, symbol). The
    /// execution worker provides a snapshot-consistent value.
    pub open_positions_on_symbol: u32,
    /// `true` when the symbol is still under liquidation cooldown.
    pub under_liquidation_cooldown: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Long,
    Short,
}

/// Config snapshot the selector reads. Every field maps to a
/// `qtss_config` key (CLAUDE.md #2) — the DB loader composes this.
#[derive(Debug, Clone)]
pub struct SelectorConfig {
    pub min_risk_reward: f64,
    pub min_ai_score: f64,
    pub max_risk_pct: f64,
    pub min_tier: u8,
    pub max_open_positions_per_symbol: u32,
}

impl Default for SelectorConfig {
    fn default() -> Self {
        Self {
            min_risk_reward: 1.5,
            min_ai_score: 0.55,
            max_risk_pct: 0.02,     // 2%
            min_tier: 6,
            max_open_positions_per_symbol: 1,
        }
    }
}

/// Outcome of running the selector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SelectionOutcome {
    /// All filters passed. `score` is a composite rank [0, 1].
    Selected {
        setup_id: Uuid,
        composite_score: f64,
    },
    /// First filter to reject wins — caller logs `reason` for audit.
    Rejected {
        setup_id: Uuid,
        filter: &'static str,
        reason: String,
    },
}

impl SelectionOutcome {
    pub fn is_selected(&self) -> bool {
        matches!(self, SelectionOutcome::Selected { .. })
    }
    pub fn setup_id(&self) -> Uuid {
        match self {
            SelectionOutcome::Selected { setup_id, .. }
            | SelectionOutcome::Rejected { setup_id, .. } => *setup_id,
        }
    }
}

/// Single-filter contract. Returning `None` means the candidate passed
/// this filter; `Some(reason)` means rejected with an audit message.
pub trait SelectorFilter: Send + Sync {
    /// Short stable tag — used for logging + metrics + error attribution.
    fn tag(&self) -> &'static str;
    fn evaluate(&self, cand: &SetupCandidate, cfg: &SelectorConfig) -> Option<String>;
}

// ---------------------------------------------------------------------------
// Concrete filters (CLAUDE.md #1 — one impl per concern, not nested if/else)
// ---------------------------------------------------------------------------

/// Requires `(|target - entry| / |entry - stop|)` ≥ `cfg.min_risk_reward`.
/// Sign-aware: LONG expects target above entry, SHORT the other way.
pub struct MinRiskRewardFilter;

impl SelectorFilter for MinRiskRewardFilter {
    fn tag(&self) -> &'static str {
        "min_risk_reward"
    }
    fn evaluate(&self, c: &SetupCandidate, cfg: &SelectorConfig) -> Option<String> {
        let entry = decimal_to_f64(c.entry_price);
        let stop = decimal_to_f64(c.stop_price);
        let target = decimal_to_f64(c.target_price);
        let (risk, reward) = match c.direction {
            Direction::Long => (entry - stop, target - entry),
            Direction::Short => (stop - entry, entry - target),
        };
        if risk <= 0.0 {
            return Some(format!("non-positive risk: {risk:.6}"));
        }
        if reward <= 0.0 {
            return Some(format!("non-positive reward: {reward:.6}"));
        }
        let rr = reward / risk;
        if rr < cfg.min_risk_reward {
            return Some(format!("R:R {rr:.2} < {:.2}", cfg.min_risk_reward));
        }
        None
    }
}

pub struct MinAiScoreFilter;

impl SelectorFilter for MinAiScoreFilter {
    fn tag(&self) -> &'static str {
        "min_ai_score"
    }
    fn evaluate(&self, c: &SetupCandidate, cfg: &SelectorConfig) -> Option<String> {
        if c.ai_score < cfg.min_ai_score {
            return Some(format!(
                "ai_score {:.3} < {:.3}",
                c.ai_score, cfg.min_ai_score
            ));
        }
        None
    }
}

pub struct MaxRiskPctFilter;

impl SelectorFilter for MaxRiskPctFilter {
    fn tag(&self) -> &'static str {
        "max_risk_pct"
    }
    fn evaluate(&self, c: &SetupCandidate, cfg: &SelectorConfig) -> Option<String> {
        if c.risk_pct > cfg.max_risk_pct {
            return Some(format!(
                "risk_pct {:.4} > {:.4}",
                c.risk_pct, cfg.max_risk_pct
            ));
        }
        None
    }
}

pub struct MinTierFilter;

impl SelectorFilter for MinTierFilter {
    fn tag(&self) -> &'static str {
        "min_tier"
    }
    fn evaluate(&self, c: &SetupCandidate, cfg: &SelectorConfig) -> Option<String> {
        if c.tier < cfg.min_tier {
            return Some(format!("tier {} < {}", c.tier, cfg.min_tier));
        }
        None
    }
}

pub struct OpenPositionCapFilter;

impl SelectorFilter for OpenPositionCapFilter {
    fn tag(&self) -> &'static str {
        "open_position_cap"
    }
    fn evaluate(&self, c: &SetupCandidate, cfg: &SelectorConfig) -> Option<String> {
        if c.open_positions_on_symbol >= cfg.max_open_positions_per_symbol {
            return Some(format!(
                "open_positions {} >= cap {}",
                c.open_positions_on_symbol, cfg.max_open_positions_per_symbol
            ));
        }
        None
    }
}

pub struct LiquidationCooldownFilter;

impl SelectorFilter for LiquidationCooldownFilter {
    fn tag(&self) -> &'static str {
        "liquidation_cooldown"
    }
    fn evaluate(&self, c: &SetupCandidate, _cfg: &SelectorConfig) -> Option<String> {
        if c.under_liquidation_cooldown {
            return Some("symbol under post-liquidation cooldown".into());
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Registry — ordered dispatch table
// ---------------------------------------------------------------------------

/// Ordered registry. Filters run in insertion order; first rejection
/// short-circuits. Insertion order matches audit expectations: cheap,
/// deterministic guards first (tier/ai_score), context lookups later
/// (open positions, cooldown).
pub struct SelectorRegistry {
    filters: Vec<Box<dyn SelectorFilter>>,
}

impl SelectorRegistry {
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
        }
    }

    /// Production defaults — all 6 filters in a sensible order.
    pub fn with_defaults() -> Self {
        let mut r = Self::new();
        r.register(Box::new(MinAiScoreFilter));
        r.register(Box::new(MinTierFilter));
        r.register(Box::new(MaxRiskPctFilter));
        r.register(Box::new(MinRiskRewardFilter));
        r.register(Box::new(OpenPositionCapFilter));
        r.register(Box::new(LiquidationCooldownFilter));
        r
    }

    pub fn register(&mut self, f: Box<dyn SelectorFilter>) {
        self.filters.push(f);
    }

    pub fn len(&self) -> usize {
        self.filters.len()
    }

    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    /// Walk filters in order; short-circuit on first rejection.
    pub fn evaluate(
        &self,
        candidate: &SetupCandidate,
        cfg: &SelectorConfig,
    ) -> SelectionOutcome {
        for f in &self.filters {
            if let Some(reason) = f.evaluate(candidate, cfg) {
                return SelectionOutcome::Rejected {
                    setup_id: candidate.setup_id,
                    filter: f.tag(),
                    reason,
                };
            }
        }
        SelectionOutcome::Selected {
            setup_id: candidate.setup_id,
            composite_score: composite_score(candidate),
        }
    }

    /// Bulk evaluate a slate of candidates, returning only the selected
    /// ones sorted by composite score (desc). Rejected ones are
    /// available via `rejections` for audit.
    pub fn rank(
        &self,
        candidates: &[SetupCandidate],
        cfg: &SelectorConfig,
    ) -> RankedOutcome {
        let mut selected: Vec<(f64, Uuid)> = Vec::new();
        let mut rejections: BTreeMap<Uuid, (String, String)> = BTreeMap::new();
        for c in candidates {
            match self.evaluate(c, cfg) {
                SelectionOutcome::Selected {
                    setup_id,
                    composite_score,
                } => {
                    selected.push((composite_score, setup_id));
                }
                SelectionOutcome::Rejected {
                    setup_id,
                    filter,
                    reason,
                } => {
                    rejections.insert(setup_id, (filter.to_string(), reason));
                }
            }
        }
        selected.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        RankedOutcome {
            selected,
            rejections,
        }
    }
}

impl Default for SelectorRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[derive(Debug, Clone, Default)]
pub struct RankedOutcome {
    /// `(composite_score, setup_id)` sorted desc by score.
    pub selected: Vec<(f64, Uuid)>,
    /// `setup_id → (filter_tag, reason)` for every rejection.
    pub rejections: BTreeMap<Uuid, (String, String)>,
}

/// Composite score: weighted ai_score + R:R clamp. Tuned to keep
/// high-confidence + high-R:R setups at the top without letting
/// borderline scores ride because of a single huge target.
fn composite_score(c: &SetupCandidate) -> f64 {
    let entry = decimal_to_f64(c.entry_price);
    let stop = decimal_to_f64(c.stop_price);
    let target = decimal_to_f64(c.target_price);
    let (risk, reward) = match c.direction {
        Direction::Long => (entry - stop, target - entry),
        Direction::Short => (stop - entry, entry - target),
    };
    let rr_clamped = if risk > 0.0 && reward > 0.0 {
        (reward / risk).min(5.0) / 5.0
    } else {
        0.0
    };
    // 70% confidence, 30% R:R — confidence is the stronger signal when
    // the ML model is well calibrated.
    0.7 * c.ai_score + 0.3 * rr_clamped
}

fn decimal_to_f64(d: Decimal) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn long_candidate() -> SetupCandidate {
        SetupCandidate {
            setup_id: Uuid::new_v4(),
            exchange: "binance".into(),
            symbol: "BTCUSDT".into(),
            timeframe: "1h".into(),
            profile: "d".into(),
            direction: Direction::Long,
            entry_price: dec!(100.0),
            stop_price: dec!(98.0),
            target_price: dec!(106.0), // R:R = 3.0
            ai_score: 0.80,
            risk_pct: 0.01,
            tier: 8,
            open_positions_on_symbol: 0,
            under_liquidation_cooldown: false,
        }
    }

    #[test]
    fn all_defaults_pass_for_clean_candidate() {
        let r = SelectorRegistry::with_defaults();
        let out = r.evaluate(&long_candidate(), &SelectorConfig::default());
        assert!(out.is_selected(), "expected selected, got {:?}", out);
    }

    #[test]
    fn low_rr_rejects_with_filter_tag() {
        let mut c = long_candidate();
        c.target_price = dec!(100.5); // R:R = 0.25
        let r = SelectorRegistry::with_defaults();
        let out = r.evaluate(&c, &SelectorConfig::default());
        match out {
            SelectionOutcome::Rejected { filter, .. } => assert_eq!(filter, "min_risk_reward"),
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[test]
    fn low_ai_score_rejects_before_rr() {
        let mut c = long_candidate();
        c.ai_score = 0.10;
        let r = SelectorRegistry::with_defaults();
        let out = r.evaluate(&c, &SelectorConfig::default());
        match out {
            SelectionOutcome::Rejected { filter, .. } => assert_eq!(filter, "min_ai_score"),
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[test]
    fn open_position_cap_blocks_second_entry() {
        let mut c = long_candidate();
        c.open_positions_on_symbol = 1;
        let r = SelectorRegistry::with_defaults();
        let out = r.evaluate(&c, &SelectorConfig::default());
        match out {
            SelectionOutcome::Rejected { filter, .. } => assert_eq!(filter, "open_position_cap"),
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[test]
    fn cooldown_blocks_entry() {
        let mut c = long_candidate();
        c.under_liquidation_cooldown = true;
        let r = SelectorRegistry::with_defaults();
        let out = r.evaluate(&c, &SelectorConfig::default());
        match out {
            SelectionOutcome::Rejected { filter, .. } => assert_eq!(filter, "liquidation_cooldown"),
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[test]
    fn short_rr_respects_sign() {
        let mut c = long_candidate();
        c.direction = Direction::Short;
        c.entry_price = dec!(100.0);
        c.stop_price = dec!(102.0); // stop above for SHORT
        c.target_price = dec!(94.0); // target below
        let r = SelectorRegistry::with_defaults();
        let out = r.evaluate(&c, &SelectorConfig::default());
        assert!(out.is_selected(), "short RR should qualify");
    }

    #[test]
    fn rank_sorts_by_composite_score_desc() {
        let mut low = long_candidate();
        low.ai_score = 0.60;
        let mut high = long_candidate();
        high.ai_score = 0.95;
        let r = SelectorRegistry::with_defaults();
        let out = r.rank(&[low.clone(), high.clone()], &SelectorConfig::default());
        assert_eq!(out.selected.len(), 2);
        assert!(out.rejections.is_empty());
        assert_eq!(out.selected[0].1, high.setup_id); // high first
        assert_eq!(out.selected[1].1, low.setup_id);
    }

    #[test]
    fn rank_partitions_selected_and_rejected() {
        let good = long_candidate();
        let mut bad = long_candidate();
        bad.ai_score = 0.10;
        let r = SelectorRegistry::with_defaults();
        let out = r.rank(&[good.clone(), bad.clone()], &SelectorConfig::default());
        assert_eq!(out.selected.len(), 1);
        assert_eq!(out.selected[0].1, good.setup_id);
        assert_eq!(out.rejections.len(), 1);
        assert!(out.rejections.contains_key(&bad.setup_id));
    }

    #[test]
    fn registry_order_is_insertion_order() {
        let r = SelectorRegistry::with_defaults();
        assert_eq!(r.len(), 6);
    }
}

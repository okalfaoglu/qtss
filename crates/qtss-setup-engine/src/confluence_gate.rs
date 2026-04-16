//! Faz 9.1 — Classic Confluence Gate.
//!
//! Katman 1 (Veto) + Katman 2 (Yön konsensüsü) + Katman 3 (Ağırlıklı skor)
//! kararlarını **tek noktada** toplayan deterministik bir kapı.
//! AI meta-model devreye girmeden önce "klasik baseline"; AI shadow
//! aşamasında karşılaştırma referansı olarak da kullanılır.
//!
//! Pipeline:
//!
//! ```text
//!   VetoRule[*].evaluate(ctx)  ── herhangi biri Veto → reject
//!          │
//!   Direction Consensus (Elliott + Wyckoff + Classical 2/3)
//!          │
//!   Weighted Score (Faz 7.8 score_confluence) ≥ min_score
//!          │
//!   → Approve(final_score, direction, rationale)
//! ```
//!
//! CLAUDE.md #1: Dispatch — yeni veto kuralı = yeni `VetoRule` impl'i
//! + registry'ye satır ekleme. Eşikler config'ten gelir (CLAUDE.md #2).
//! CLAUDE.md #4: crate venue bilmez; ctx zaten normalize `ConfluenceInputs`
//! + sade alanlar (regime, kill_switch flag, vs.) taşır.

use qtss_confluence::{
    score_confluence, ConfluenceDirection, ConfluenceInputs, ConfluenceReading, ConfluenceWeights,
    DetectionVote,
};
use serde::{Deserialize, Serialize};

/// External context: worker tarafı doldurur.
#[derive(Debug, Clone, Default)]
pub struct GateContext {
    pub inputs: ConfluenceInputs,
    pub regime_label: Option<String>,
    /// If true, any open request is rejected (kill_switch veto).
    pub kill_switch_on: bool,
    /// Kaynaklar temiz mi? (stale data detection — veto).
    pub stale_data: bool,
    /// News blackout window active?
    pub news_blackout: bool,
}

/// Ağırlıklı skor + kabul/red eşikleri.
#[derive(Debug, Clone)]
pub struct GateConfig {
    pub weights: ConfluenceWeights,
    /// `final_score` >= this → approve. Usually `[0.55, 0.75]`.
    pub min_score: f64,
    /// Direction consensus: kaç yapısal yönlü oy gerekir? (2/3 default).
    pub min_direction_votes: u8,
    /// Regime labels that categorically conflict with the candidate.
    pub reject_on_regimes: Vec<String>,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            weights: ConfluenceWeights::default(),
            min_score: 0.55,
            min_direction_votes: 2,
            reject_on_regimes: vec![],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VetoKind {
    KillSwitch,
    StaleData,
    NewsBlackout,
    RegimeOpposite,
    DirectionConsensusFail,
    BelowMinScore,
    NoDirection,
}

impl VetoKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VetoKind::KillSwitch => "kill_switch",
            VetoKind::StaleData => "stale_data",
            VetoKind::NewsBlackout => "news_blackout",
            VetoKind::RegimeOpposite => "regime_opposite",
            VetoKind::DirectionConsensusFail => "direction_consensus_fail",
            VetoKind::BelowMinScore => "below_min_score",
            VetoKind::NoDirection => "no_direction",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GateApproval {
    pub direction: ConfluenceDirection,
    pub final_score: f64,
    pub reading: ConfluenceReading,
    pub direction_votes: u8,
    pub rationale: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GateRejection {
    pub kind: VetoKind,
    pub reason: String,
    /// Optional partial reading for logging / GUI inspector.
    pub reading: Option<ConfluenceReading>,
}

pub type GateDecision = Result<GateApproval, GateRejection>;

// ---------------------------------------------------------------------------
// Veto rules (Layer 1) — dispatch registry.
// ---------------------------------------------------------------------------

pub trait VetoRule: Send + Sync {
    fn kind(&self) -> VetoKind;
    fn evaluate(&self, ctx: &GateContext, cfg: &GateConfig) -> Option<String>;
}

struct KillSwitchVeto;
impl VetoRule for KillSwitchVeto {
    fn kind(&self) -> VetoKind {
        VetoKind::KillSwitch
    }
    fn evaluate(&self, ctx: &GateContext, _: &GateConfig) -> Option<String> {
        ctx.kill_switch_on.then(|| "kill_switch active".to_string())
    }
}

struct StaleDataVeto;
impl VetoRule for StaleDataVeto {
    fn kind(&self) -> VetoKind {
        VetoKind::StaleData
    }
    fn evaluate(&self, ctx: &GateContext, _: &GateConfig) -> Option<String> {
        ctx.stale_data.then(|| "feature freshness check failed".to_string())
    }
}

struct NewsBlackoutVeto;
impl VetoRule for NewsBlackoutVeto {
    fn kind(&self) -> VetoKind {
        VetoKind::NewsBlackout
    }
    fn evaluate(&self, ctx: &GateContext, _: &GateConfig) -> Option<String> {
        ctx.news_blackout.then(|| "news blackout window".to_string())
    }
}

struct RegimeOppositeVeto;
impl VetoRule for RegimeOppositeVeto {
    fn kind(&self) -> VetoKind {
        VetoKind::RegimeOpposite
    }
    fn evaluate(&self, ctx: &GateContext, cfg: &GateConfig) -> Option<String> {
        let label = ctx.regime_label.as_deref()?;
        cfg.reject_on_regimes
            .iter()
            .any(|r| r.eq_ignore_ascii_case(label))
            .then(|| format!("regime '{label}' on reject list"))
    }
}

static RULES: &[&dyn VetoRule] = &[
    &KillSwitchVeto,
    &StaleDataVeto,
    &NewsBlackoutVeto,
    &RegimeOppositeVeto,
];

// ---------------------------------------------------------------------------
// Direction consensus (Layer 2)
// ---------------------------------------------------------------------------

fn structural_family(fam: &str) -> bool {
    matches!(fam, "elliott" | "wyckoff" | "classical")
}

fn tally_direction(detections: &[DetectionVote]) -> (u8, u8) {
    let mut long_n = 0u8;
    let mut short_n = 0u8;
    for v in detections.iter().filter(|v| structural_family(&v.family)) {
        match v.direction {
            ConfluenceDirection::Long => long_n += 1,
            ConfluenceDirection::Short => short_n += 1,
            _ => {}
        }
    }
    (long_n, short_n)
}

fn consensus_direction(
    detections: &[DetectionVote],
    min_votes: u8,
) -> Option<(ConfluenceDirection, u8)> {
    let (long_n, short_n) = tally_direction(detections);
    if long_n >= min_votes && long_n > short_n {
        return Some((ConfluenceDirection::Long, long_n));
    }
    if short_n >= min_votes && short_n > long_n {
        return Some((ConfluenceDirection::Short, short_n));
    }
    None
}

// ---------------------------------------------------------------------------
// Public entry
// ---------------------------------------------------------------------------

pub fn should_open(ctx: &GateContext, cfg: &GateConfig) -> GateDecision {
    // Layer 1 — veto dispatch.
    for rule in RULES {
        if let Some(msg) = rule.evaluate(ctx, cfg) {
            return Err(GateRejection {
                kind: rule.kind(),
                reason: msg,
                reading: None,
            });
        }
    }

    // Layer 2 — direction consensus.
    let (consensus_dir, votes) =
        match consensus_direction(&ctx.inputs.detections, cfg.min_direction_votes) {
            Some(x) => x,
            None => {
                return Err(GateRejection {
                    kind: VetoKind::DirectionConsensusFail,
                    reason: format!(
                        "structural direction vote < {} (elliott+wyckoff+classical)",
                        cfg.min_direction_votes
                    ),
                    reading: None,
                });
            }
        };

    // Layer 3 — weighted score via qtss-confluence.
    let reading = score_confluence(&ctx.inputs, &cfg.weights);

    // Consensus direction & weighted direction must agree (bilateral
    // check — avoids TBM-only override flipping structural majority).
    if reading.direction != ConfluenceDirection::Neutral
        && reading.direction != consensus_dir
    {
        return Err(GateRejection {
            kind: VetoKind::DirectionConsensusFail,
            reason: format!(
                "weighted direction {:?} disagrees with consensus {:?}",
                reading.direction, consensus_dir
            ),
            reading: Some(reading),
        });
    }

    if reading.guven < cfg.min_score {
        return Err(GateRejection {
            kind: VetoKind::BelowMinScore,
            reason: format!(
                "guven {:.3} < min_score {:.3}",
                reading.guven, cfg.min_score
            ),
            reading: Some(reading),
        });
    }

    let rationale = vec![
        format!("consensus_votes={votes}"),
        format!("guven={:.3}", reading.guven),
        format!("erken_uyari={:.3}", reading.erken_uyari),
        format!("layers={}", reading.layer_count),
    ];
    Ok(GateApproval {
        direction: consensus_dir,
        final_score: reading.guven,
        reading,
        direction_votes: votes,
        rationale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vote(family: &str, dir: ConfluenceDirection, score: f32) -> DetectionVote {
        DetectionVote {
            family: family.to_string(),
            subkind: "s".to_string(),
            direction: dir,
            structural_score: score,
        }
    }

    fn ctx_with_votes(v: Vec<DetectionVote>) -> GateContext {
        let mut inp = ConfluenceInputs::default();
        inp.tbm_score = Some(0.8);
        inp.tbm_confidence = Some(0.9);
        inp.detections = v;
        GateContext { inputs: inp, ..Default::default() }
    }

    #[test]
    fn kill_switch_rejects_immediately() {
        let mut ctx = ctx_with_votes(vec![
            vote("elliott", ConfluenceDirection::Long, 0.8),
            vote("wyckoff", ConfluenceDirection::Long, 0.8),
        ]);
        ctx.kill_switch_on = true;
        let r = should_open(&ctx, &GateConfig::default()).unwrap_err();
        assert_eq!(r.kind, VetoKind::KillSwitch);
    }

    #[test]
    fn stale_data_rejects() {
        let mut ctx = ctx_with_votes(vec![]);
        ctx.stale_data = true;
        let r = should_open(&ctx, &GateConfig::default()).unwrap_err();
        assert_eq!(r.kind, VetoKind::StaleData);
    }

    #[test]
    fn regime_opposite_rejects() {
        let mut ctx = ctx_with_votes(vec![
            vote("elliott", ConfluenceDirection::Long, 0.8),
            vote("wyckoff", ConfluenceDirection::Long, 0.8),
        ]);
        ctx.regime_label = Some("strong_downtrend".to_string());
        let cfg = GateConfig {
            reject_on_regimes: vec!["strong_downtrend".into()],
            ..Default::default()
        };
        let r = should_open(&ctx, &cfg).unwrap_err();
        assert_eq!(r.kind, VetoKind::RegimeOpposite);
    }

    #[test]
    fn one_vote_fails_consensus() {
        let ctx = ctx_with_votes(vec![vote("elliott", ConfluenceDirection::Long, 0.8)]);
        let r = should_open(&ctx, &GateConfig::default()).unwrap_err();
        assert_eq!(r.kind, VetoKind::DirectionConsensusFail);
    }

    #[test]
    fn two_of_three_agree_passes_low_score_still_rejects() {
        // 2 long + 1 weak score → consensus ok, guven maybe < threshold.
        let ctx = ctx_with_votes(vec![
            vote("elliott", ConfluenceDirection::Long, 0.3),
            vote("wyckoff", ConfluenceDirection::Long, 0.3),
        ]);
        let cfg = GateConfig {
            min_score: 0.9,
            ..Default::default()
        };
        let r = should_open(&ctx, &cfg).unwrap_err();
        assert_eq!(r.kind, VetoKind::BelowMinScore);
    }

    #[test]
    fn strong_three_long_approves() {
        let ctx = ctx_with_votes(vec![
            vote("elliott", ConfluenceDirection::Long, 0.9),
            vote("wyckoff", ConfluenceDirection::Long, 0.9),
            vote("classical", ConfluenceDirection::Long, 0.9),
        ]);
        let cfg = GateConfig {
            min_score: 0.3,
            ..Default::default()
        };
        let ok = should_open(&ctx, &cfg).unwrap();
        assert_eq!(ok.direction, ConfluenceDirection::Long);
        assert_eq!(ok.direction_votes, 3);
        assert!(ok.final_score >= 0.3);
    }

    #[test]
    fn split_vote_fails() {
        let ctx = ctx_with_votes(vec![
            vote("elliott", ConfluenceDirection::Long, 0.8),
            vote("wyckoff", ConfluenceDirection::Short, 0.8),
        ]);
        let r = should_open(&ctx, &GateConfig::default()).unwrap_err();
        assert_eq!(r.kind, VetoKind::DirectionConsensusFail);
    }

    #[test]
    fn non_structural_votes_ignored_for_consensus() {
        // harmonic+range vote long but structural only one → fail.
        let ctx = ctx_with_votes(vec![
            vote("harmonic", ConfluenceDirection::Long, 0.9),
            vote("range", ConfluenceDirection::Long, 0.9),
            vote("elliott", ConfluenceDirection::Long, 0.9),
        ]);
        let r = should_open(&ctx, &GateConfig::default()).unwrap_err();
        assert_eq!(r.kind, VetoKind::DirectionConsensusFail);
    }
}

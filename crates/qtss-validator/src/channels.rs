//! Confirmation channels.
//!
//! A channel is anything that can look at a candidate detection and
//! return an opinion in `0..1` about how likely it is to play out.
//! Returning `None` means "no opinion" — the validator simply leaves
//! that channel out of the blend.
//!
//! Channels are dispatched through the [`ConfirmationChannel`] trait so
//! adding a new one is `impl ConfirmationChannel for MyChannel` plus a
//! single `register` call (CLAUDE.md rule #1: trait + dispatch instead
//! of scattered match arms).

use crate::context::{is_higher_timeframe, pattern_key, ValidationContext};
use qtss_domain::v2::detection::{Detection, PatternKind};
use qtss_domain::v2::regime::{RegimeKind, TrendStrength};

pub trait ConfirmationChannel: Send + Sync {
    fn name(&self) -> &'static str;
    fn evaluate(&self, det: &Detection, ctx: &ValidationContext) -> Option<f64>;
}

// ---------------------------------------------------------------------------
// Regime alignment
// ---------------------------------------------------------------------------

/// Some patterns are reliable in trending regimes (Elliott impulses,
/// classical breakouts), others want a sideways tape (harmonics inside
/// PRZ, Wyckoff trading-range events). The lookup table makes the
/// expected regime explicit per family — adding a new family is one
/// row, no central match to edit.
pub struct RegimeAlignment;

#[derive(Debug, Clone, Copy)]
enum PreferredRegime {
    Trending,
    Ranging,
    Either,
}

fn preferred_regime(kind: &PatternKind) -> PreferredRegime {
    match kind {
        PatternKind::Elliott(_) => PreferredRegime::Trending,
        PatternKind::Harmonic(_) => PreferredRegime::Ranging,
        PatternKind::Classical(name) => {
            if name.contains("triangle") || name.contains("wedge") {
                PreferredRegime::Either
            } else {
                PreferredRegime::Trending
            }
        }
        PatternKind::Wyckoff(_) => PreferredRegime::Ranging,
        PatternKind::Range(_) => PreferredRegime::Ranging,
        PatternKind::Custom(_) => PreferredRegime::Either,
    }
}

impl ConfirmationChannel for RegimeAlignment {
    fn name(&self) -> &'static str {
        "regime_alignment"
    }

    fn evaluate(&self, det: &Detection, _ctx: &ValidationContext) -> Option<f64> {
        let regime = &det.regime_at_detection;
        let pref = preferred_regime(&det.kind);
        let strength_bonus = match regime.trend_strength {
            TrendStrength::None => 0.0,
            TrendStrength::Weak => 0.05,
            TrendStrength::Moderate => 0.10,
            TrendStrength::Strong => 0.15,
            TrendStrength::VeryStrong => 0.20,
        };
        let base: f64 = match (pref, regime.kind) {
            (PreferredRegime::Either, _) => 0.7,
            (PreferredRegime::Trending, RegimeKind::TrendingUp) => 0.9 + strength_bonus,
            (PreferredRegime::Trending, RegimeKind::TrendingDown) => 0.9 + strength_bonus,
            (PreferredRegime::Trending, _) => 0.35,
            (PreferredRegime::Ranging, RegimeKind::Ranging) => 0.9,
            (PreferredRegime::Ranging, RegimeKind::Squeeze) => 0.8,
            (PreferredRegime::Ranging, _) => 0.35,
        };
        Some(base.min(1.0))
    }
}

// ---------------------------------------------------------------------------
// Multi-timeframe confluence
// ---------------------------------------------------------------------------

/// Looks for an agreeing detection on a strictly higher timeframe with
/// a *compatible* family. Score is driven by the highest structural
/// score among the matching higher-TF detections.
pub struct MultiTimeframeConfluence;

fn family_label(kind: &PatternKind) -> &'static str {
    match kind {
        PatternKind::Elliott(_) => "elliott",
        PatternKind::Harmonic(_) => "harmonic",
        PatternKind::Classical(_) => "classical",
        PatternKind::Wyckoff(_) => "wyckoff",
        PatternKind::Range(_) => "range",
        PatternKind::Custom(_) => "custom",
    }
}

impl ConfirmationChannel for MultiTimeframeConfluence {
    fn name(&self) -> &'static str {
        "multi_timeframe"
    }

    fn evaluate(&self, det: &Detection, ctx: &ValidationContext) -> Option<f64> {
        if ctx.higher_tf_detections.is_empty() {
            return None; // no opinion — caller hasn't supplied any HTF context
        }
        let same_family = family_label(&det.kind);
        let same_symbol = &det.instrument.symbol;
        let mut best: Option<f32> = None;
        for other in &ctx.higher_tf_detections {
            if other.instrument.symbol != *same_symbol {
                continue;
            }
            if !is_higher_timeframe(other.timeframe, det.timeframe) {
                continue;
            }
            if family_label(&other.kind) != same_family {
                continue;
            }
            best = Some(best.map(|b| b.max(other.structural_score)).unwrap_or(other.structural_score));
        }
        // No HTF match at all = a soft penalty (the channel *did* look,
        // it just didn't find anything supportive).
        Some(best.map(|b| b as f64).unwrap_or(0.3))
    }
}

// ---------------------------------------------------------------------------
// Historical hit rate
// ---------------------------------------------------------------------------

/// Pulls the historical hit-rate for this `(family, timeframe)` from the
/// supplied stats. Requires a minimum sample size before it will speak —
/// otherwise it returns `None` and is excluded from the blend.
pub struct HistoricalHitRate {
    pub min_samples: u32,
}

impl ConfirmationChannel for HistoricalHitRate {
    fn name(&self) -> &'static str {
        "historical_hit_rate"
    }

    fn evaluate(&self, det: &Detection, ctx: &ValidationContext) -> Option<f64> {
        let key = pattern_key(det);
        let stat = ctx.hit_rates.get(&key)?;
        if stat.samples < self.min_samples {
            return None;
        }
        Some(stat.hit_rate as f64)
    }
}

// ---------------------------------------------------------------------------
// Multi-TF regime confluence (Faz 11)
// ---------------------------------------------------------------------------

/// Boosts confidence when the multi-TF regime agrees with the pattern's
/// preferred regime, penalises when it disagrees or is transitioning.
pub struct MultiTfRegimeConfluence;

impl ConfirmationChannel for MultiTfRegimeConfluence {
    fn name(&self) -> &'static str {
        "multi_tf_regime_confluence"
    }

    fn evaluate(&self, det: &Detection, ctx: &ValidationContext) -> Option<f64> {
        let mtf = ctx.multi_tf_regime.as_ref()?;
        let pref = preferred_regime(&det.kind);

        // Transition penalty: if timeframes disagree, reduce confidence
        let transition_penalty = if mtf.is_transitioning { 0.15 } else { 0.0 };

        let alignment = match (pref, mtf.dominant_regime) {
            (PreferredRegime::Either, _) => 0.7,
            (PreferredRegime::Trending, RegimeKind::TrendingUp | RegimeKind::TrendingDown) => 0.9,
            (PreferredRegime::Trending, _) => 0.3,
            (PreferredRegime::Ranging, RegimeKind::Ranging | RegimeKind::Squeeze) => 0.9,
            (PreferredRegime::Ranging, _) => 0.3,
        };

        // Weight by confluence score (how aligned the TFs are)
        let score = (alignment * mtf.confluence_score - transition_penalty).clamp(0.0, 1.0);
        Some(score)
    }
}

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
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::detection::{Detection, PatternKind};
use qtss_domain::v2::regime::{RegimeKind, TrendStrength};
use rust_decimal::prelude::ToPrimitive;

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

// ---------------------------------------------------------------------------
// Classical breakout / volume channels (P2)
// ---------------------------------------------------------------------------
//
// Every threshold here is config-driven by construction: the channel
// struct carries the numeric knobs so the worker reads them from
// system_config at startup (CLAUDE.md #2). No magic numbers inside the
// evaluate() bodies.

fn dec_f64(d: rust_decimal::Decimal) -> Option<f64> {
    d.to_f64()
}

/// Wilder-style ATR over the final `period` fully-closed bars of the
/// supplied window. Returns `None` when there are not enough bars.
fn atr(bars: &[Bar], period: usize) -> Option<f64> {
    if bars.len() < period + 1 {
        return None;
    }
    let tail = &bars[bars.len() - period - 1..];
    let mut sum: f64 = 0.0;
    for w in tail.windows(2) {
        let prev_close = dec_f64(w[0].close)?;
        let h = dec_f64(w[1].high)?;
        let l = dec_f64(w[1].low)?;
        let tr = (h - l).max((h - prev_close).abs()).max((l - prev_close).abs());
        sum += tr;
    }
    Some(sum / period as f64)
}

fn is_classical_directional(det: &Detection) -> bool {
    match &det.kind {
        PatternKind::Classical(name) => {
            // Neutral patterns (symmetrical triangle pre-breakout) have
            // no meaningful breakout direction yet.
            !name.contains("symmetrical") && !name.ends_with("_neutral")
        }
        _ => false,
    }
}

fn is_bullish_subkind(det: &Detection) -> bool {
    match &det.kind {
        PatternKind::Classical(name) => {
            name.contains("bull") || name.contains("bottom") || name.contains("ascending")
        }
        _ => false,
    }
}

/// Breakout close vs wick.
///
/// A valid breakout closes BEYOND the pattern's invalidation / neckline
/// line — not just pierces with a wick. Score is the fraction of bar range
/// that the close sits on the breakout side of the line. Kitabi kural:
/// "close beyond neckline confirms breakout, wick alone does not".
pub struct BreakoutCloseQuality;

impl ConfirmationChannel for BreakoutCloseQuality {
    fn name(&self) -> &'static str {
        "breakout_close_quality"
    }

    fn evaluate(&self, det: &Detection, ctx: &ValidationContext) -> Option<f64> {
        if !is_classical_directional(det) {
            return None;
        }
        let last = ctx.recent_bars.last()?;
        let line = dec_f64(det.invalidation_price)?;
        let close = dec_f64(last.close)?;
        let high = dec_f64(last.high)?;
        let low = dec_f64(last.low)?;
        let range = (high - low).max(1e-9);
        if is_bullish_subkind(det) {
            // break to the upside: price must close ABOVE the line. Score
            // = how deep into the bar range the close sits above it, so a
            // wick-only poke (close back inside) scores near zero.
            if close <= line {
                return Some(0.0);
            }
            Some(((close - line) / range).clamp(0.0, 1.0))
        } else {
            if close >= line {
                return Some(0.0);
            }
            Some(((line - close) / range).clamp(0.0, 1.0))
        }
    }
}

/// Breakout candle body size vs ATR.
///
/// A "strong" breakout has a body at least `min_body_atr_mult` × ATR.
/// Narrow-range breakouts are traps (Bulkowski). Score scales linearly
/// up to `max_body_atr_mult`, above which the bar is "exhaustively wide"
/// and may be climactic — capped at 1.0.
pub struct BreakoutBodyAtr {
    pub atr_period: usize,
    pub min_body_atr_mult: f64,
    pub max_body_atr_mult: f64,
}

impl ConfirmationChannel for BreakoutBodyAtr {
    fn name(&self) -> &'static str {
        "breakout_body_atr"
    }

    fn evaluate(&self, det: &Detection, ctx: &ValidationContext) -> Option<f64> {
        if !is_classical_directional(det) {
            return None;
        }
        let last = ctx.recent_bars.last()?;
        let body = dec_f64(last.close.max(last.open) - last.close.min(last.open))?;
        let atr_v = atr(&ctx.recent_bars, self.atr_period)?;
        if atr_v <= 0.0 {
            return None;
        }
        let mult = body / atr_v;
        if mult < self.min_body_atr_mult {
            return Some((mult / self.min_body_atr_mult).clamp(0.0, 1.0) * 0.4);
        }
        let span = (self.max_body_atr_mult - self.min_body_atr_mult).max(1e-9);
        Some(((mult - self.min_body_atr_mult) / span).clamp(0.0, 1.0))
    }
}

/// Volume confirmation: declining through pattern, expanding on breakout.
///
/// Classical TA rule (Edwards & Magee, Bulkowski):
///   - volume should DRY UP as pattern matures
///   - breakout bar volume should EXPAND sharply (>= N × avg)
///
/// We split the recent_bars window into "pattern" (earliest 80%) and
/// "breakout" (final bar); compare avg-volume pattern-early vs pattern-late
/// and the breakout bar vs overall pattern avg.
pub struct VolumeConfirmation {
    pub min_breakout_vol_mult: f64,
    pub max_late_to_early_ratio: f64,
}

impl ConfirmationChannel for VolumeConfirmation {
    fn name(&self) -> &'static str {
        "volume_confirmation"
    }

    fn evaluate(&self, det: &Detection, ctx: &ValidationContext) -> Option<f64> {
        if !is_classical_directional(det) {
            return None;
        }
        let bars = &ctx.recent_bars;
        if bars.len() < 10 {
            return None;
        }
        let (pattern_bars, breakout) = bars.split_at(bars.len() - 1);
        let n = pattern_bars.len();
        let half = n / 2;
        let avg = |slice: &[Bar]| -> Option<f64> {
            if slice.is_empty() {
                return None;
            }
            let mut s = 0.0;
            for b in slice {
                s += dec_f64(b.volume)?;
            }
            Some(s / slice.len() as f64)
        };
        let early = avg(&pattern_bars[..half])?;
        let late = avg(&pattern_bars[half..])?;
        let all_pat = avg(pattern_bars)?;
        let bo_vol = dec_f64(breakout[0].volume)?;
        if all_pat <= 0.0 || early <= 0.0 {
            return None;
        }
        // Contraction score: late/early <= max_late_to_early_ratio is
        // "dried up". Map linearly: 1.0 at ratio=0, 0.0 at ratio=cap.
        let contraction_ratio = late / early;
        let contraction_score =
            (1.0 - (contraction_ratio / self.max_late_to_early_ratio).clamp(0.0, 1.0)).max(0.0);
        // Expansion score: breakout volume >= min_mult × pattern avg.
        let expansion_mult = bo_vol / all_pat;
        let expansion_score = (expansion_mult / self.min_breakout_vol_mult).clamp(0.0, 1.0);
        // Equal-weight combine.
        Some((contraction_score * 0.5 + expansion_score * 0.5).clamp(0.0, 1.0))
    }
}

/// P6 — Retest / throwback quality.
///
/// After a directional breakout (close beyond `invalidation_price`), the
/// textbook continuation requires price to PULL BACK to the broken level
/// and BOUNCE off it (kitabi: "kırılan direnç destek olur"). Failed
/// retests (price closing back through the line) invalidate the breakout.
///
/// Scoring (all checks against `ctx.recent_bars`, oldest..newest):
///   - no breakout yet                    → None  (silent abstain)
///   - breakout, no test yet              → 0.40
///   - tested within tolerance, no bounce → 0.70
///   - tested + bounced (clean continuation) → 1.0
///   - test FAILED (close through line on wrong side) → 0.00
///
/// Tolerance is fraction of price: a retest is "touched" when any bar's
/// wick comes within `tolerance_pct × line_price` of the line.
pub struct RetestQuality {
    pub tolerance_pct: f64,
    pub max_bars_after_breakout: usize,
}

impl ConfirmationChannel for RetestQuality {
    fn name(&self) -> &'static str {
        "retest_quality"
    }

    fn evaluate(&self, det: &Detection, ctx: &ValidationContext) -> Option<f64> {
        if !is_classical_directional(det) {
            return None;
        }
        let bars = &ctx.recent_bars;
        if bars.len() < 3 {
            return None;
        }
        let line = dec_f64(det.invalidation_price)?;
        if line.abs() <= 0.0 {
            return None;
        }
        let bull = is_bullish_subkind(det);
        // Locate breakout bar = first bar (chronological) whose close is
        // beyond the line in the directional sense.
        let mut bo_idx: Option<usize> = None;
        for (i, b) in bars.iter().enumerate() {
            let c = match dec_f64(b.close) { Some(v) => v, None => continue };
            let crossed = if bull { c > line } else { c < line };
            if crossed {
                bo_idx = Some(i);
                break;
            }
        }
        let bo = bo_idx?;
        // Walk forward up to max_bars_after_breakout looking for: (a)
        // failure (close back on wrong side), (b) test (wick within
        // tolerance), (c) bounce (close moves further away after test).
        let tol = self.tolerance_pct * line.abs();
        let end = (bo + 1 + self.max_bars_after_breakout).min(bars.len());
        let mut tested = false;
        for i in (bo + 1)..end {
            let b = &bars[i];
            let c = dec_f64(b.close)?;
            let h = dec_f64(b.high)?;
            let l = dec_f64(b.low)?;
            // Failure: close back on wrong side ⇒ failed breakout.
            let failed = if bull { c < line } else { c > line };
            if failed {
                return Some(0.0);
            }
            // Test: wick within tolerance of the line (low for bull, high
            // for bear).
            let touched = if bull { l <= line + tol } else { h >= line - tol };
            if touched {
                tested = true;
            }
            // Bounce after test: bar closes a meaningful distance away
            // from the line (>= tolerance away).
            if tested {
                let away = if bull { c - line } else { line - c };
                if away >= tol {
                    return Some(1.0);
                }
            }
        }
        // No failure observed; return partial score depending on whether
        // we even saw a test.
        Some(if tested { 0.7 } else { 0.4 })
    }
}

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

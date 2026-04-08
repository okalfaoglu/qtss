//! Regime classification.
//!
//! Implemented as an ordered rule table rather than scattered if/else
//! (per CLAUDE.md rule #1). The first matching rule wins. Adding a new
//! regime kind = appending one entry — no central branch to edit.

use crate::config::RegimeConfig;
use qtss_domain::v2::regime::{RegimeKind, TrendStrength};

/// Indicator bundle the classifier looks at. All values are post-warm-up.
#[derive(Debug, Clone, Copy)]
pub struct Indicators {
    pub adx: f64,
    pub plus_di: f64,
    pub minus_di: f64,
    pub bb_width: f64,
    pub atr_pct: f64,
    pub choppiness: f64,
}

/// Verdict produced by the classifier.
#[derive(Debug, Clone, Copy)]
pub struct Verdict {
    pub kind: RegimeKind,
    pub trend_strength: TrendStrength,
    pub confidence: f32,
}

type Rule = fn(&Indicators, &RegimeConfig) -> Option<Verdict>;

/// Ordered rules. First match wins.
const RULES: &[Rule] = &[
    rule_squeeze,
    rule_trending_up,
    rule_trending_down,
    rule_ranging,
    rule_volatile,
];

pub fn classify(ind: &Indicators, cfg: &RegimeConfig) -> Verdict {
    for rule in RULES {
        if let Some(v) = rule(ind, cfg) {
            return v;
        }
    }
    Verdict {
        kind: RegimeKind::Uncertain,
        trend_strength: TrendStrength::None,
        confidence: 0.3,
    }
}

// ---------------------------------------------------------------------------
// Individual rules. Each is a single function so it can be unit-tested
// in isolation and re-ordered without touching the engine.
// ---------------------------------------------------------------------------

fn rule_squeeze(ind: &Indicators, cfg: &RegimeConfig) -> Option<Verdict> {
    // A squeeze is a low-volatility coil regardless of ADX direction.
    if ind.bb_width < cfg.bb_squeeze_threshold && ind.atr_pct < cfg.volatility_threshold {
        return Some(Verdict {
            kind: RegimeKind::Squeeze,
            trend_strength: TrendStrength::None,
            confidence: 0.75,
        });
    }
    None
}

fn rule_trending_up(ind: &Indicators, cfg: &RegimeConfig) -> Option<Verdict> {
    if ind.adx >= cfg.adx_trend_threshold && ind.plus_di > ind.minus_di {
        return Some(Verdict {
            kind: RegimeKind::TrendingUp,
            trend_strength: trend_strength(ind.adx, cfg),
            confidence: trend_confidence(ind.adx, cfg),
        });
    }
    None
}

fn rule_trending_down(ind: &Indicators, cfg: &RegimeConfig) -> Option<Verdict> {
    if ind.adx >= cfg.adx_trend_threshold && ind.minus_di > ind.plus_di {
        return Some(Verdict {
            kind: RegimeKind::TrendingDown,
            trend_strength: trend_strength(ind.adx, cfg),
            confidence: trend_confidence(ind.adx, cfg),
        });
    }
    None
}

fn rule_ranging(ind: &Indicators, cfg: &RegimeConfig) -> Option<Verdict> {
    if ind.choppiness >= cfg.chop_range_threshold {
        return Some(Verdict {
            kind: RegimeKind::Ranging,
            trend_strength: TrendStrength::None,
            confidence: 0.7,
        });
    }
    None
}

fn rule_volatile(ind: &Indicators, cfg: &RegimeConfig) -> Option<Verdict> {
    if ind.atr_pct >= cfg.volatility_threshold {
        return Some(Verdict {
            kind: RegimeKind::Volatile,
            trend_strength: TrendStrength::None,
            confidence: 0.6,
        });
    }
    None
}

// ---------------------------------------------------------------------------
// Helpers — kept tiny so each rule stays one branch deep.
// ---------------------------------------------------------------------------

fn trend_strength(adx: f64, cfg: &RegimeConfig) -> TrendStrength {
    // Single match → no scattered if/else.
    let bucket = if adx >= cfg.adx_strong_threshold + 20.0 {
        4
    } else if adx >= cfg.adx_strong_threshold {
        3
    } else if adx >= cfg.adx_trend_threshold + 10.0 {
        2
    } else if adx >= cfg.adx_trend_threshold {
        1
    } else {
        0
    };
    match bucket {
        0 => TrendStrength::None,
        1 => TrendStrength::Weak,
        2 => TrendStrength::Moderate,
        3 => TrendStrength::Strong,
        _ => TrendStrength::VeryStrong,
    }
}

fn trend_confidence(adx: f64, cfg: &RegimeConfig) -> f32 {
    let span = (cfg.adx_strong_threshold - cfg.adx_trend_threshold).max(1.0);
    let raw = ((adx - cfg.adx_trend_threshold) / span).clamp(0.0, 1.0);
    (0.5 + 0.5 * raw) as f32
}

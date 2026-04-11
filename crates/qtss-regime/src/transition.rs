//! Regime transition detection.
//!
//! Compares consecutive regime snapshots for the same (symbol, interval)
//! pair and emits a `RegimeTransition` when the regime kind changes.
//! No IO — the caller persists transitions.

use chrono::{DateTime, Utc};
use qtss_domain::v2::regime::{RegimeKind, RegimeSnapshot};

/// A detected regime transition.
#[derive(Debug, Clone)]
pub struct RegimeTransition {
    pub symbol: String,
    pub interval: String,
    pub from_regime: RegimeKind,
    pub to_regime: RegimeKind,
    /// 0.0–1.0; how abrupt the transition is (high = fast).
    pub transition_speed: f64,
    /// Indicator names that confirm the transition.
    pub confirming_indicators: Vec<String>,
    pub detected_at: DateTime<Utc>,
    pub confidence: f64,
}

/// Detect a transition between two consecutive snapshots.
///
/// Returns `None` when the regime kind has not changed or when
/// confidence is below `min_confidence`.
pub fn detect_transition(
    symbol: &str,
    interval: &str,
    prev: &RegimeSnapshot,
    curr: &RegimeSnapshot,
    min_confidence: f64,
) -> Option<RegimeTransition> {
    if prev.kind == curr.kind {
        return None;
    }

    let mut confirming = Vec::new();
    let speed = compute_speed(prev, curr, &mut confirming);
    let confidence = (curr.confidence as f64 * 0.6 + speed * 0.4).clamp(0.0, 1.0);

    if confidence < min_confidence {
        return None;
    }

    Some(RegimeTransition {
        symbol: symbol.to_string(),
        interval: interval.to_string(),
        from_regime: prev.kind,
        to_regime: curr.kind,
        transition_speed: speed,
        confirming_indicators: confirming,
        detected_at: curr.at,
        confidence,
    })
}

/// Compute transition speed (0–1) and collect confirming indicators.
fn compute_speed(
    prev: &RegimeSnapshot,
    curr: &RegimeSnapshot,
    confirming: &mut Vec<String>,
) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    let mut speed_factors = Vec::new();

    // ADX delta
    let adx_prev = prev.adx.to_f64().unwrap_or(0.0);
    let adx_curr = curr.adx.to_f64().unwrap_or(0.0);
    let adx_delta = (adx_curr - adx_prev).abs();
    if adx_delta > 5.0 {
        confirming.push("adx".into());
        speed_factors.push((adx_delta / 20.0).clamp(0.0, 1.0));
    }

    // BB width delta
    let bb_prev = prev.bb_width.to_f64().unwrap_or(0.0);
    let bb_curr = curr.bb_width.to_f64().unwrap_or(0.0);
    let bb_delta = (bb_curr - bb_prev).abs();
    if bb_delta > 0.01 {
        confirming.push("bb_width".into());
        speed_factors.push((bb_delta / 0.05).clamp(0.0, 1.0));
    }

    // ATR delta
    let atr_prev = prev.atr_pct.to_f64().unwrap_or(0.0);
    let atr_curr = curr.atr_pct.to_f64().unwrap_or(0.0);
    let atr_delta = (atr_curr - atr_prev).abs();
    if atr_delta > 0.005 {
        confirming.push("atr_pct".into());
        speed_factors.push((atr_delta / 0.02).clamp(0.0, 1.0));
    }

    // Choppiness delta
    let chop_prev = prev.choppiness.to_f64().unwrap_or(50.0);
    let chop_curr = curr.choppiness.to_f64().unwrap_or(50.0);
    let chop_delta = (chop_curr - chop_prev).abs();
    if chop_delta > 5.0 {
        confirming.push("choppiness".into());
        speed_factors.push((chop_delta / 20.0).clamp(0.0, 1.0));
    }

    if speed_factors.is_empty() {
        0.3 // minimal change, still a transition
    } else {
        speed_factors.iter().sum::<f64>() / speed_factors.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use qtss_domain::v2::regime::TrendStrength;
    use rust_decimal_macros::dec;

    #[test]
    fn no_transition_same_regime() {
        let s = RegimeSnapshot {
            at: Utc::now(),
            kind: RegimeKind::Ranging,
            trend_strength: TrendStrength::None,
            adx: dec!(20),
            bb_width: dec!(0.03),
            atr_pct: dec!(0.02),
            choppiness: dec!(65),
            confidence: 0.7,
        };
        assert!(detect_transition("BTC", "1h", &s, &s, 0.5).is_none());
    }

    #[test]
    fn detects_squeeze_to_trend() {
        let prev = RegimeSnapshot {
            at: Utc::now(),
            kind: RegimeKind::Squeeze,
            trend_strength: TrendStrength::None,
            adx: dec!(18),
            bb_width: dec!(0.02),
            atr_pct: dec!(0.01),
            choppiness: dec!(70),
            confidence: 0.75,
        };
        let curr = RegimeSnapshot {
            at: Utc::now(),
            kind: RegimeKind::TrendingUp,
            trend_strength: TrendStrength::Strong,
            adx: dec!(35),
            bb_width: dec!(0.06),
            atr_pct: dec!(0.03),
            choppiness: dec!(40),
            confidence: 0.85,
        };
        let t = detect_transition("BTC", "4h", &prev, &curr, 0.5).unwrap();
        assert_eq!(t.from_regime, RegimeKind::Squeeze);
        assert_eq!(t.to_regime, RegimeKind::TrendingUp);
        assert!(!t.confirming_indicators.is_empty());
    }
}

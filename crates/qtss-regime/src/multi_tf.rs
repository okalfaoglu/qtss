//! Multi-timeframe regime confluence.
//!
//! Collects per-interval regime snapshots for a single symbol and
//! computes a weighted dominant regime + confluence score. No IO —
//! the caller provides the snapshots and weights.

use qtss_domain::v2::regime::{RegimeKind, RegimeSnapshot};
use std::collections::HashMap;

/// Result of multi-TF confluence computation.
#[derive(Debug, Clone)]
pub struct MultiTfRegime {
    pub symbol: String,
    /// Per-interval snapshots used in the computation.
    pub snapshots: Vec<(String, RegimeSnapshot)>,
    /// Weighted dominant regime.
    pub dominant_regime: RegimeKind,
    /// 0.0–1.0 — how aligned the timeframes are.
    pub confluence_score: f64,
    /// True when at least two timeframes disagree on the regime category.
    pub is_transitioning: bool,
}

/// Default timeframe weights (used when caller provides none).
pub fn default_tf_weights() -> HashMap<String, f64> {
    [
        ("5m", 0.10),
        ("15m", 0.15),
        ("1h", 0.25),
        ("4h", 0.30),
        ("1d", 0.20),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Compute multi-TF confluence from a set of (interval, snapshot) pairs.
///
/// `weights` maps interval string → weight (should sum to ~1.0).
/// Missing intervals are ignored; the remaining weights are re-normalised.
pub fn compute_confluence(
    symbol: &str,
    snapshots: &[(String, RegimeSnapshot)],
    weights: &HashMap<String, f64>,
) -> Option<MultiTfRegime> {
    if snapshots.is_empty() {
        return None;
    }

    // Accumulate weighted votes per regime kind.
    let mut regime_scores: HashMap<RegimeKind, f64> = HashMap::new();
    let mut total_weight = 0.0_f64;

    for (interval, snap) in snapshots {
        let w = weights.get(interval.as_str()).copied().unwrap_or(0.0);
        if w <= 0.0 {
            continue;
        }
        let effective = w * snap.confidence as f64;
        *regime_scores.entry(snap.kind).or_default() += effective;
        total_weight += w;
    }

    if total_weight <= 0.0 {
        return None;
    }

    // Dominant regime = highest weighted score.
    let (dominant, best_score) = regime_scores
        .iter()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))?;
    let dominant = *dominant;

    // Confluence score: fraction of total weight that agrees with dominant.
    let confluence_score = (best_score / total_weight).clamp(0.0, 1.0);

    // Transitioning: more than one distinct regime present.
    let distinct_regimes = regime_scores.keys().count();
    let is_transitioning = distinct_regimes > 1 && confluence_score < 0.7;

    Some(MultiTfRegime {
        symbol: symbol.to_string(),
        snapshots: snapshots.to_vec(),
        dominant_regime: dominant,
        confluence_score,
        is_transitioning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use qtss_domain::v2::regime::TrendStrength;
    use rust_decimal::Decimal;

    fn snap(kind: RegimeKind, confidence: f32) -> RegimeSnapshot {
        RegimeSnapshot {
            at: Utc::now(),
            kind,
            trend_strength: TrendStrength::None,
            adx: Decimal::ZERO,
            bb_width: Decimal::ZERO,
            atr_pct: Decimal::ZERO,
            choppiness: Decimal::ZERO,
            confidence,
        }
    }

    #[test]
    fn all_agree_high_confluence() {
        let snaps = vec![
            ("1h".into(), snap(RegimeKind::TrendingUp, 0.8)),
            ("4h".into(), snap(RegimeKind::TrendingUp, 0.9)),
            ("1d".into(), snap(RegimeKind::TrendingUp, 0.7)),
        ];
        let w = default_tf_weights();
        let r = compute_confluence("BTCUSDT", &snaps, &w).unwrap();
        assert_eq!(r.dominant_regime, RegimeKind::TrendingUp);
        assert!(r.confluence_score > 0.7);
        assert!(!r.is_transitioning);
    }

    #[test]
    fn mixed_low_confluence() {
        let snaps = vec![
            ("1h".into(), snap(RegimeKind::TrendingUp, 0.7)),
            ("4h".into(), snap(RegimeKind::Ranging, 0.6)),
            ("1d".into(), snap(RegimeKind::TrendingDown, 0.8)),
        ];
        let w = default_tf_weights();
        let r = compute_confluence("BTCUSDT", &snaps, &w).unwrap();
        assert!(r.confluence_score < 0.7);
        assert!(r.is_transitioning);
    }
}

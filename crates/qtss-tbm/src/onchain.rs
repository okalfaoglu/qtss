//! Onchain Pillar — smart-money / flow / funding analysis.
//!
//! Two scoring paths, dispatched in one place (CLAUDE.md #1):
//!
//! 1. **Aggregate path** (Faz 7.7) — when an upstream onchain pipeline
//!    has already produced an aggregate score + direction (`v1` Hat A
//!    `onchain_signal_scores` row), we trust that and only check
//!    direction agreement with the TBM hypothesis. This avoids
//!    double-scoring the same raw metrics in two places.
//! 2. **Raw path** (legacy) — when no aggregate is available, fall
//!    back to the original 4-metric composition. Tests around the
//!    raw path stay green so old fixtures keep working.
//!
//! Direction reconciliation: if Hat A says `long` and the TBM caller
//! is searching for a bottom (`is_bottom_search=true`) the directions
//! agree → full credit. If they disagree the pillar score is
//! attenuated by a config-driven `conflict_weight_factor` rather than
//! zeroed out, because Hat A and TBM use different time scales and a
//! short-term clash can still carry useful information.

use crate::pillar::{PillarKind, PillarScore};

/// On-chain metric bundle. The two halves (aggregate vs raw) are
/// independent — populate whichever the upstream pipeline can supply.
#[derive(Debug, Clone, Default)]
pub struct OnchainMetrics {
    // ── Aggregate path (Faz 7.7) ────────────────────────────────────
    /// Pre-blended aggregate score in `0..1`. When `Some`, the raw
    /// fields are ignored and this drives the pillar.
    pub aggregate_score: Option<f64>,
    /// Confidence of the aggregate in `0..1`. Used as a multiplier
    /// on the final pillar weight.
    pub aggregate_confidence: Option<f64>,
    /// Aggregate direction string (`"long"` / `"short"` / `"neutral"`).
    /// Compared to the TBM caller's `is_bottom_search`.
    pub aggregate_direction: Option<String>,

    // ── Raw path (legacy 4-metric composition) ──────────────────────
    /// Smart money net flow (positive = into exchange = sell pressure).
    pub smart_money_net_flow: Option<f64>,
    /// Exchange netflow (positive = into exchange).
    pub exchange_netflow: Option<f64>,
    /// Whale transaction count (last 24h).
    pub whale_tx_count: Option<u32>,
    /// Funding rate (negative = short-heavy).
    pub funding_rate: Option<f64>,
}

/// Tunables for the aggregate-path scoring. Sourced from
/// `system_config` by the worker so the operator can rebalance
/// without a deploy (CLAUDE.md #2). Tests use [`OnchainTuning::default`].
#[derive(Debug, Clone, Copy)]
pub struct OnchainTuning {
    /// Pillar weight applied when the aggregate path lights up.
    pub pillar_weight: f64,
    /// Multiplier on the score when Hat A direction disagrees with
    /// the TBM hypothesis. Range `0..1`.
    pub conflict_weight_factor: f64,
}

impl Default for OnchainTuning {
    fn default() -> Self {
        Self { pillar_weight: 0.15, conflict_weight_factor: 0.3 }
    }
}

/// Provider contract — async because the real implementation hits
/// Postgres. Tests inject a synchronous stub.
#[async_trait::async_trait]
pub trait OnchainMetricsProvider: Send + Sync {
    async fn fetch(&self, symbol: &str) -> Option<OnchainMetrics>;
}

/// Direction parser — single source of truth so the dispatch table
/// below stays one match arm long (CLAUDE.md #1).
fn direction_agrees(direction: Option<&str>, is_bottom: bool) -> Option<bool> {
    let d = direction?.trim().to_ascii_lowercase();
    match d.as_str() {
        "long" | "bull" | "bullish" | "bottom" => Some(is_bottom),
        "short" | "bear" | "bearish" | "top" => Some(!is_bottom),
        _ => None,
    }
}

/// Single dispatch point: aggregate path first, then raw fallback.
#[must_use]
pub fn score_onchain_with_tuning(
    metrics: &OnchainMetrics,
    is_bottom_search: bool,
    tuning: OnchainTuning,
) -> PillarScore {
    if metrics.aggregate_score.is_some() {
        return score_aggregate_path(metrics, is_bottom_search, tuning);
    }
    score_raw_path(metrics, is_bottom_search)
}

/// Backwards-compatible wrapper using default tuning. Existing tests
/// and call sites that don't care about tuning keep working.
#[must_use]
pub fn score_onchain(metrics: &OnchainMetrics, is_bottom_search: bool) -> PillarScore {
    score_onchain_with_tuning(metrics, is_bottom_search, OnchainTuning::default())
}

fn score_aggregate_path(
    metrics: &OnchainMetrics,
    is_bottom: bool,
    tuning: OnchainTuning,
) -> PillarScore {
    let agg = metrics.aggregate_score.unwrap_or(0.0).clamp(0.0, 1.0);
    let confidence = metrics
        .aggregate_confidence
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    let agreement = direction_agrees(metrics.aggregate_direction.as_deref(), is_bottom);

    // Score is the aggregate scaled to 0..100. Weight is the configured
    // pillar weight, attenuated when the direction conflicts.
    let score = agg * 100.0;
    let mut details = Vec::new();
    let mut weight = tuning.pillar_weight * confidence;

    match agreement {
        Some(true) => details.push(format!(
            "aggregate {:.2} agrees with {} hypothesis",
            agg,
            if is_bottom { "bottom" } else { "top" }
        )),
        Some(false) => {
            weight *= tuning.conflict_weight_factor;
            details.push(format!(
                "aggregate {:.2} CONFLICTS with {} hypothesis (weight × {:.2})",
                agg,
                if is_bottom { "bottom" } else { "top" },
                tuning.conflict_weight_factor
            ));
        }
        None => details.push("aggregate direction unknown — neutral".into()),
    }

    if score < 0.5 {
        // Effectively no signal — drop weight so this pillar doesn't
        // sit in the denominator dragging the total toward zero.
        weight = 0.0;
        details.push("aggregate score ~0 — pillar muted".into());
    }

    PillarScore { kind: PillarKind::Onchain, score, weight, details }
}

fn score_raw_path(metrics: &OnchainMetrics, is_bottom_search: bool) -> PillarScore {
    let mut score = 0.0_f64;
    let mut details = Vec::new();
    let mut has_data = false;

    // 1) Smart money flow (max 30)
    if let Some(flow) = metrics.smart_money_net_flow {
        has_data = true;
        if is_bottom_search && flow < 0.0 {
            score += 30.0;
            details.push(format!("Smart money outflow (accumulation) flow={flow:.0}"));
        } else if !is_bottom_search && flow > 0.0 {
            score += 30.0;
            details.push(format!("Smart money inflow (distribution) flow={flow:.0}"));
        }
    }

    // 2) Exchange netflow (max 25)
    if let Some(nf) = metrics.exchange_netflow {
        has_data = true;
        if is_bottom_search && nf < 0.0 {
            score += 25.0;
            details.push("Exchange outflow (bullish)".into());
        } else if !is_bottom_search && nf > 0.0 {
            score += 25.0;
            details.push("Exchange inflow (bearish)".into());
        }
    }

    // 3) Funding rate (max 25)
    if let Some(fr) = metrics.funding_rate {
        has_data = true;
        if is_bottom_search && fr < -0.01 {
            score += 25.0;
            details.push(format!("Negative funding {fr:.4} (over-shorted)"));
        } else if !is_bottom_search && fr > 0.01 {
            score += 25.0;
            details.push(format!("High funding {fr:.4} (over-leveraged longs)"));
        }
    }

    // 4) Whale activity (max 20)
    if let Some(wc) = metrics.whale_tx_count {
        has_data = true;
        if wc > 50 {
            score += 20.0;
            details.push(format!("High whale activity: {wc} txns"));
        } else if wc > 20 {
            score += 10.0;
            details.push(format!("Moderate whale activity: {wc} txns"));
        }
    }

    if !has_data {
        details.push("No on-chain data available".into());
    }

    PillarScore {
        kind: PillarKind::Onchain,
        score: score.min(100.0),
        weight: if has_data { 0.15 } else { 0.0 },
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_data_returns_zero_weight() {
        let s = score_onchain(&OnchainMetrics::default(), true);
        assert_eq!(s.weight, 0.0);
    }

    #[test]
    fn raw_accumulation_signal() {
        let m = OnchainMetrics {
            smart_money_net_flow: Some(-500.0),
            exchange_netflow: Some(-1000.0),
            funding_rate: Some(-0.02),
            whale_tx_count: Some(60),
            ..Default::default()
        };
        let s = score_onchain(&m, true);
        assert!(s.score >= 80.0);
        assert_eq!(s.weight, 0.15);
    }

    #[test]
    fn aggregate_path_agreement_full_credit() {
        let m = OnchainMetrics {
            aggregate_score: Some(0.8),
            aggregate_confidence: Some(1.0),
            aggregate_direction: Some("long".into()),
            ..Default::default()
        };
        let s = score_onchain(&m, true);
        assert!((s.score - 80.0).abs() < 1e-6);
        assert!((s.weight - 0.15).abs() < 1e-6);
    }

    #[test]
    fn aggregate_path_conflict_attenuates_weight() {
        let m = OnchainMetrics {
            aggregate_score: Some(0.8),
            aggregate_confidence: Some(1.0),
            aggregate_direction: Some("short".into()),
            ..Default::default()
        };
        let s = score_onchain(&m, true);
        // Weight: 0.15 * 0.3 (default conflict factor) = 0.045
        assert!((s.weight - 0.045).abs() < 1e-6);
    }

    #[test]
    fn aggregate_path_low_score_mutes_pillar() {
        let m = OnchainMetrics {
            aggregate_score: Some(0.0),
            aggregate_confidence: Some(1.0),
            aggregate_direction: Some("long".into()),
            ..Default::default()
        };
        let s = score_onchain(&m, true);
        assert_eq!(s.weight, 0.0);
    }

    #[test]
    fn aggregate_path_unknown_direction_neutral() {
        let m = OnchainMetrics {
            aggregate_score: Some(0.6),
            aggregate_confidence: Some(0.5),
            aggregate_direction: Some("sideways".into()),
            ..Default::default()
        };
        let s = score_onchain(&m, true);
        // Confidence halves the weight: 0.15 * 0.5 = 0.075
        assert!((s.weight - 0.075).abs() < 1e-6);
        assert!((s.score - 60.0).abs() < 1e-6);
    }
}

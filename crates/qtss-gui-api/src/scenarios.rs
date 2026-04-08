//! `/v2/scenarios/{venue}/{symbol}` wire types -- Faz 5 Adim (c).
//!
//! The scenario tree panel shows branching outcomes the simulator
//! considers plausible from the current bar: bull / neutral / bear,
//! each carrying a trigger condition, probability mass, and a price
//! band the branch is expected to reach.
//!
//! These DTOs are deliberately decoupled from the engine that will
//! eventually produce them (`qtss-scenario-engine`, planned). Today
//! the API route ships a deterministic volatility-based stub that
//! satisfies the contract; when the engine lands, the route swaps in
//! its output without touching this module or the frontend.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Inclusive price band a scenario branch is expected to reach.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetBand {
    pub low: Decimal,
    pub high: Decimal,
}

/// One node in the scenario tree. The root is the "current state"
/// node; its `children` are the bull / neutral / bear branches.
/// Each child can recurse for follow-on scenarios (e.g. continuation
/// vs reversal once the first branch fires).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioNode {
    pub id: String,
    /// Short label the chart uses ("bull", "bear", "neutral",
    /// "continuation", "reversal", ...).
    pub label: String,
    /// Plain-language trigger condition shown on hover.
    pub trigger: String,
    /// Probability mass of this branch *given the parent fired*
    /// (so children at one level should sum to ~1.0).
    pub probability: Decimal,
    pub target_band: TargetBand,
    pub children: Vec<ScenarioNode>,
}

/// Whole `/v2/scenarios/...` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioTree {
    pub generated_at: DateTime<Utc>,
    pub venue: String,
    pub symbol: String,
    pub timeframe: String,
    /// How many bars forward the tree projects.
    pub horizon_bars: u32,
    /// The bar the tree was anchored on (last close).
    pub anchor_price: Decimal,
    pub root: ScenarioNode,
}

/// Pure builder used by the route handler. Takes the most recent
/// closes (chronological), an anchor close, and a horizon, and emits
/// a 3-branch (bull/neutral/bear) tree with continuation/reversal
/// children. Volatility is the stdev of log returns over the input.
///
/// This is a *placeholder* the route uses until `qtss-scenario-engine`
/// lands. Keeping it as a free function makes it easy to delete later
/// and swap in the real engine without touching DTOs or the route
/// shape.
pub fn build_volatility_tree(
    closes: &[Decimal],
    anchor: Decimal,
    horizon_bars: u32,
) -> ScenarioNode {
    let sigma = log_return_stdev(closes);
    // Expected drift over horizon (sqrt-time scaling, no mean
    // assumption -- the placeholder is symmetric on purpose).
    let drift = sigma * (horizon_bars as f64).sqrt();
    let bull_target = anchor * decimal_from_f64(1.0 + drift);
    let bear_target = anchor * decimal_from_f64(1.0 - drift);
    let half_band = anchor * decimal_from_f64(drift * 0.5);

    let bull = ScenarioNode {
        id: "bull".into(),
        label: "bull".into(),
        trigger: "Close > anchor + 0.5σ".into(),
        probability: decimal_from_f64(0.30),
        target_band: TargetBand {
            low: anchor + half_band,
            high: bull_target,
        },
        children: vec![
            child("bull/continuation", "continuation", "Bull breaks +1σ", 0.55, anchor + half_band, bull_target),
            child("bull/reversal", "reversal", "Bull stalls below band", 0.45, anchor, anchor + half_band),
        ],
    };
    let neutral = ScenarioNode {
        id: "neutral".into(),
        label: "neutral".into(),
        trigger: "|Close - anchor| < 0.5σ".into(),
        probability: decimal_from_f64(0.40),
        target_band: TargetBand {
            low: anchor - half_band,
            high: anchor + half_band,
        },
        children: Vec::new(),
    };
    let bear = ScenarioNode {
        id: "bear".into(),
        label: "bear".into(),
        trigger: "Close < anchor - 0.5σ".into(),
        probability: decimal_from_f64(0.30),
        target_band: TargetBand {
            low: bear_target,
            high: anchor - half_band,
        },
        children: vec![
            child("bear/continuation", "continuation", "Bear breaks -1σ", 0.55, bear_target, anchor - half_band),
            child("bear/reversal", "reversal", "Bear stalls above band", 0.45, anchor - half_band, anchor),
        ],
    };

    ScenarioNode {
        id: "root".into(),
        label: "root".into(),
        trigger: format!("anchor={anchor}, σ={sigma:.6}"),
        probability: Decimal::ONE,
        target_band: TargetBand {
            low: bear_target,
            high: bull_target,
        },
        children: vec![bull, neutral, bear],
    }
}

fn child(
    id: &str,
    label: &str,
    trigger: &str,
    p: f64,
    low: Decimal,
    high: Decimal,
) -> ScenarioNode {
    ScenarioNode {
        id: id.into(),
        label: label.into(),
        trigger: trigger.into(),
        probability: decimal_from_f64(p),
        target_band: TargetBand { low, high },
        children: Vec::new(),
    }
}

fn log_return_stdev(closes: &[Decimal]) -> f64 {
    if closes.len() < 2 {
        return 0.0;
    }
    let f: Vec<f64> = closes.iter().filter_map(decimal_to_f64).collect();
    if f.len() < 2 {
        return 0.0;
    }
    let returns: Vec<f64> = f
        .windows(2)
        .filter_map(|w| {
            if w[0] > 0.0 && w[1] > 0.0 {
                Some((w[1] / w[0]).ln())
            } else {
                None
            }
        })
        .collect();
    if returns.is_empty() {
        return 0.0;
    }
    let mean: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
    let var: f64 =
        returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    var.sqrt()
}

fn decimal_to_f64(d: &Decimal) -> Option<f64> {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64()
}

fn decimal_from_f64(f: f64) -> Decimal {
    use rust_decimal::prelude::FromPrimitive;
    Decimal::from_f64(f).unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn tree_has_three_top_level_branches() {
        let closes: Vec<Decimal> = (0..30).map(|i| dec!(100) + Decimal::from(i)).collect();
        let tree = build_volatility_tree(&closes, dec!(130), 10);
        assert_eq!(tree.children.len(), 3);
        let labels: Vec<&str> = tree.children.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"bull"));
        assert!(labels.contains(&"neutral"));
        assert!(labels.contains(&"bear"));
    }

    #[test]
    fn child_probabilities_sum_to_one_per_level() {
        let closes: Vec<Decimal> = vec![dec!(100), dec!(101), dec!(102), dec!(101), dec!(103)];
        let tree = build_volatility_tree(&closes, dec!(103), 5);
        let total: Decimal = tree.children.iter().map(|c| c.probability).sum();
        assert_eq!(total, dec!(1.00));
        for branch in &tree.children {
            if branch.children.is_empty() {
                continue;
            }
            let sub: Decimal = branch.children.iter().map(|c| c.probability).sum();
            assert_eq!(sub, dec!(1.00));
        }
    }

    #[test]
    fn flat_series_produces_degenerate_tree() {
        let closes: Vec<Decimal> = vec![dec!(100); 10];
        let tree = build_volatility_tree(&closes, dec!(100), 10);
        // Zero vol -> bull and bear collapse onto the anchor.
        let bull = tree.children.iter().find(|c| c.label == "bull").unwrap();
        assert_eq!(bull.target_band.high, dec!(100));
    }

    #[test]
    fn json_round_trip() {
        let closes: Vec<Decimal> = vec![dec!(100), dec!(102), dec!(101), dec!(103)];
        let tree = ScenarioTree {
            generated_at: Utc::now(),
            venue: "binance".into(),
            symbol: "BTCUSDT".into(),
            timeframe: "1h".into(),
            horizon_bars: 10,
            anchor_price: dec!(103),
            root: build_volatility_tree(&closes, dec!(103), 10),
        };
        let j = serde_json::to_string(&tree).unwrap();
        let back: ScenarioTree = serde_json::from_str(&j).unwrap();
        assert_eq!(back.root.children.len(), 3);
    }
}

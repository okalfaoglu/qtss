//! Equal Highs / Equal Lows detector.
//!
//! Equal levels are key liquidity zones — when multiple swing points
//! form at nearly the same price, buy/sell stops accumulate there.
//! Smart money targets these levels for stop hunts.

use serde::{Deserialize, Serialize};

use crate::OhlcBar;
use crate::helpers::{is_pivot_high, is_pivot_low};

// ── Config ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EqualLevelsConfig {
    /// Pivot fractal window.
    #[serde(default = "default_pivot_window")]
    pub pivot_window: usize,
    /// Max price difference (ATR fraction) to consider "equal".
    #[serde(default = "default_equal_tolerance_atr")]
    pub equal_tolerance_atr: f64,
    /// Minimum bar distance between equal pivots (avoid adjacent).
    #[serde(default = "default_min_bar_distance")]
    pub min_bar_distance: usize,
    /// Lookback window.
    #[serde(default = "default_scan_lookback")]
    pub scan_lookback: usize,
}

fn default_pivot_window() -> usize { 3 }
fn default_equal_tolerance_atr() -> f64 { 0.15 }
fn default_min_bar_distance() -> usize { 5 }
fn default_scan_lookback() -> usize { 100 }

impl Default for EqualLevelsConfig {
    fn default() -> Self {
        Self {
            pivot_window: default_pivot_window(),
            equal_tolerance_atr: default_equal_tolerance_atr(),
            min_bar_distance: default_min_bar_distance(),
            scan_lookback: default_scan_lookback(),
        }
    }
}

// ── Output ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EqualLevelMatch {
    /// "equal_highs" or "equal_lows"
    pub subkind: String,
    /// Average price level.
    pub level: f64,
    /// Number of equal pivots.
    pub count: usize,
    /// Bar indices of the equal pivots.
    pub pivot_bars: Vec<i64>,
    /// Price difference between the most extreme pair.
    pub max_diff: f64,
    /// Whether the last bar has approached this level (within 1 ATR).
    pub price_near: bool,
    /// Quality score 0.0–1.0.
    pub score: f64,
}

// ── Detection ───────────────────────────────────────────────────────

pub fn detect_equal_levels(
    bars: &[OhlcBar],
    atr_value: f64,
    cfg: &EqualLevelsConfig,
) -> Vec<EqualLevelMatch> {
    let n = bars.len();
    if n < 20 {
        return Vec::new();
    }

    let scan_start = n.saturating_sub(cfg.scan_lookback);
    let w = cfg.pivot_window.max(1);
    let tol = atr_value * cfg.equal_tolerance_atr;

    let mut highs: Vec<(usize, f64)> = Vec::new();
    let mut lows: Vec<(usize, f64)> = Vec::new();

    for i in scan_start..n {
        if is_pivot_high(bars, i, w) {
            highs.push((i, bars[i].high));
        }
        if is_pivot_low(bars, i, w) {
            lows.push((i, bars[i].low));
        }
    }

    let mut results = Vec::new();
    let last_close = bars[n - 1].close;

    // Find equal highs
    for group in find_equal_groups(&highs, tol, cfg.min_bar_distance) {
        if group.len() < 2 {
            continue;
        }
        let prices: Vec<f64> = group.iter().map(|&(_, p)| p).collect();
        let level = prices.iter().sum::<f64>() / prices.len() as f64;
        let max_diff = prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - prices.iter().cloned().fold(f64::INFINITY, f64::min);
        let pivot_bars: Vec<i64> = group.iter().map(|&(idx, _)| bars[idx].bar_index).collect();
        let price_near = (last_close - level).abs() <= atr_value;

        let score = equal_level_score(group.len(), max_diff, atr_value, price_near);

        results.push(EqualLevelMatch {
            subkind: "equal_highs".into(),
            level,
            count: group.len(),
            pivot_bars,
            max_diff,
            price_near,
            score,
        });
    }

    // Find equal lows
    for group in find_equal_groups(&lows, tol, cfg.min_bar_distance) {
        if group.len() < 2 {
            continue;
        }
        let prices: Vec<f64> = group.iter().map(|&(_, p)| p).collect();
        let level = prices.iter().sum::<f64>() / prices.len() as f64;
        let max_diff = prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - prices.iter().cloned().fold(f64::INFINITY, f64::min);
        let pivot_bars: Vec<i64> = group.iter().map(|&(idx, _)| bars[idx].bar_index).collect();
        let price_near = (last_close - level).abs() <= atr_value;

        let score = equal_level_score(group.len(), max_diff, atr_value, price_near);

        results.push(EqualLevelMatch {
            subkind: "equal_lows".into(),
            level,
            count: group.len(),
            pivot_bars,
            max_diff,
            price_near,
            score,
        });
    }

    results
}

fn find_equal_groups(pivots: &[(usize, f64)], tol: f64, min_bar_dist: usize) -> Vec<Vec<(usize, f64)>> {
    let mut used = vec![false; pivots.len()];
    let mut groups = Vec::new();

    for i in 0..pivots.len() {
        if used[i] {
            continue;
        }
        let mut group = vec![pivots[i]];
        used[i] = true;

        for j in (i + 1)..pivots.len() {
            if used[j] {
                continue;
            }
            let bar_dist = pivots[j].0.abs_diff(pivots[i].0);
            if bar_dist < min_bar_dist {
                continue;
            }
            if (pivots[j].1 - pivots[i].1).abs() <= tol {
                group.push(pivots[j]);
                used[j] = true;
            }
        }

        if group.len() >= 2 {
            groups.push(group);
        }
    }

    groups
}

fn equal_level_score(count: usize, max_diff: f64, atr: f64, price_near: bool) -> f64 {
    let count_score = ((count as f64 - 1.0) / 3.0).clamp(0.0, 0.35);
    let precision = if atr > 1e-12 { 1.0 - (max_diff / atr).clamp(0.0, 1.0) } else { 0.5 };
    let precision_score = precision * 0.35;
    let near_score = if price_near { 0.3 } else { 0.1 };
    (count_score + precision_score + near_score).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_equal_highs() {
        let pivots = vec![(5, 100.0), (15, 100.05), (25, 100.02)];
        let groups = find_equal_groups(&pivots, 0.5, 5);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 3);
    }

    #[test]
    fn respects_min_distance() {
        let pivots = vec![(5, 100.0), (7, 100.05)]; // too close
        let groups = find_equal_groups(&pivots, 0.5, 5);
        assert!(groups.is_empty());
    }
}

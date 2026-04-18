//! Liquidity Pool / Liquidity Sweep detector.
//!
//! Liquidity pools form at:
//! - **Equal Highs / Equal Lows** (clustered stop-losses)
//! - **Swing highs / Swing lows** (obvious support/resistance)
//! - **Round numbers** (psychological levels)
//!
//! A **Liquidity Sweep** occurs when price briefly pierces a pool and
//! reverses — stop-hunting before the real move.

use serde::{Deserialize, Serialize};

use crate::OhlcBar;
use crate::helpers::{is_pivot_high, is_pivot_low};

// ── Config ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityPoolConfig {
    /// Pivot fractal window for identifying swing points.
    #[serde(default = "default_pivot_window")]
    pub pivot_window: usize,
    /// ATR-based tolerance for clustering pivots into a pool.
    #[serde(default = "default_cluster_atr_mult")]
    pub cluster_atr_mult: f64,
    /// Minimum pivot touches to form a pool.
    #[serde(default = "default_min_touches")]
    pub min_touches: usize,
    /// Lookback for scanning.
    #[serde(default = "default_scan_lookback")]
    pub scan_lookback: usize,
    /// Max wick penetration (ATR fraction) for sweep classification.
    #[serde(default = "default_sweep_max_penetration_atr")]
    pub sweep_max_penetration_atr: f64,
}

fn default_pivot_window() -> usize { 3 }
fn default_cluster_atr_mult() -> f64 { 0.3 }
fn default_min_touches() -> usize { 2 }
fn default_scan_lookback() -> usize { 100 }
fn default_sweep_max_penetration_atr() -> f64 { 0.5 }

impl Default for LiquidityPoolConfig {
    fn default() -> Self {
        Self {
            pivot_window: default_pivot_window(),
            cluster_atr_mult: default_cluster_atr_mult(),
            min_touches: default_min_touches(),
            scan_lookback: default_scan_lookback(),
            sweep_max_penetration_atr: default_sweep_max_penetration_atr(),
        }
    }
}

// ── Output ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityPoolMatch {
    /// "liquidity_pool_high" or "liquidity_pool_low"
    pub subkind: String,
    /// Price level of the pool.
    pub level: f64,
    /// Number of pivot touches at this level.
    pub touches: usize,
    /// Bar indices of the contributing pivots.
    pub pivot_bars: Vec<i64>,
    /// Whether the last candle swept this pool.
    pub swept: bool,
    /// If swept: did price reclaim (close back inside)? → liquidity grab signal.
    pub reclaimed: bool,
    /// Quality score 0.0–1.0.
    pub score: f64,
}

// ── Detection ───────────────────────────────────────────────────────

pub fn detect_liquidity_pools(
    bars: &[OhlcBar],
    atr_value: f64,
    cfg: &LiquidityPoolConfig,
) -> Vec<LiquidityPoolMatch> {
    let n = bars.len();
    if n < cfg.scan_lookback.min(20) {
        return Vec::new();
    }

    let scan_start = n.saturating_sub(cfg.scan_lookback);
    let tol = atr_value * cfg.cluster_atr_mult;
    let w = cfg.pivot_window.max(1);

    // Collect pivot highs and lows
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

    // Cluster highs into pools
    let high_pools = cluster_levels(&highs, tol, cfg.min_touches);
    for (level, members) in &high_pools {
        let pivot_bars: Vec<i64> = members.iter().map(|&idx| bars[idx].bar_index).collect();
        let last = &bars[n - 1];
        let swept = last.high > *level;
        let reclaimed = swept && last.close < *level;
        let penetration = if swept { (last.high - level) / atr_value.max(1e-12) } else { 0.0 };
        let is_sweep = swept && penetration <= cfg.sweep_max_penetration_atr;

        let score = compute_pool_score(members.len(), is_sweep && reclaimed);

        results.push(LiquidityPoolMatch {
            subkind: "liquidity_pool_high".into(),
            level: *level,
            touches: members.len(),
            pivot_bars,
            swept: is_sweep,
            reclaimed: is_sweep && reclaimed,
            score,
        });
    }

    // Cluster lows into pools
    let low_pools = cluster_levels(&lows, tol, cfg.min_touches);
    for (level, members) in &low_pools {
        let pivot_bars: Vec<i64> = members.iter().map(|&idx| bars[idx].bar_index).collect();
        let last = &bars[n - 1];
        let swept = last.low < *level;
        let reclaimed = swept && last.close > *level;
        let penetration = if swept { (level - last.low) / atr_value.max(1e-12) } else { 0.0 };
        let is_sweep = swept && penetration <= cfg.sweep_max_penetration_atr;

        let score = compute_pool_score(members.len(), is_sweep && reclaimed);

        results.push(LiquidityPoolMatch {
            subkind: "liquidity_pool_low".into(),
            level: *level,
            touches: members.len(),
            pivot_bars,
            swept: is_sweep,
            reclaimed: is_sweep && reclaimed,
            score,
        });
    }

    results
}

/// Cluster price levels within tolerance. Returns (avg_level, member_indices).
fn cluster_levels(pivots: &[(usize, f64)], tol: f64, min_touches: usize) -> Vec<(f64, Vec<usize>)> {
    let mut used = vec![false; pivots.len()];
    let mut clusters = Vec::new();

    for i in 0..pivots.len() {
        if used[i] {
            continue;
        }
        let mut members = vec![pivots[i].0];
        let mut sum = pivots[i].1;
        used[i] = true;

        for j in (i + 1)..pivots.len() {
            if used[j] {
                continue;
            }
            if (pivots[j].1 - pivots[i].1).abs() <= tol {
                members.push(pivots[j].0);
                sum += pivots[j].1;
                used[j] = true;
            }
        }

        if members.len() >= min_touches {
            let avg = sum / members.len() as f64;
            clusters.push((avg, members));
        }
    }

    clusters
}

fn compute_pool_score(touches: usize, sweep_reclaim: bool) -> f64 {
    let touch_score = ((touches as f64 - 1.0) / 4.0).clamp(0.0, 0.4);
    let sweep_score = if sweep_reclaim { 0.4 } else { 0.0 };
    let base = 0.2;
    (base + touch_score + sweep_score).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    fn bar(i: i64, o: f64, h: f64, l: f64, c: f64) -> OhlcBar {
        OhlcBar { open: o, high: h, low: l, close: c, bar_index: i, volume: Some(1000.0) }
    }

    #[test]
    fn clusters_equal_lows() {
        let pivots = vec![(5, 100.0), (10, 100.1), (15, 100.05)];
        let clusters = cluster_levels(&pivots, 0.5, 2);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].1.len(), 3);
    }
}

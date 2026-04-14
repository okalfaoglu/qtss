//! Range-based detectors: Trading Range, FVG, Order Block, Liquidity Pool, Equal Highs/Lows.
//!
//! Architecture follows the EventSpec dispatch table pattern (same as `qtss-wyckoff`).
//! Each detector is a pure function: `bars + ATR + config → Vec<Match>`.
//! The orchestrator wraps results into `Detection` objects.

mod ohlc;
pub mod helpers;
pub mod fvg;
pub mod order_block;
pub mod liquidity_pool;
pub mod equal_levels;

pub use ohlc::OhlcBar;
pub use fvg::{detect_fvg, FvgConfig, FvgMatch};
pub use order_block::{detect_order_blocks, OrderBlockConfig, OrderBlockMatch};
pub use liquidity_pool::{detect_liquidity_pools, LiquidityPoolConfig, LiquidityPoolMatch};
pub use equal_levels::{detect_equal_levels, EqualLevelsConfig, EqualLevelMatch};

use serde::{Deserialize, Serialize};

// ── Unified config ──────────────────────────────────────────────────

/// Unified range detector configuration. Each sub-detector can be independently enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeDetectorConfig {
    #[serde(default = "default_true")]
    pub fvg_enabled: bool,
    #[serde(default)]
    pub fvg: FvgConfig,

    #[serde(default = "default_true")]
    pub order_block_enabled: bool,
    #[serde(default)]
    pub order_block: OrderBlockConfig,

    #[serde(default = "default_true")]
    pub liquidity_pool_enabled: bool,
    #[serde(default)]
    pub liquidity_pool: LiquidityPoolConfig,

    #[serde(default = "default_true")]
    pub equal_levels_enabled: bool,
    #[serde(default)]
    pub equal_levels: EqualLevelsConfig,

    /// P18 — global quality floor applied after all sub-detectors run.
    /// Zones with score below this are dropped before reaching the
    /// chart/outbox. 0.5 keeps mid/high-quality zones, drops noise.
    /// Config key: `range.min_score`.
    #[serde(default = "default_min_score")]
    pub min_score: f64,
}

fn default_true() -> bool { true }
fn default_min_score() -> f64 { 0.5 }

impl Default for RangeDetectorConfig {
    fn default() -> Self {
        Self {
            fvg_enabled: true,
            fvg: FvgConfig::default(),
            order_block_enabled: true,
            order_block: OrderBlockConfig::default(),
            liquidity_pool_enabled: true,
            liquidity_pool: LiquidityPoolConfig::default(),
            equal_levels_enabled: true,
            equal_levels: EqualLevelsConfig::default(),
            min_score: default_min_score(),
        }
    }
}

// ── Unified match ───────────────────────────────────────────────────

/// A range detection result — unified envelope for all sub-detectors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeMatch {
    /// Sub-kind string: "bullish_fvg", "bearish_ob", "liquidity_pool_low", "equal_highs", etc.
    pub subkind: String,
    /// Bar index where the pattern was detected.
    pub bar_index: i64,
    /// Upper price level of the zone/gap/block.
    pub zone_high: f64,
    /// Lower price level.
    pub zone_low: f64,
    /// Quality score 0.0–1.0.
    pub score: f64,
    /// Full detector-specific metadata (serialized FvgMatch / OrderBlockMatch / etc.)
    pub meta: serde_json::Value,
}

// ── Dispatch ────────────────────────────────────────────────────────

/// Run all enabled range sub-detectors and return unified results.
pub fn detect_all(bars: &[OhlcBar], atr_value: f64, cfg: &RangeDetectorConfig) -> Vec<RangeMatch> {
    let mut results = Vec::new();

    if cfg.fvg_enabled {
        for m in detect_fvg(bars, atr_value, &cfg.fvg) {
            results.push(RangeMatch {
                subkind: m.subkind.clone(),
                bar_index: m.bar_index,
                zone_high: m.gap_high,
                zone_low: m.gap_low,
                score: m.score as f64,
                meta: serde_json::to_value(&m).unwrap_or_default(),
            });
        }
    }

    if cfg.order_block_enabled {
        for m in detect_order_blocks(bars, atr_value, &cfg.order_block) {
            results.push(RangeMatch {
                subkind: m.subkind.clone(),
                bar_index: m.bar_index,
                zone_high: m.ob_high,
                zone_low: m.ob_low,
                score: m.score as f64,
                meta: serde_json::to_value(&m).unwrap_or_default(),
            });
        }
    }

    if cfg.liquidity_pool_enabled {
        for m in detect_liquidity_pools(bars, atr_value, &cfg.liquidity_pool) {
            let half_atr = atr_value * 0.25;
            results.push(RangeMatch {
                subkind: m.subkind.clone(),
                bar_index: *m.pivot_bars.last().unwrap_or(&0),
                zone_high: m.level + half_atr,
                zone_low: m.level - half_atr,
                score: m.score as f64,
                meta: serde_json::to_value(&m).unwrap_or_default(),
            });
        }
    }

    if cfg.equal_levels_enabled {
        for m in detect_equal_levels(bars, atr_value, &cfg.equal_levels) {
            let half_atr = atr_value * 0.25;
            results.push(RangeMatch {
                subkind: m.subkind.clone(),
                bar_index: *m.pivot_bars.last().unwrap_or(&0),
                zone_high: m.level + half_atr,
                zone_low: m.level - half_atr,
                score: m.score as f64,
                meta: serde_json::to_value(&m).unwrap_or_default(),
            });
        }
    }

    // P18 — quality floor: drop zones below global min_score.
    results.retain(|m| m.score >= cfg.min_score);

    // Sort by score descending
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results
}

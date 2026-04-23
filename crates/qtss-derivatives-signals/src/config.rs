//! Threshold configuration for derivatives detectors. Every knob is
//! seeded from `system_config` at call-site (CLAUDE.md #2).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivConfig {
    /// Minimum score 0..1 for any detector to publish.
    pub min_score: f32,

    /// Funding-rate Z-score threshold. |z| >= this ⇒ spike. Default 2.0
    /// (2σ). On a low-volatility day z=2 ≈ 0.01% per 8h (≈0.03%/day).
    pub funding_z_threshold: f64,
    /// Rolling window for the funding-rate baseline (count of prior
    /// funding periods). 21 = ~7 days at 8h cycles.
    pub funding_window: usize,

    /// OI change threshold as a fraction of 24h-ago OI. 0.05 = 5%.
    pub oi_delta_pct: f64,
    /// Price-divergence floor for an OI imbalance to count. If price
    /// moved > this fraction in the OI-delta window with OI flat (or
    /// opposite), emit an event. 0.02 = 2%.
    pub oi_price_divergence_pct: f64,
    /// Lookback in hours for OI baseline.
    pub oi_window_hours: i64,

    /// Basis dislocation threshold as a fraction of index price. 0.001
    /// = 10 bps. Above this the perp is trading meaningfully different
    /// from spot.
    pub basis_dislocation_pct: f64,

    /// Long/short ratio extremes (crowded-trade markers). Long >
    /// `lsr_long_extreme` ⇒ over-long (contrarian short signal); Short >
    /// `lsr_short_extreme` ⇒ over-short (contrarian long signal).
    pub lsr_long_extreme: f64,
    pub lsr_short_extreme: f64,

    /// Taker buy/sell ratio extremes. Buy > `taker_buy_dominance`
    /// triggers a bull taker-flow; Sell > `taker_sell_dominance` a
    /// bear. Defaults 1.4 / 1.4 — 40% tilt either direction.
    pub taker_buy_dominance: f64,
    pub taker_sell_dominance: f64,
}

impl Default for DerivConfig {
    fn default() -> Self {
        Self {
            min_score: 0.55,
            funding_z_threshold: 2.0,
            funding_window: 21,
            oi_delta_pct: 0.05,
            oi_price_divergence_pct: 0.02,
            oi_window_hours: 24,
            basis_dislocation_pct: 0.001,
            lsr_long_extreme: 2.5,
            lsr_short_extreme: 2.5,
            taker_buy_dominance: 1.4,
            taker_sell_dominance: 1.4,
        }
    }
}

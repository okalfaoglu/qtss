//! Order-flow detector thresholds.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderFlowConfig {
    pub min_score: f32,

    /// Liquidation cluster — minimum number of liquidation events in
    /// the window for a cluster to qualify. Binance 1h window typically
    /// has 0-5 liqs on quiet days; 20+ marks a flash-crash-style flush.
    pub liq_cluster_min_count: usize,
    /// Minimum total notional ($) for the cluster.
    pub liq_cluster_min_notional_usd: f64,
    /// Window length in seconds. The onchain poller usually writes
    /// 1h windows, so 3600 is the useful ceiling.
    pub liq_cluster_window_secs: i64,

    /// Single block trade threshold (notional in USD). Bitcoin
    /// whale-watching convention: 500K for BTC, 100K for alts.
    pub block_trade_notional_usd: f64,

    /// CVD divergence — bar lookback for price-vs-CVD comparison.
    pub cvd_divergence_bars: usize,
    /// Minimum price move (fraction) over the lookback for a
    /// divergence to matter. Without this every tiny drift triggers.
    pub cvd_divergence_price_min_pct: f64,
    /// Minimum CVD counter-move (fraction of window abs-sum) to
    /// qualify as "flat or declining against the price".
    pub cvd_divergence_cvd_opposite_min: f64,
}

impl Default for OrderFlowConfig {
    fn default() -> Self {
        Self {
            min_score: 0.55,
            liq_cluster_min_count: 15,
            liq_cluster_min_notional_usd: 250_000.0,
            liq_cluster_window_secs: 3600,
            block_trade_notional_usd: 500_000.0,
            cvd_divergence_bars: 12,
            cvd_divergence_price_min_pct: 0.01,
            cvd_divergence_cvd_opposite_min: 0.15,
        }
    }
}

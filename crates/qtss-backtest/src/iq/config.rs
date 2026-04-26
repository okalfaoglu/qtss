//! Backtest configuration — every knob the operator can tune lives
//! here. Defaults mirror the live worker's `system_config` rows so a
//! fresh backtest reproduces the live pipeline byte-for-byte; CLI
//! flags / config files override individual fields.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What kind of setup are we backtesting?  Locked to the user's
/// IQ-D / IQ-T pipeline. The variant decides which composite scorer
/// and which gates apply per bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IqPolarity {
    /// IQ-D — long bias on a major dip candidate.
    Dip,
    /// IQ-T — short bias on a major top candidate.
    Top,
}

/// Composite weights snapshot — all 10 channels of the major-dip /
/// major-top scorer. Backtest can sweep this struct via grid /
/// walk-forward optimisation. Defaults match `Weights::defaults()`
/// in `major_dip_candidate_loop.rs` (FAZ 25.4.G rebalance).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositeWeights {
    pub structural: f64,
    pub fib_retrace: f64,
    pub volume_capit: f64,
    pub cvd_divergence: f64,
    pub indicator: f64,
    pub sentiment: f64,
    pub multi_tf: f64,
    pub funding_oi: f64,
    pub wyckoff_alignment: f64,
    pub cycle_alignment: f64,
}

impl Default for CompositeWeights {
    fn default() -> Self {
        Self {
            structural: 0.16,
            fib_retrace: 0.12,
            volume_capit: 0.12,
            cvd_divergence: 0.07,
            indicator: 0.07,
            sentiment: 0.07,
            multi_tf: 0.07,
            funding_oi: 0.08,
            wyckoff_alignment: 0.14,
            cycle_alignment: 0.10,
        }
    }
}

impl CompositeWeights {
    /// Sums every weight; the live scorer expects ~1.0. Backtest
    /// can renormalise via [`Self::normalised`] after a sweep.
    pub fn sum(&self) -> f64 {
        self.structural
            + self.fib_retrace
            + self.volume_capit
            + self.cvd_divergence
            + self.indicator
            + self.sentiment
            + self.multi_tf
            + self.funding_oi
            + self.wyckoff_alignment
            + self.cycle_alignment
    }

    /// Returns a copy with weights scaled to sum to `target`. Used
    /// by grid-search where individual knobs vary independently and
    /// the resulting sum drifts away from 1.0.
    pub fn normalised(&self, target: f64) -> Self {
        let s = self.sum();
        if s == 0.0 {
            return self.clone();
        }
        let k = target / s;
        Self {
            structural: self.structural * k,
            fib_retrace: self.fib_retrace * k,
            volume_capit: self.volume_capit * k,
            cvd_divergence: self.cvd_divergence * k,
            indicator: self.indicator * k,
            sentiment: self.sentiment * k,
            multi_tf: self.multi_tf * k,
            funding_oi: self.funding_oi * k,
            wyckoff_alignment: self.wyckoff_alignment * k,
            cycle_alignment: self.cycle_alignment * k,
        }
    }
}

/// Setup-creation gates. Mirrors the live `iq_d_candidate` /
/// `iq_t_candidate` config keys so backtests can probe the impact
/// of each gate independently (e.g. "what if we required Wyckoff
/// alignment a year ago — would PnL be better?").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IqGates {
    pub min_dip_score: f64,
    pub require_wyckoff_alignment: bool,
    pub require_cycle_alignment: bool,
    /// Composite score threshold below which no setup is opened.
    /// Live default: 0.55 ("developing").
    pub min_composite: f64,
}

impl Default for IqGates {
    fn default() -> Self {
        Self {
            min_dip_score: 0.0,
            require_wyckoff_alignment: true,
            require_cycle_alignment: false,
            min_composite: 0.55,
        }
    }
}

/// Risk + sizing rules — kept intentionally minimal for v1. Backtest
/// uses `risk_per_trade_pct` of starting equity for each entry; TP
/// ladders + SL come from the setup itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskRules {
    pub starting_equity: Decimal,
    pub risk_per_trade_pct: f64,
    pub max_concurrent_trades: u32,
    /// Bars to keep a position open before forced exit (timeout).
    pub max_holding_bars: u32,
    /// Optional ATR-based trailing stop multiplier. `None` = no trail.
    pub trailing_stop_atr_mult: Option<f64>,
}

impl Default for RiskRules {
    fn default() -> Self {
        Self {
            starting_equity: Decimal::from(10_000),
            risk_per_trade_pct: 0.01,
            max_concurrent_trades: 5,
            max_holding_bars: 200,
            trailing_stop_atr_mult: None,
        }
    }
}

/// Symbol + timeframe + time window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IqBacktestUniverse {
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub timeframe: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
}

/// Full backtest config bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IqBacktestConfig {
    pub universe: IqBacktestUniverse,
    pub polarity: IqPolarity,
    pub weights: CompositeWeights,
    pub gates: IqGates,
    pub risk: RiskRules,
    /// File path for per-trade JSONL log. `None` = no file output
    /// (still in-memory aggregation for the report).
    pub trade_log_path: Option<PathBuf>,
    /// Snapshot the path of an open trade every N bars (for path
    /// analysis in attribution). 0 = no snapshots.
    pub path_snapshot_every_bars: u32,
    /// Tag stamped into every trade row — useful for grouping
    /// optimisation runs ("weight_grid_run_42").
    pub run_tag: String,
}

impl Default for IqBacktestConfig {
    fn default() -> Self {
        Self {
            universe: IqBacktestUniverse {
                exchange: "binance".into(),
                segment: "futures".into(),
                symbol: "BTCUSDT".into(),
                timeframe: "4h".into(),
                start_time: Utc::now() - chrono::Duration::days(365),
                end_time: Utc::now(),
            },
            polarity: IqPolarity::Dip,
            weights: CompositeWeights::default(),
            gates: IqGates::default(),
            risk: RiskRules::default(),
            trade_log_path: None,
            path_snapshot_every_bars: 5,
            run_tag: "default".into(),
        }
    }
}

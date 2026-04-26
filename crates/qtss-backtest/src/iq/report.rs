//! In-memory aggregate report — what the runner returns when it's
//! done. Designed for two consumers:
//!
//!   1. CLI: pretty-printed summary (win rate, sharpe, drawdown,
//!      loss-reason histogram).
//!   2. Optimisation: a single scalar `score()` for grid / Bayesian
//!      search to maximise.
//!
//! For deeper analysis the JSONL trade log is the source of truth;
//! this report is the concise snapshot.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::attribution::{OutcomeAttribution, OutcomeClass};
use super::availability::DataAvailabilityReport;
use super::config::IqBacktestConfig;
use super::trade::IqTrade;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IqBacktestReport {
    pub config: IqBacktestConfig,
    pub bars_processed: u64,

    // ── Trade counts ──────────────────────────────────────────────
    pub total_trades: u64,
    pub wins: u64,
    pub losses: u64,
    pub scratches: u64,
    pub aborted: u64,
    pub open_at_end: u64,

    // ── PnL ───────────────────────────────────────────────────────
    pub gross_pnl: Decimal,
    pub net_pnl: Decimal,
    pub starting_equity: Decimal,
    pub final_equity: Decimal,
    pub peak_equity: Decimal,
    pub max_drawdown_pct: f64,

    // ── Stats ─────────────────────────────────────────────────────
    pub win_rate: f64,
    pub avg_win_pct: f64,
    pub avg_loss_pct: f64,
    pub profit_factor: f64,
    pub expectancy_pct: f64,
    pub sharpe_ratio: Option<f64>,

    // ── Loss-reason histogram (user spec) ─────────────────────────
    pub loss_reason_counts: BTreeMap<String, u64>,

    // ── Component-weakness on losers ──────────────────────────────
    /// "Which channel was the weakest at entry on average across
    /// losing trades?" — pandas-friendly.
    pub avg_loss_components: BTreeMap<String, f64>,

    // ── BUG BACKTEST — pre-flight data availability snapshot ─────
    /// Per-channel data coverage probed at the START of the run.
    /// `None` when the runner did not probe (older snapshots).
    /// When this shows `missing`/`empty` rows, the corresponding
    /// scorer returned 0 for every bar in the window — usually the
    /// reason a run produces 0 trades despite a populated tape.
    #[serde(default)]
    pub data_availability: Option<DataAvailabilityReport>,
}

impl IqBacktestReport {
    /// Score the run with a single scalar — used by optimisation.
    /// Default scoring: net PnL minus max drawdown penalty. Reward
    /// runs with high net PnL and low DD.
    pub fn score(&self) -> f64 {
        let net = self.net_pnl.to_f64().unwrap_or(0.0);
        // Penalty: 2x max DD. Tuned so a 50% DD run with $1k profit
        // scores worse than a 5% DD run with $500 profit.
        net - 2.0 * self.max_drawdown_pct.abs() * self.starting_equity.to_f64().unwrap_or(1.0) / 100.0
    }

    /// Empty report with the config attached. Used as the seed
    /// when the runner starts.
    pub fn seed(config: IqBacktestConfig) -> Self {
        let starting_equity = config.risk.starting_equity;
        Self {
            config,
            bars_processed: 0,
            total_trades: 0,
            wins: 0,
            losses: 0,
            scratches: 0,
            aborted: 0,
            open_at_end: 0,
            gross_pnl: Decimal::ZERO,
            net_pnl: Decimal::ZERO,
            starting_equity,
            final_equity: starting_equity,
            peak_equity: starting_equity,
            max_drawdown_pct: 0.0,
            win_rate: 0.0,
            avg_win_pct: 0.0,
            avg_loss_pct: 0.0,
            profit_factor: 0.0,
            expectancy_pct: 0.0,
            sharpe_ratio: None,
            loss_reason_counts: BTreeMap::new(),
            avg_loss_components: BTreeMap::new(),
            data_availability: None,
        }
    }
}

/// Build a report by aggregating the supplied closed trades plus
/// per-trade attributions. The runner calls this once at the end of
/// a run.
pub fn aggregate(
    config: IqBacktestConfig,
    bars_processed: u64,
    trades: &[(IqTrade, OutcomeAttribution)],
) -> IqBacktestReport {
    let mut report = IqBacktestReport::seed(config);
    report.bars_processed = bars_processed;

    let mut net_run = report.starting_equity;
    let mut peak = report.starting_equity;
    let mut max_dd = 0.0_f64;
    let mut wins_pct: Vec<f64> = Vec::new();
    let mut losses_pct: Vec<f64> = Vec::new();
    let mut comp_sums: BTreeMap<String, f64> = BTreeMap::new();
    let mut comp_counts: BTreeMap<String, u64> = BTreeMap::new();

    for (t, attr) in trades {
        report.total_trades += 1;
        report.gross_pnl += t.gross_pnl;
        report.net_pnl += t.net_pnl;
        net_run += t.net_pnl;
        if net_run > peak {
            peak = net_run;
        }
        let dd_pct = ((peak - net_run) / peak).to_f64().unwrap_or(0.0) * 100.0;
        if dd_pct > max_dd {
            max_dd = dd_pct;
        }

        match attr.class {
            OutcomeClass::Win => {
                report.wins += 1;
                wins_pct.push(t.net_pnl_pct);
            }
            OutcomeClass::Loss => {
                report.losses += 1;
                losses_pct.push(t.net_pnl_pct);
                if let Some(reason) = attr.loss_reason {
                    let key = format!("{reason:?}");
                    *report.loss_reason_counts.entry(key).or_insert(0) += 1;
                }
                // Component averages on losers.
                if let Some(obj) = t.entry_components.as_object() {
                    for (k, v) in obj {
                        if let Some(score) = v.as_f64() {
                            *comp_sums.entry(k.clone()).or_insert(0.0) += score;
                            *comp_counts.entry(k.clone()).or_insert(0) += 1;
                        }
                    }
                }
            }
            OutcomeClass::Scratch => report.scratches += 1,
            OutcomeClass::Aborted => report.aborted += 1,
            OutcomeClass::OpenAtEnd => report.open_at_end += 1,
        }
    }

    report.final_equity = net_run;
    report.peak_equity = peak;
    report.max_drawdown_pct = max_dd;

    let resolved = report.wins + report.losses;
    report.win_rate = if resolved > 0 {
        report.wins as f64 / resolved as f64
    } else {
        0.0
    };
    report.avg_win_pct = avg(&wins_pct);
    report.avg_loss_pct = avg(&losses_pct);
    let total_wins: f64 = wins_pct.iter().sum();
    let total_losses_abs: f64 = losses_pct.iter().map(|x| x.abs()).sum();
    report.profit_factor = if total_losses_abs > 0.0 {
        total_wins / total_losses_abs
    } else if total_wins > 0.0 {
        f64::INFINITY
    } else {
        0.0
    };
    report.expectancy_pct =
        report.win_rate * report.avg_win_pct
            + (1.0 - report.win_rate) * report.avg_loss_pct;
    report.sharpe_ratio = sharpe(&wins_pct, &losses_pct);

    for (k, sum) in comp_sums {
        let n = *comp_counts.get(&k).unwrap_or(&1) as f64;
        report
            .avg_loss_components
            .insert(k, if n > 0.0 { sum / n } else { 0.0 });
    }
    report
}

fn avg(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn sharpe(wins: &[f64], losses: &[f64]) -> Option<f64> {
    let mut all: Vec<f64> = wins.iter().chain(losses.iter()).copied().collect();
    if all.len() < 2 {
        return None;
    }
    let mean = all.iter().sum::<f64>() / all.len() as f64;
    let variance =
        all.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / all.len() as f64;
    let stddev = variance.sqrt();
    // We don't know the bar cadence here so we don't annualise —
    // raw per-trade Sharpe is fine for relative comparison across
    // optimisation runs. Report is a vehicle for ranking weights,
    // not absolute risk-adjusted return.
    all.clear();
    if stddev == 0.0 {
        None
    } else {
        Some(mean / stddev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iq::attribution::classify;
    use crate::iq::config::IqPolarity;
    use crate::iq::trade::{TradeOutcome, TradeState};
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn closed(pnl_pct: f64, outcome: TradeOutcome) -> IqTrade {
        let mut t = IqTrade::pending(
            "test",
            IqPolarity::Dip,
            "BTCUSDT",
            "4h",
            "binance",
            "futures",
            100,
            Utc::now(),
            dec!(50000),
            dec!(48000),
            vec![dec!(52000)],
            dec!(0.1),
            json!({"structural_completion": 0.7, "fib_retrace_quality": 0.3}),
            0.7,
        );
        t.state = TradeState::Closed;
        t.outcome = Some(outcome);
        t.net_pnl_pct = pnl_pct;
        t.net_pnl = Decimal::from((pnl_pct * 10.0) as i64);
        t.gross_pnl = t.net_pnl;
        t
    }

    #[test]
    fn aggregate_counts_each_class() {
        let cfg = IqBacktestConfig::default();
        let trades = vec![
            closed(2.0, TradeOutcome::TakeProfitFull),
            closed(-1.5, TradeOutcome::StopLoss),
            closed(-1.0, TradeOutcome::StopLoss),
            closed(0.001, TradeOutcome::StopLoss), // scratch
        ];
        let with_attr: Vec<_> =
            trades.into_iter().map(|t| { let a = classify(&t); (t, a) }).collect();
        let report = aggregate(cfg, 1000, &with_attr);
        assert_eq!(report.total_trades, 4);
        assert_eq!(report.wins, 1);
        assert_eq!(report.losses, 2);
        assert_eq!(report.scratches, 1);
        assert!((report.win_rate - 1.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn loss_reason_histogram_built() {
        let cfg = IqBacktestConfig::default();
        let trades = vec![
            closed(-1.0, TradeOutcome::StopLoss),
            closed(-1.0, TradeOutcome::StopLoss),
            closed(-0.5, TradeOutcome::Timeout),
        ];
        let with_attr: Vec<_> =
            trades.into_iter().map(|t| { let a = classify(&t); (t, a) }).collect();
        let report = aggregate(cfg, 100, &with_attr);
        let total_losses: u64 = report.loss_reason_counts.values().sum();
        assert_eq!(total_losses, 3);
    }

    #[test]
    fn avg_loss_components_includes_weakest_channel() {
        let cfg = IqBacktestConfig::default();
        let trades =
            vec![closed(-1.0, TradeOutcome::StopLoss); 3];
        let with_attr: Vec<_> =
            trades.into_iter().map(|t| { let a = classify(&t); (t, a) }).collect();
        let report = aggregate(cfg, 100, &with_attr);
        assert!(report.avg_loss_components.contains_key("structural_completion"));
        assert!(report.avg_loss_components.contains_key("fib_retrace_quality"));
        let fib = *report.avg_loss_components.get("fib_retrace_quality").unwrap();
        assert!((fib - 0.3).abs() < 0.01);
    }
}

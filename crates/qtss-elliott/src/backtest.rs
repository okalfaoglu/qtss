//! Elliott Wave backtest — historical pattern performance analysis.
//!
//! Simulates pattern detection on historical bars and measures:
//!   - Win rate (patterns that reached target)
//!   - Loss rate (patterns invalidated before target)
//!   - Average risk/reward ratio
//!   - Max consecutive losses

use crate::corrective::CorrectiveWave;
use crate::invalidation::{check_corrective_invalid, check_motive_invalid, corrective_invalidation_level, motive_invalidation_level};
use crate::motive::MotiveWave;
use crate::targets::{corrective_primary_target, motive_primary_target, project_corrective_targets, project_motive_targets};
use qtss_domain::v2::bar::Bar;
use std::collections::VecDeque;

/// Trade outcome from a detected pattern.
#[derive(Debug, Clone, Copy)]
pub enum TradeOutcome {
    /// Pattern reached target price before invalidation.
    Win(f64), // profit in pips/points
    /// Pattern invalidated before reaching target.
    Loss(f64), // loss in pips/points
    /// Still in progress (pattern not yet resolved).
    Open,
}

/// Single pattern trade record.
#[derive(Debug, Clone)]
pub struct PatternTrade {
    pub pattern_type: &'static str,      // "motive" or "corrective"
    pub direction: i8,                   // 1=bullish, -1=bearish
    pub entry_price: f64,
    pub target_price: f64,
    pub stop_price: f64,
    pub outcome: TradeOutcome,
    pub bars_to_resolve: usize,
}

/// Backtest statistics.
#[derive(Debug, Clone)]
pub struct BacktestStats {
    pub total_patterns: usize,
    pub wins: usize,
    pub losses: usize,
    pub open: usize,
    pub win_rate: f64,
    pub avg_profit: f64,
    pub avg_loss: f64,
    pub profit_factor: f64, // total_profit / total_loss
    pub max_consecutive_losses: usize,
    pub avg_bars_to_win: f64,
    pub avg_bars_to_loss: f64,
}

/// Simulates a motive pattern trade on historical bars.
pub fn simulate_motive_trade(
    motive: &MotiveWave,
    entry_bar_idx: usize,
    bars: &[Bar],
) -> PatternTrade {
    let entry = bars[entry_bar_idx].close.to_f64().unwrap_or(0.0);
    let targets = project_motive_targets(motive);
    let target = motive_primary_target(&targets);
    let stop = motive_invalidation_level(motive);

    let mut outcome = TradeOutcome::Open;
    let mut bars_to_resolve = bars.len() - entry_bar_idx;

    // Simulate bars forward from entry.
    for (i, bar) in bars[entry_bar_idx + 1..].iter().enumerate() {
        let high = bar.high.to_f64().unwrap_or(0.0);
        let low = bar.low.to_f64().unwrap_or(0.0);

        // Check target hit.
        if motive.direction > 0 {
            if high >= target {
                outcome = TradeOutcome::Win((target - entry).abs());
                bars_to_resolve = i + 1;
                break;
            }
            // Check stop hit.
            if low <= stop {
                outcome = TradeOutcome::Loss((stop - entry).abs());
                bars_to_resolve = i + 1;
                break;
            }
        } else {
            if low <= target {
                outcome = TradeOutcome::Win((entry - target).abs());
                bars_to_resolve = i + 1;
                break;
            }
            if high >= stop {
                outcome = TradeOutcome::Loss((entry - stop).abs());
                bars_to_resolve = i + 1;
                break;
            }
        }
    }

    PatternTrade {
        pattern_type: "motive",
        direction: motive.direction,
        entry_price: entry,
        target_price: target,
        stop_price: stop,
        outcome,
        bars_to_resolve,
    }
}

/// Simulates a corrective pattern trade on historical bars.
pub fn simulate_corrective_trade(
    corr: &CorrectiveWave,
    entry_bar_idx: usize,
    bars: &[Bar],
) -> PatternTrade {
    let entry = bars[entry_bar_idx].close.to_f64().unwrap_or(0.0);
    let targets = project_corrective_targets(corr);
    let target = corrective_primary_target(&targets);
    let stop = corrective_invalidation_level(corr);

    let mut outcome = TradeOutcome::Open;
    let mut bars_to_resolve = bars.len() - entry_bar_idx;

    // Simulate bars forward from entry.
    for (i, bar) in bars[entry_bar_idx + 1..].iter().enumerate() {
        let high = bar.high.to_f64().unwrap_or(0.0);
        let low = bar.low.to_f64().unwrap_or(0.0);

        // Check target hit.
        if corr.direction > 0 {
            if high >= target {
                outcome = TradeOutcome::Win((target - entry).abs());
                bars_to_resolve = i + 1;
                break;
            }
            if low <= stop {
                outcome = TradeOutcome::Loss((entry - stop).abs());
                bars_to_resolve = i + 1;
                break;
            }
        } else {
            if low <= target {
                outcome = TradeOutcome::Win((entry - target).abs());
                bars_to_resolve = i + 1;
                break;
            }
            if high >= stop {
                outcome = TradeOutcome::Loss((entry - stop).abs());
                bars_to_resolve = i + 1;
                break;
            }
        }
    }

    PatternTrade {
        pattern_type: "corrective",
        direction: corr.direction,
        entry_price: entry,
        target_price: target,
        stop_price: stop,
        outcome,
        bars_to_resolve,
    }
}

/// Calculate backtest statistics from trade list.
pub fn calculate_stats(trades: &[PatternTrade]) -> BacktestStats {
    if trades.is_empty() {
        return BacktestStats {
            total_patterns: 0,
            wins: 0,
            losses: 0,
            open: 0,
            win_rate: 0.0,
            avg_profit: 0.0,
            avg_loss: 0.0,
            profit_factor: 0.0,
            max_consecutive_losses: 0,
            avg_bars_to_win: 0.0,
            avg_bars_to_loss: 0.0,
        };
    }

    let mut wins = 0;
    let mut losses = 0;
    let mut open = 0;
    let mut total_profit = 0.0;
    let mut total_loss = 0.0;
    let mut bars_to_wins = VecDeque::new();
    let mut bars_to_losses = VecDeque::new();
    let mut consecutive_losses = 0;
    let mut max_consecutive_losses = 0;

    for trade in trades {
        match trade.outcome {
            TradeOutcome::Win(profit) => {
                wins += 1;
                total_profit += profit;
                bars_to_wins.push_back(trade.bars_to_resolve as f64);
                consecutive_losses = 0;
            }
            TradeOutcome::Loss(loss) => {
                losses += 1;
                total_loss += loss;
                bars_to_losses.push_back(trade.bars_to_resolve as f64);
                consecutive_losses += 1;
                max_consecutive_losses = max_consecutive_losses.max(consecutive_losses);
            }
            TradeOutcome::Open => {
                open += 1;
            }
        }
    }

    let total = trades.len();
    let win_rate = if total > 0 { wins as f64 / total as f64 } else { 0.0 };
    let avg_profit = if wins > 0 { total_profit / wins as f64 } else { 0.0 };
    let avg_loss = if losses > 0 { total_loss / losses as f64 } else { 0.0 };
    let profit_factor = if total_loss > 0.0 {
        total_profit / total_loss
    } else {
        0.0
    };
    let avg_bars_to_win = if bars_to_wins.is_empty() {
        0.0
    } else {
        bars_to_wins.iter().sum::<f64>() / bars_to_wins.len() as f64
    };
    let avg_bars_to_loss = if bars_to_losses.is_empty() {
        0.0
    } else {
        bars_to_losses.iter().sum::<f64>() / bars_to_losses.len() as f64
    };

    BacktestStats {
        total_patterns: total,
        wins,
        losses,
        open,
        win_rate,
        avg_profit,
        avg_loss,
        profit_factor,
        max_consecutive_losses,
        avg_bars_to_win,
        avg_bars_to_loss,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zigzag::ZigZagPoint;
    use chrono::Utc;
    use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
    use qtss_domain::v2::timeframe::Timeframe;
    use rust_decimal_macros::dec;

    fn mock_bar(h: f64, l: f64, c: f64) -> Bar {
        Bar {
            instrument: Instrument {
                symbol: "TEST".to_string(),
                venue: Venue::Binance,
                asset_class: AssetClass::Spot,
                quote_asset: "USDT".to_string(),
                session_calendar: SessionCalendar::Crypto24h,
            },
            timeframe: Timeframe::M1,
            open_time: Utc::now(),
            open: dec!(c),
            high: dec!(h),
            low: dec!(l),
            close: dec!(c),
            volume: dec!(100),
            closed: true,
        }
    }

    fn mock_point(bars_ago: usize, price: f64, direction: i8) -> ZigZagPoint {
        ZigZagPoint {
            bars_ago,
            price,
            direction,
        }
    }

    #[test]
    fn test_simulate_motive_win() {
        let motive = MotiveWave {
            points: [
                mock_point(4, 100.0, 1),
                mock_point(3, 95.0, -1),
                mock_point(2, 110.0, 1),
                mock_point(1, 105.0, -1),
                mock_point(0, 115.0, 1),
            ],
            direction: 1,
            score: 0.7,
        };

        let bars = vec![
            mock_bar(115.5, 114.5, 115.0), // Entry
            mock_bar(117.0, 116.0, 116.5), // Moving toward target
            mock_bar(120.0, 119.0, 119.5), // Target hit
        ];

        let trade = simulate_motive_trade(&motive, 0, &bars);
        assert!(matches!(trade.outcome, TradeOutcome::Win(_)));
    }

    #[test]
    fn test_calculate_stats() {
        let trades = vec![
            PatternTrade {
                pattern_type: "motive",
                direction: 1,
                entry_price: 100.0,
                target_price: 110.0,
                stop_price: 95.0,
                outcome: TradeOutcome::Win(10.0),
                bars_to_resolve: 5,
            },
            PatternTrade {
                pattern_type: "motive",
                direction: 1,
                entry_price: 105.0,
                target_price: 115.0,
                stop_price: 100.0,
                outcome: TradeOutcome::Loss(5.0),
                bars_to_resolve: 3,
            },
        ];

        let stats = calculate_stats(&trades);
        assert_eq!(stats.total_patterns, 2);
        assert_eq!(stats.wins, 1);
        assert_eq!(stats.losses, 1);
        assert_eq!(stats.win_rate, 0.5);
        assert!(stats.profit_factor > 1.0);
    }
}

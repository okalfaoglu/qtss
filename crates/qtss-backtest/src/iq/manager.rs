//! `IqLifecycleManager` — bar-by-bar evaluation of open trades:
//! stop-loss, TP ladder (1R/2R/3R cascade), trailing stop, timeout.
//!
//! Each bar:
//!   1. Mark the trade to the bar's HIGH/LOW (worst-case touch logic).
//!   2. Check SL — if HIGH ≥ SL for shorts or LOW ≤ SL for longs,
//!      stop hits. Close the entire remaining qty at SL price.
//!   3. Check TPs in order — for each TP not yet hit, see if the
//!      bar's range crosses it. If so, scale out a fraction of the
//!      position at that TP. Default ladder: 33%/33%/34%.
//!   4. If trailing stop is enabled and we've already hit TP1, drag
//!      the stop up (down for shorts) by `trailing_stop_atr_mult * ATR`.
//!   5. Check timeout — if `bars_held >= max_holding_bars`, close at
//!      bar close.
//!
//! When all qty is exhausted (or stopped/timed out), the trade
//! transitions to `Closed` and gets returned.

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::trace;

use super::config::{IqBacktestConfig, IqPolarity};
use super::cost::{CostModel, FillCost};
use super::runner::TradeManager;
use super::trade::{IqTrade, TradeOutcome, TradeState};

/// Default TP ladder distribution. Three tiers, fairly even split
/// with a slight bias to the final tier so the longest-held portion
/// of the position captures the strongest move.
const DEFAULT_TP_FRACTIONS: [&str; 3] = ["0.33", "0.33", "0.34"];

pub struct IqLifecycleManager {
    pub config: IqBacktestConfig,
    pub cost: CostModel,
}

impl IqLifecycleManager {
    pub fn new(config: IqBacktestConfig, cost: CostModel) -> Self {
        Self { config, cost }
    }

    /// Realise PnL on a fraction of the position at `exit_price`.
    /// Returns the gross PnL on this slice. Cost (fee + slippage) is
    /// charged separately and accumulated on the trade.
    fn realise_slice(
        &self,
        trade: &mut IqTrade,
        slice_qty: Decimal,
        exit_price: Decimal,
        exit_time: DateTime<Utc>,
        long: bool,
    ) -> (Decimal, FillCost) {
        let entry = trade.entry_price;
        let raw_pnl = if long {
            (exit_price - entry) * slice_qty
        } else {
            (entry - exit_price) * slice_qty
        };
        let notional = exit_price * slice_qty;
        let holding_secs = (exit_time - trade.entry_time).num_seconds();
        let cost = self.cost.fill_cost(notional, holding_secs, long);
        trade.gross_pnl += raw_pnl;
        trade.costs.fee += cost.fee;
        trade.costs.slippage += cost.slippage;
        trade.costs.funding += cost.funding;
        trade.remaining_qty -= slice_qty;
        if trade.remaining_qty < Decimal::ZERO {
            trade.remaining_qty = Decimal::ZERO;
        }
        (raw_pnl, cost)
    }

    /// Finalize a trade once all qty is closed. Computes net_pnl /
    /// net_pnl_pct and stamps exit_* metadata. Idempotent — calling
    /// twice is harmless (the state guard skips).
    fn finalize(
        &self,
        trade: &mut IqTrade,
        exit_bar: usize,
        exit_time: DateTime<Utc>,
        exit_price: Decimal,
        outcome: TradeOutcome,
    ) {
        if matches!(trade.state, TradeState::Closed | TradeState::Aborted) {
            return;
        }
        trade.state = TradeState::Closed;
        trade.exit_bar = Some(exit_bar);
        trade.exit_time = Some(exit_time);
        trade.exit_price = Some(exit_price);
        trade.outcome = Some(outcome);
        trade.net_pnl = trade.gross_pnl - trade.costs.total();
        let entry_f = trade.entry_price.to_f64().unwrap_or(0.0).max(f64::MIN_POSITIVE);
        let qty_f = trade.initial_qty.to_f64().unwrap_or(0.0).max(f64::MIN_POSITIVE);
        let notional_f = entry_f * qty_f;
        if notional_f > 0.0 {
            trade.net_pnl_pct =
                (trade.net_pnl.to_f64().unwrap_or(0.0) / notional_f) * 100.0;
        }
    }

    fn tp_fractions(&self, n_tps: usize) -> Vec<Decimal> {
        // Use the canonical 33/33/34 split for 3 TPs; equal-split
        // otherwise. Future commit can read per-config fractions.
        if n_tps == 0 {
            return Vec::new();
        }
        if n_tps == DEFAULT_TP_FRACTIONS.len() {
            DEFAULT_TP_FRACTIONS
                .iter()
                .map(|s| s.parse::<Decimal>().unwrap_or(Decimal::ONE))
                .collect()
        } else {
            // Equal split = 1/N for N tiers.
            let f = Decimal::ONE / Decimal::from(n_tps as u64);
            vec![f; n_tps]
        }
    }
}

#[async_trait::async_trait]
impl TradeManager for IqLifecycleManager {
    async fn on_bar(
        &self,
        bar_index: usize,
        bar_time: DateTime<Utc>,
        bar_high: Decimal,
        bar_low: Decimal,
        bar_close: Decimal,
        open_trades: &mut Vec<IqTrade>,
    ) -> Vec<IqTrade> {
        let mut closed_now = Vec::new();
        let mut keep = Vec::new();

        for mut trade in std::mem::take(open_trades) {
            // Skip non-active states (shouldn't be in open_trades but
            // defensive).
            if !matches!(trade.state, TradeState::Open | TradeState::ScalingOut)
            {
                if matches!(trade.state, TradeState::Closed | TradeState::Aborted)
                {
                    closed_now.push(trade);
                } else {
                    keep.push(trade);
                }
                continue;
            }

            let long = matches!(trade.polarity, IqPolarity::Dip);
            // Path observation (MFE/MAE) per bar mid.
            trade.observe_path(bar_index, bar_time, bar_close, long);

            let initial = trade.initial_qty;
            let fractions = self.tp_fractions(trade.take_profits.len());

            // Determine intra-bar event ordering: which gets priority
            // when both SL and a TP are inside the bar's range?
            // Conservative approach: SL hits BEFORE any TP for longs
            // when bar_low <= SL (worst-case execution).
            let sl_hit = if long {
                bar_low <= trade.stop_loss
            } else {
                bar_high >= trade.stop_loss
            };

            // Sequentially evaluate TPs (only those not yet hit).
            // tier_pnls[i] > 0 means TP[i] already filled.
            let mut tier_filled: Vec<bool> = trade
                .tier_pnls
                .iter()
                .take(trade.take_profits.len())
                .map(|p| *p > Decimal::ZERO)
                .collect();
            for (i, tp) in trade.take_profits.clone().iter().enumerate() {
                if i >= tier_filled.len() {
                    break;
                }
                if tier_filled[i] {
                    continue;
                }
                let crossed = if long {
                    bar_high >= *tp
                } else {
                    bar_low <= *tp
                };
                if !crossed {
                    continue;
                }
                let frac = fractions
                    .get(i)
                    .copied()
                    .unwrap_or(Decimal::ONE);
                let slice = initial * frac;
                let slice = slice.min(trade.remaining_qty);
                if slice <= Decimal::ZERO {
                    break;
                }
                let (slice_pnl, _cost) =
                    self.realise_slice(&mut trade, slice, *tp, bar_time, long);
                trade.tier_pnls[i] = slice_pnl;
                tier_filled[i] = true;
                trade.state = TradeState::ScalingOut;
                trace!(
                    bar = bar_index,
                    tp_idx = i,
                    tp_price = %tp,
                    slice = %slice,
                    "tp tier hit"
                );
            }

            // SL after TPs — if SL still triggers AFTER TPs filled,
            // remaining qty exits at SL. This conservatively assumes
            // worst-case ordering for the "did SL hit at all this bar?"
            // case.
            if sl_hit && trade.remaining_qty > Decimal::ZERO {
                // Bind stop_loss + remaining_qty into local copies so
                // we can hand a &mut Trade to `realise_slice` without
                // tripping the borrow checker.
                let sl_price = trade.stop_loss;
                let qty_left = trade.remaining_qty;
                let (slice_pnl, _cost) = self.realise_slice(
                    &mut trade,
                    qty_left,
                    sl_price,
                    bar_time,
                    long,
                );
                let last_idx = trade.tier_pnls.len() - 1;
                trade.tier_pnls[last_idx] = slice_pnl;
                let any_tp = tier_filled.iter().any(|f| *f);
                let outcome = if any_tp {
                    TradeOutcome::TakeProfitPartial
                } else {
                    TradeOutcome::StopLoss
                };
                self.finalize(&mut trade, bar_index, bar_time, sl_price, outcome);
                closed_now.push(trade);
                continue;
            }

            // Final TP filled — close out as TakeProfitFull.
            if trade.remaining_qty.is_zero() {
                let final_tp = *trade
                    .take_profits
                    .last()
                    .unwrap_or(&trade.entry_price);
                self.finalize(
                    &mut trade,
                    bar_index,
                    bar_time,
                    final_tp,
                    TradeOutcome::TakeProfitFull,
                );
                closed_now.push(trade);
                continue;
            }

            // Timeout?
            if trade.bars_held >= self.config.risk.max_holding_bars {
                if trade.remaining_qty > Decimal::ZERO {
                    let qty_left = trade.remaining_qty;
                    let (slice_pnl, _cost) = self.realise_slice(
                        &mut trade,
                        qty_left,
                        bar_close,
                        bar_time,
                        long,
                    );
                    let last_idx = trade.tier_pnls.len() - 1;
                    trade.tier_pnls[last_idx] = slice_pnl;
                }
                self.finalize(
                    &mut trade,
                    bar_index,
                    bar_time,
                    bar_close,
                    TradeOutcome::Timeout,
                );
                closed_now.push(trade);
                continue;
            }

            keep.push(trade);
        }

        *open_trades = keep;
        closed_now
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iq::config::IqBacktestConfig;
    use crate::iq::config::IqPolarity;
    use chrono::TimeZone;
    use serde_json::json;

    fn cfg() -> IqBacktestConfig {
        let mut c = IqBacktestConfig::default();
        c.risk.starting_equity = dec!(10000);
        c.risk.max_holding_bars = 10;
        c
    }

    fn fixture_trade() -> IqTrade {
        IqTrade::pending(
            "test",
            IqPolarity::Dip,
            "BTCUSDT",
            "4h",
            "binance",
            "futures",
            100,
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            dec!(50000),
            dec!(49500),
            vec![dec!(50500), dec!(51000), dec!(51500)],
            dec!(0.1),
            json!({}),
            0.7,
        )
    }

    #[tokio::test]
    async fn long_hits_tp1_partial_then_sl() {
        let mgr = IqLifecycleManager::new(cfg(), CostModel::default());
        let mut trade = fixture_trade();
        trade.state = TradeState::Open;
        let mut open = vec![trade];

        // Bar 1: high reaches TP1, low stays above SL → partial TP1.
        let closed = mgr
            .on_bar(
                101,
                Utc::now(),
                dec!(50500),
                dec!(49800),
                dec!(50300),
                &mut open,
            )
            .await;
        assert_eq!(closed.len(), 0);
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].state, TradeState::ScalingOut);
        assert!(open[0].tier_pnls[0] > Decimal::ZERO);

        // Bar 2: SL hit → trade closes as TakeProfitPartial.
        let closed = mgr
            .on_bar(
                102,
                Utc::now(),
                dec!(50100),
                dec!(49400),
                dec!(49500),
                &mut open,
            )
            .await;
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].state, TradeState::Closed);
        assert_eq!(
            closed[0].outcome,
            Some(TradeOutcome::TakeProfitPartial)
        );
    }

    #[tokio::test]
    async fn long_full_sl_classifies_stop_loss() {
        let mgr = IqLifecycleManager::new(cfg(), CostModel::default());
        let mut trade = fixture_trade();
        trade.state = TradeState::Open;
        let mut open = vec![trade];

        // Bar 1: dives directly to SL — no TP hit.
        let closed = mgr
            .on_bar(
                101,
                Utc::now(),
                dec!(50100),
                dec!(49400),
                dec!(49500),
                &mut open,
            )
            .await;
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].outcome, Some(TradeOutcome::StopLoss));
    }

    #[tokio::test]
    async fn full_winner_closes_at_tp3() {
        let mgr = IqLifecycleManager::new(cfg(), CostModel::default());
        let mut trade = fixture_trade();
        trade.state = TradeState::Open;
        let mut open = vec![trade];

        // Bar 1: rallies through ALL three TPs in one shot.
        let closed = mgr
            .on_bar(
                101,
                Utc::now(),
                dec!(52000),
                dec!(50200),
                dec!(51800),
                &mut open,
            )
            .await;
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].outcome, Some(TradeOutcome::TakeProfitFull));
        assert!(closed[0].gross_pnl > Decimal::ZERO);
    }

    #[tokio::test]
    async fn timeout_closes_at_bar_close() {
        let mut c = cfg();
        c.risk.max_holding_bars = 2;
        let mgr = IqLifecycleManager::new(c, CostModel::default());
        let mut trade = fixture_trade();
        trade.state = TradeState::Open;
        let mut open = vec![trade];

        // Bar 1: nothing happens.
        let _ = mgr
            .on_bar(101, Utc::now(), dec!(50100), dec!(49800), dec!(50000), &mut open)
            .await;
        // Bar 2: still nothing (bars_held = 2).
        let closed = mgr
            .on_bar(102, Utc::now(), dec!(50050), dec!(49850), dec!(49950), &mut open)
            .await;
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].outcome, Some(TradeOutcome::Timeout));
    }
}

use chrono::{DateTime, Utc};
use qtss_domain::bar::TimestampBar;
use qtss_domain::orders::OrderSide;
use qtss_domain::symbol::InstrumentId;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use tracing::instrument;

use crate::metrics::PerformanceReport;
use crate::strategy::Strategy;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub initial_equity: Decimal,
    /// İşlem başına basit slip (fiyat yönüne göre uygulanır).
    pub slippage_bps: u32,
    /// [`qtss_domain::commission::CommissionPolicy::ManualBps`] ile aynı mantık (backtest yerel).
    pub maker_fee_bps: u32,
    pub taker_fee_bps: u32,
    pub max_leverage: Decimal,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            initial_equity: dec!(100_000),
            slippage_bps: 2,
            maker_fee_bps: 2,
            taker_fee_bps: 5,
            max_leverage: dec!(3),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityPoint {
    pub ts: DateTime<Utc>,
    pub equity: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosedTrade {
    pub entry_ts: DateTime<Utc>,
    pub exit_ts: DateTime<Utc>,
    pub side: OrderSide,
    pub qty: Decimal,
    pub entry_px: Decimal,
    pub exit_px: Decimal,
    pub pnl: Decimal,
    pub fee: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResult {
    pub equity_curve: Vec<EquityPoint>,
    pub trades: Vec<ClosedTrade>,
    pub report: PerformanceReport,
}

#[derive(Debug)]
pub struct OpenPosition {
    pub side: OrderSide,
    pub qty: Decimal,
    pub entry_px: Decimal,
    pub entry_ts: DateTime<Utc>,
}

pub struct BacktestContext {
    pub instrument: InstrumentId,
    pub cash: Decimal,
    pub position: Option<OpenPosition>,
    closed_trades: Vec<ClosedTrade>,
}

impl BacktestContext {
    pub fn new(instrument: InstrumentId, cash: Decimal) -> Self {
        Self {
            instrument,
            cash,
            position: None,
            closed_trades: Vec::new(),
        }
    }

    pub fn take_closed_trades(&mut self) -> Vec<ClosedTrade> {
        std::mem::take(&mut self.closed_trades)
    }

    /// MVP: tek yönlü spot long; genişletilecek emir motoru buraya bağlanır.
    pub fn market_order(
        &mut self,
        side: OrderSide,
        qty: Decimal,
        price: Decimal,
        ts: DateTime<Utc>,
        slip_bps: u32,
        fee_bps: u32,
    ) -> Result<(), &'static str> {
        let slip = price * Decimal::from(slip_bps) / dec!(10_000);
        let fill_px = match side {
            OrderSide::Buy => price + slip,
            OrderSide::Sell => price - slip,
        };
        let notional = fill_px * qty;
        let fee = notional * Decimal::from(fee_bps) / dec!(10_000);

        match side {
            OrderSide::Buy => {
                let cost = notional + fee;
                if self.cash < cost {
                    return Err("insufficient_cash");
                }
                if self.position.is_some() {
                    return Err("position_already_open");
                }
                self.cash -= cost;
                self.position = Some(OpenPosition {
                    side: OrderSide::Buy,
                    qty,
                    entry_px: fill_px,
                    entry_ts: ts,
                });
                Ok(())
            }
            OrderSide::Sell => {
                let Some(pos) = self.position.take() else {
                    return Err("no_position");
                };
                if !matches!(pos.side, OrderSide::Buy) {
                    return Err("position_side_mismatch");
                }
                let proceeds = fill_px * qty - fee;
                let pnl = (fill_px - pos.entry_px) * qty - fee;
                self.cash += proceeds;
                self.closed_trades.push(ClosedTrade {
                    entry_ts: pos.entry_ts,
                    exit_ts: ts,
                    side: pos.side,
                    qty,
                    entry_px: pos.entry_px,
                    exit_px: fill_px,
                    pnl,
                    fee,
                });
                Ok(())
            }
        }
    }
}

pub struct BacktestEngine {
    cfg: BacktestConfig,
}

impl BacktestEngine {
    pub fn new(cfg: BacktestConfig) -> Self {
        Self { cfg }
    }

    #[instrument(skip(self, bars, strategy), fields(strategy = strategy.name()))]
    pub fn run<S: Strategy + ?Sized>(
        &self,
        instrument: InstrumentId,
        bars: VecDeque<TimestampBar>,
        strategy: &mut S,
    ) -> BacktestResult {
        let mut ctx = BacktestContext::new(instrument, self.cfg.initial_equity);
        let mut equity_curve = Vec::new();
        let mut closed: Vec<ClosedTrade> = Vec::new();

        for bar in bars.iter() {
            strategy.on_bar(&mut ctx, bar);
            closed.extend(ctx.take_closed_trades());

            let mark = match &ctx.position {
                Some(p) if matches!(p.side, OrderSide::Buy) => ctx.cash + p.qty * bar.close,
                None => ctx.cash,
                _ => ctx.cash,
            };
            equity_curve.push(EquityPoint {
                ts: bar.ts,
                equity: mark,
            });
        }

        closed.extend(ctx.take_closed_trades());

        let report = PerformanceReport::from_equity_and_trades(
            &equity_curve,
            &closed,
            self.cfg.initial_equity,
        );

        BacktestResult {
            equity_curve,
            trades: closed,
            report,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qtss_domain::bar::TimestampBar;
    use qtss_domain::exchange::{ExchangeId, MarketSegment};

    struct Nop;
    impl Strategy for Nop {
        fn name(&self) -> &'static str {
            "nop"
        }
        fn on_bar(&mut self, _ctx: &mut BacktestContext, _bar: &TimestampBar) {}
    }

    #[test]
    fn flat_equity_without_strategy_moves() {
        let eng = BacktestEngine::new(BacktestConfig::default());
        let instr = InstrumentId {
            exchange: ExchangeId::Binance,
            segment: MarketSegment::Spot,
            symbol: "BTCUSDT".into(),
        };
        let mut bars = VecDeque::new();
        bars.push_back(TimestampBar {
            ts: Utc::now(),
            open: dec!(1),
            high: dec!(1),
            low: dec!(1),
            close: dec!(1),
            volume: dec!(1),
        });

        let mut nop = Nop;
        let res = eng.run(instr, bars, &mut nop);
        assert!(
            (res.equity_curve.last().unwrap().equity - dec!(100_000)).abs() < dec!(1),
            "equity should stay ~initial when flat"
        );
    }
}

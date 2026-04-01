use chrono::{DateTime, Utc};
use qtss_domain::bar::TimestampBar;
use qtss_domain::exchange::MarketSegment;
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
    /// Linear futures: isolated-style initial margin = notional / leverage. Spot için 1 kabul edilir.
    pub max_leverage: Decimal,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            initial_equity: dec!(100_000),
            slippage_bps: 2,
            maker_fee_bps: 2,
            taker_fee_bps: 5,
            max_leverage: dec!(5),
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
    /// Açılışta ödenen komisyon (kapanışta `ClosedTrade.fee` = entry + exit).
    pub entry_fee: Decimal,
    /// Linear futures isolated margin; spot için sıfır.
    pub initial_margin: Decimal,
}

pub struct BacktestContext {
    pub instrument: InstrumentId,
    pub cash: Decimal,
    pub position: Option<OpenPosition>,
    closed_trades: Vec<ClosedTrade>,
    pub slippage_bps: u32,
    pub taker_fee_bps: u32,
    /// Spot: 1; linear futures: `BacktestConfig::max_leverage` (clamp edilmiş).
    pub leverage: Decimal,
}

impl BacktestContext {
    pub fn new(
        instrument: InstrumentId,
        cash: Decimal,
        slippage_bps: u32,
        taker_fee_bps: u32,
        leverage: Decimal,
    ) -> Self {
        Self {
            instrument,
            cash,
            position: None,
            closed_trades: Vec::new(),
            slippage_bps,
            taker_fee_bps,
            leverage,
        }
    }

    pub fn take_closed_trades(&mut self) -> Vec<ClosedTrade> {
        std::mem::take(&mut self.closed_trades)
    }

    /// Max base-asset size for a new long or short open: spot buy requires `notional + fee <= cash`;
    /// linear futures requires `notional/leverage + fee <= cash` (taker `taker_fee_bps`; slippage not reserved).
    pub fn max_order_qty_base(&self, price: Decimal) -> Decimal {
        if price <= Decimal::ZERO {
            return Decimal::ZERO;
        }
        let fee_rate = Decimal::from(self.taker_fee_bps) / dec!(10_000);
        match self.instrument.segment {
            MarketSegment::Futures => {
                let lev = self.eff_leverage();
                let unit_cost = Decimal::ONE / lev + fee_rate;
                if unit_cost <= Decimal::ZERO {
                    return Decimal::ZERO;
                }
                let max_notional = self.cash / unit_cost;
                max_notional / price
            }
            _ => {
                let denom = price * (Decimal::ONE + fee_rate);
                if denom <= Decimal::ZERO {
                    return Decimal::ZERO;
                }
                self.cash / denom
            }
        }
    }

    fn linear_futures(&self) -> bool {
        matches!(self.instrument.segment, MarketSegment::Futures)
    }

    fn eff_leverage(&self) -> Decimal {
        self.leverage.max(dec!(1))
    }

    /// Long aç / short aç / kapat; komisyon her bacakta `fee_bps` ile hesaplanır, `ClosedTrade.fee` = giriş + çıkış.
    pub fn market_order(
        &mut self,
        side: OrderSide,
        qty: Decimal,
        price: Decimal,
        ts: DateTime<Utc>,
        slip_bps: u32,
        fee_bps: u32,
    ) -> Result<(), &'static str> {
        if qty <= Decimal::ZERO {
            return Err("invalid_qty");
        }
        let slip = price * Decimal::from(slip_bps) / dec!(10_000);
        let fill_px = match side {
            OrderSide::Buy => price + slip,
            OrderSide::Sell => price - slip,
        };
        let notional = fill_px * qty;
        let fee = notional * Decimal::from(fee_bps) / dec!(10_000);
        let futures = self.linear_futures();
        let lev = self.eff_leverage();

        if side == OrderSide::Buy {
            if self.position.is_none() {
                if futures {
                    let margin = notional / lev;
                    if self.cash < margin + fee {
                        return Err("insufficient_cash");
                    }
                    self.cash -= margin + fee;
                    self.position = Some(OpenPosition {
                        side: OrderSide::Buy,
                        qty,
                        entry_px: fill_px,
                        entry_ts: ts,
                        entry_fee: fee,
                        initial_margin: margin,
                    });
                    return Ok(());
                }
                let cost = notional + fee;
                if self.cash < cost {
                    return Err("insufficient_cash");
                }
                self.cash -= cost;
                self.position = Some(OpenPosition {
                    side: OrderSide::Buy,
                    qty,
                    entry_px: fill_px,
                    entry_ts: ts,
                    entry_fee: fee,
                    initial_margin: Decimal::ZERO,
                });
                return Ok(());
            }
            let is_short = self
                .position
                .as_ref()
                .is_some_and(|p| p.side == OrderSide::Sell);
            if !is_short {
                return Err("invalid_order_for_position");
            }
            let pos = self.position.take().expect("short position");
            let exit_fee = fee;
            if futures {
                let m = pos.initial_margin;
                self.cash += m + (pos.entry_px - fill_px) * qty - exit_fee;
            } else {
                let cost = notional + exit_fee;
                if self.cash < cost {
                    self.position = Some(pos);
                    return Err("insufficient_cash");
                }
                self.cash -= cost;
            }
            let total_fee = pos.entry_fee + exit_fee;
            let pnl = (pos.entry_px - fill_px) * qty - total_fee;
            self.closed_trades.push(ClosedTrade {
                entry_ts: pos.entry_ts,
                exit_ts: ts,
                side: OrderSide::Sell,
                qty,
                entry_px: pos.entry_px,
                exit_px: fill_px,
                pnl,
                fee: total_fee,
            });
            return Ok(());
        }

        // Sell
        if let Some(pos) = self.position.as_ref() {
            if pos.side != OrderSide::Buy {
                return Err("invalid_order_for_position");
            }
            let pos = self.position.take().expect("long position");
            let exit_fee = fee;
            if futures {
                let m = pos.initial_margin;
                self.cash += m + (fill_px - pos.entry_px) * qty - exit_fee;
            } else {
                let proceeds = notional - exit_fee;
                self.cash += proceeds;
            }
            let total_fee = pos.entry_fee + exit_fee;
            let pnl = (fill_px - pos.entry_px) * qty - total_fee;
            self.closed_trades.push(ClosedTrade {
                entry_ts: pos.entry_ts,
                exit_ts: ts,
                side: pos.side,
                qty,
                entry_px: pos.entry_px,
                exit_px: fill_px,
                pnl,
                fee: total_fee,
            });
            return Ok(());
        }

        if futures {
            let margin = notional / lev;
            if self.cash < margin + fee {
                return Err("insufficient_cash");
            }
            self.cash -= margin + fee;
            self.position = Some(OpenPosition {
                side: OrderSide::Sell,
                qty,
                entry_px: fill_px,
                entry_ts: ts,
                entry_fee: fee,
                initial_margin: margin,
            });
            return Ok(());
        }

        let proceeds = notional - fee;
        self.cash += proceeds;
        self.position = Some(OpenPosition {
            side: OrderSide::Sell,
            qty,
            entry_px: fill_px,
            entry_ts: ts,
            entry_fee: fee,
            initial_margin: Decimal::ZERO,
        });
        Ok(())
    }
}

pub struct BacktestEngine {
    cfg: BacktestConfig,
}

impl BacktestEngine {
    pub fn new(cfg: BacktestConfig) -> Self {
        Self { cfg }
    }

    fn effective_leverage(instrument: &InstrumentId, cfg: &BacktestConfig) -> Decimal {
        match instrument.segment {
            MarketSegment::Futures => cfg.max_leverage.max(dec!(1)).min(dec!(125)),
            _ => dec!(1),
        }
    }

    fn mark_equity(ctx: &BacktestContext, bar_close: Decimal) -> Decimal {
        let Some(p) = &ctx.position else {
            return ctx.cash;
        };
        if matches!(ctx.instrument.segment, MarketSegment::Futures) {
            match p.side {
                OrderSide::Buy => ctx.cash + p.initial_margin + (bar_close - p.entry_px) * p.qty,
                OrderSide::Sell => ctx.cash + p.initial_margin + (p.entry_px - bar_close) * p.qty,
            }
        } else {
            match p.side {
                OrderSide::Buy => ctx.cash + p.qty * bar_close,
                OrderSide::Sell => ctx.cash - p.qty * bar_close,
            }
        }
    }

    #[instrument(skip(self, bars, strategy), fields(strategy = strategy.name()))]
    pub fn run<S: Strategy + ?Sized>(
        &self,
        instrument: InstrumentId,
        bars: VecDeque<TimestampBar>,
        strategy: &mut S,
    ) -> BacktestResult {
        let lev = Self::effective_leverage(&instrument, &self.cfg);
        let mut ctx = BacktestContext::new(
            instrument,
            self.cfg.initial_equity,
            self.cfg.slippage_bps,
            self.cfg.taker_fee_bps,
            lev,
        );
        let mut equity_curve = Vec::new();
        let mut closed: Vec<ClosedTrade> = Vec::new();

        for bar in bars.iter() {
            strategy.on_bar(&mut ctx, bar);
            closed.extend(ctx.take_closed_trades());

            let mark = Self::mark_equity(&ctx, bar.close);
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
    use qtss_domain::orders::OrderSide;

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

    /// Fixed size so expected fee and PnL in the round-trip test stay deterministic.
    struct OpenCloseLongFixedQty {
        n: i32,
    }

    impl Strategy for OpenCloseLongFixedQty {
        fn name(&self) -> &'static str {
            "open_close_long_fixed_qty"
        }
        fn on_bar(&mut self, ctx: &mut BacktestContext, bar: &TimestampBar) {
            self.n += 1;
            let slip = ctx.slippage_bps;
            let fee = ctx.taker_fee_bps;
            if self.n == 1 {
                let q = dec!(100);
                let _ = ctx.market_order(OrderSide::Buy, q, bar.close, bar.ts, slip, fee);
            } else if self.n == 2 {
                if let Some(p) = &ctx.position {
                    if p.side == OrderSide::Buy {
                        let q = p.qty;
                        let _ = ctx.market_order(OrderSide::Sell, q, bar.close, bar.ts, slip, fee);
                    }
                }
            }
        }
    }

    #[test]
    fn long_round_trip_closed_trade_fee_is_entry_plus_exit() {
        let eng = BacktestEngine::new(BacktestConfig {
            // 100 qty @ 100: notional 10_000 + 1% fee 100 = 10_100
            initial_equity: dec!(10_100),
            slippage_bps: 0,
            taker_fee_bps: 100,
            maker_fee_bps: 0,
            max_leverage: dec!(1),
        });
        let instr = InstrumentId {
            exchange: ExchangeId::Binance,
            segment: MarketSegment::Spot,
            symbol: "X".into(),
        };
        let t0 = Utc::now();
        let t1 = t0 + chrono::Duration::seconds(60);
        let mut bars = VecDeque::new();
        for (ts, c) in [(t0, dec!(100)), (t1, dec!(100))] {
            bars.push_back(TimestampBar {
                ts,
                open: c,
                high: c,
                low: c,
                close: c,
                volume: dec!(1),
            });
        }
        let mut s = OpenCloseLongFixedQty { n: 0 };
        let res = eng.run(instr, bars, &mut s);
        assert_eq!(res.trades.len(), 1);
        let t = &res.trades[0];
        assert_eq!(t.side, OrderSide::Buy);
        assert_eq!(t.fee, dec!(200));
        assert_eq!(t.pnl, dec!(-200));
    }

    #[test]
    fn max_order_qty_base_spot_reserves_taker_fee() {
        let instr = InstrumentId {
            exchange: ExchangeId::Binance,
            segment: MarketSegment::Spot,
            symbol: "X".into(),
        };
        let ctx = BacktestContext::new(
            instr,
            dec!(10_000),
            0,
            100,
            dec!(1),
        );
        let q = ctx.max_order_qty_base(dec!(100));
        let notional = dec!(100) * q;
        let fee = notional * dec!(100) / dec!(10_000);
        assert!(notional + fee <= dec!(10_000));
    }

    struct OpenCloseShort {
        n: i32,
    }

    impl Strategy for OpenCloseShort {
        fn name(&self) -> &'static str {
            "open_close_short"
        }
        fn on_bar(&mut self, ctx: &mut BacktestContext, bar: &TimestampBar) {
            self.n += 1;
            let slip = ctx.slippage_bps;
            let fee = ctx.taker_fee_bps;
            if self.n == 1 {
                let q = dec!(1);
                let _ = ctx.market_order(OrderSide::Sell, q, bar.close, bar.ts, slip, fee);
            } else if self.n == 2 {
                if let Some(p) = &ctx.position {
                    if p.side == OrderSide::Sell {
                        let q = p.qty;
                        let _ = ctx.market_order(OrderSide::Buy, q, bar.close, bar.ts, slip, fee);
                    }
                }
            }
        }
    }

    #[test]
    fn short_round_trip_flat_price_loses_fees_only() {
        let eng = BacktestEngine::new(BacktestConfig {
            initial_equity: dec!(10_000),
            slippage_bps: 0,
            taker_fee_bps: 100,
            maker_fee_bps: 0,
            max_leverage: dec!(10),
        });
        let instr = InstrumentId {
            exchange: ExchangeId::Binance,
            segment: MarketSegment::Futures,
            symbol: "X".into(),
        };
        let t0 = Utc::now();
        let t1 = t0 + chrono::Duration::seconds(60);
        let mut bars = VecDeque::new();
        for (ts, c) in [(t0, dec!(100)), (t1, dec!(100))] {
            bars.push_back(TimestampBar {
                ts,
                open: c,
                high: c,
                low: c,
                close: c,
                volume: dec!(1),
            });
        }
        let mut s = OpenCloseShort { n: 0 };
        let res = eng.run(instr, bars, &mut s);
        assert_eq!(res.trades.len(), 1);
        let t = &res.trades[0];
        assert_eq!(t.side, OrderSide::Sell);
        assert_eq!(t.fee, dec!(2));
        assert_eq!(t.pnl, dec!(-2));
    }

    #[test]
    fn futures_long_10x_uses_margin_not_full_notional() {
        struct OneBarLong {
            done: bool,
        }
        impl Strategy for OneBarLong {
            fn name(&self) -> &'static str {
                "one_bar_long"
            }
            fn on_bar(&mut self, ctx: &mut BacktestContext, bar: &TimestampBar) {
                if self.done {
                    return;
                }
                self.done = true;
                let q = dec!(1);
                let _ = ctx.market_order(
                    OrderSide::Buy,
                    q,
                    bar.close,
                    bar.ts,
                    ctx.slippage_bps,
                    ctx.taker_fee_bps,
                );
            }
        }

        let eng = BacktestEngine::new(BacktestConfig {
            initial_equity: dec!(10_000),
            slippage_bps: 0,
            taker_fee_bps: 0,
            maker_fee_bps: 0,
            max_leverage: dec!(10),
        });
        let instr = InstrumentId {
            exchange: ExchangeId::Binance,
            segment: MarketSegment::Futures,
            symbol: "X".into(),
        };
        let t0 = Utc::now();
        let mut bars = VecDeque::new();
        bars.push_back(TimestampBar {
            ts: t0,
            open: dec!(100),
            high: dec!(100),
            low: dec!(100),
            close: dec!(100),
            volume: dec!(1),
        });
        let mut s = OneBarLong { done: false };
        let res = eng.run(instr, bars, &mut s);
        // margin 100/10 = 10; wallet 9990; equity = 9990 + 10 + 0 = 10000
        let eq = res.equity_curve.last().unwrap().equity;
        assert!((eq - dec!(10_000)).abs() < dec!(1));
    }
}

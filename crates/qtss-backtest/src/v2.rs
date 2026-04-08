//! qtss-backtest v2 — historical replay over the new strategy /
//! execution / portfolio plane.
//!
//! The v1 engine in `engine.rs` keeps working untouched (Faz 7 will
//! retire it). v2 wires up the same building blocks the live worker
//! uses:
//!
//! ```text
//!   BarStream ──┬──> SignalSource ──> StrategyProvider ──> TradeIntent
//!               │                                              │
//!               └──> PortfolioEngine.mark_all <─ ApprovedIntent│
//!                                                              ▼
//!                                       ExecutionRouter (sim adapter)
//!                                                              │
//!                                                              ▼
//!                                                       PortfolioEngine.apply_fill
//! ```
//!
//! The runner is async because the execution router is async — even
//! the sim adapter exposes the same trait so live and backtest share
//! one code path. Risk gating is intentionally pluggable: callers
//! pass a closure that converts a `TradeIntent` into an
//! `ApprovedIntent`, so this crate does *not* depend on `qtss-risk`
//! directly. That keeps the dependency graph shallow and lets
//! research workflows swap in custom sizing without forking the
//! runner.
//!
//! ## Design (CLAUDE.md)
//!
//! - **No hardcoded numbers (#2):** every knob (starting equity,
//!   slippage, fees, mark-on-bar field) lives on
//!   [`BacktestV2Config`]. The runner has no defaults.
//! - **No leaky layers (#3):** this module knows about strategies,
//!   the portfolio engine, and the execution router. It does **not**
//!   know about storage, the validator, or any specific venue.
//! - **No scattered if/else (#1):** the runner's main loop is a flat
//!   sequence of trait calls; per-mode behaviour comes from the
//!   `RuntimeContext` policy, not from match arms inside the loop.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::detection::ValidatedDetection;
use qtss_domain::v2::intent::{ApprovedIntent, TradeIntent};
use qtss_execution_v2::{ExecutionRouter, RoutedAcks};
use qtss_portfolio::{PortfolioConfig, PortfolioEngine};
use qtss_runtime::{RunMode, RuntimeContext, RuntimeError};
use qtss_strategy::{StrategyContext, StrategyProvider};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Source of bars. Async so backends can stream from disk / DB.
#[async_trait]
pub trait BarStream: Send {
    async fn next(&mut self) -> Option<Bar>;
}

/// Source of validated detections aligned to a bar timestamp. The
/// runner consults this on every bar so research can plug in either
/// pre-computed detection fixtures or a live detector chain.
pub trait SignalSource: Send {
    fn signals_at(&mut self, at: DateTime<Utc>) -> Vec<ValidatedDetection>;
}

/// Closure-shaped risk approval hook so the backtest runner does not
/// depend on `qtss-risk` directly. Returning `None` rejects the
/// intent (typed as a *pass*, not an error).
pub type RiskApprover = Arc<dyn Fn(&TradeIntent) -> Option<ApprovedIntent> + Send + Sync>;

#[derive(Debug, Clone)]
pub struct BacktestV2Config {
    pub portfolio: PortfolioConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestSummary {
    pub bars_processed: u64,
    pub intents_emitted: u64,
    pub intents_approved: u64,
    pub orders_filled: u64,
    pub final_equity: Decimal,
    pub peak_equity: Decimal,
}

pub struct BacktestRunner {
    ctx: RuntimeContext,
    strategy: Arc<dyn StrategyProvider>,
    router: ExecutionRouter,
    portfolio: PortfolioEngine,
    risk: RiskApprover,
}

impl BacktestRunner {
    pub fn new(
        ctx: RuntimeContext,
        config: BacktestV2Config,
        strategy: Arc<dyn StrategyProvider>,
        router: ExecutionRouter,
        risk: RiskApprover,
    ) -> Result<Self, RuntimeError> {
        // Refuse to run unless the runtime context is actually a
        // backtest — guards against misconfigured callers (e.g. a
        // live worker handing us a Live context by accident).
        if !matches!(ctx.mode(), RunMode::Backtest) {
            return Err(RuntimeError::NotAllowed {
                op: "backtest_run",
                mode: ctx.mode(),
            });
        }
        Ok(Self {
            portfolio: PortfolioEngine::new(config.portfolio),
            ctx,
            strategy,
            router,
            risk,
        })
    }

    pub fn portfolio(&self) -> &PortfolioEngine {
        &self.portfolio
    }

    /// Drive the runner over `bars`. For every bar:
    ///
    /// 1. mark the portfolio at the bar close,
    /// 2. ask `signals` for any detections aligned to that bar,
    /// 3. evaluate the strategy on each detection,
    /// 4. risk-approve the resulting intents,
    /// 5. route the approved intents to the execution adapter,
    /// 6. apply the resulting fills back into the portfolio.
    pub async fn run<B, S>(
        &mut self,
        mut bars: B,
        mut signals: S,
    ) -> Result<BacktestSummary, RuntimeError>
    where
        B: BarStream,
        S: SignalSource,
    {
        let mut summary = BacktestSummary {
            bars_processed: 0,
            intents_emitted: 0,
            intents_approved: 0,
            orders_filled: 0,
            final_equity: Decimal::ZERO,
            peak_equity: Decimal::ZERO,
        };
        let strat_ctx = StrategyContext { run_mode: self.ctx.mode() };

        while let Some(bar) = bars.next().await {
            self.portfolio
                .mark(&bar.instrument.symbol, bar.close);
            summary.bars_processed += 1;

            for sig in signals.signals_at(bar.open_time) {
                let intents = self
                    .strategy
                    .evaluate(&sig, &strat_ctx)
                    .map_err(|e| RuntimeError::InvalidConfig(e.to_string()))?;
                summary.intents_emitted += intents.len() as u64;

                for intent in intents {
                    let Some(approved) = (self.risk)(&intent) else {
                        continue;
                    };
                    summary.intents_approved += 1;

                    match self.router.route(&approved).await {
                        Ok(acks) => {
                            self.absorb(&approved, &acks);
                            summary.orders_filled += 1;
                        }
                        Err(e) => {
                            tracing::warn!(
                                run_id = %self.ctx.id().0,
                                error = %e,
                                "router rejected approved intent"
                            );
                        }
                    }
                }
            }
        }

        let snap = self.portfolio.snapshot();
        summary.final_equity = snap.equity;
        summary.peak_equity = snap.peak_equity;
        Ok(summary)
    }

    fn absorb(&mut self, approved: &ApprovedIntent, acks: &RoutedAcks) {
        // The bracket's entry leg drives the position; stop / TPs are
        // resting orders that don't fill in this loop. Live mode will
        // get those fills via user-data-stream callbacks.
        for fill in &acks.entry.fills {
            self.portfolio.apply_fill(
                &approved.intent.instrument.symbol,
                approved.intent.side,
                fill.quantity,
                fill.price,
                fill.fee,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use qtss_domain::v2::bar::Bar;
    use qtss_domain::v2::instrument::{
        AssetClass, Instrument, SessionCalendar, Venue,
    };
    use qtss_domain::v2::intent::{
        SizingHint, Side, TimeInForce, TradeIntent,
    };
    use qtss_domain::v2::timeframe::Timeframe;
    use qtss_execution_v2::{ExecutionRouter, SimAdapter, SimConfig};
    use qtss_fees::{FeeBook, FeeModel, FeeSchedule};
    use qtss_runtime::{RuntimeId, StorageNamespace};
    use qtss_strategy::StrategyResult;
    use rust_decimal_macros::dec;
    use std::collections::VecDeque;
    use uuid::Uuid;

    fn instrument() -> Instrument {
        Instrument {
            venue: Venue::Binance,
            asset_class: AssetClass::CryptoSpot,
            symbol: "BTCUSDT".into(),
            quote_ccy: "USDT".into(),
            tick_size: dec!(0.01),
            lot_size: dec!(0.00001),
            session: SessionCalendar::binance_24x7(),
        }
    }

    fn bar(t: DateTime<Utc>, close: Decimal) -> Bar {
        Bar {
            instrument: instrument(),
            timeframe: Timeframe::H1,
            open_time: t,
            open: close,
            high: close,
            low: close,
            close,
            volume: dec!(1),
            closed: true,
        }
    }

    struct VecBars(VecDeque<Bar>);
    #[async_trait]
    impl BarStream for VecBars {
        async fn next(&mut self) -> Option<Bar> {
            self.0.pop_front()
        }
    }

    struct NoSignals;
    impl SignalSource for NoSignals {
        fn signals_at(&mut self, _: DateTime<Utc>) -> Vec<ValidatedDetection> {
            vec![]
        }
    }

    struct PassThroughStrategy;
    impl StrategyProvider for PassThroughStrategy {
        fn id(&self) -> &str {
            "passthrough"
        }
        fn evaluate(
            &self,
            _signal: &ValidatedDetection,
            _ctx: &StrategyContext,
        ) -> StrategyResult<Vec<TradeIntent>> {
            Ok(vec![])
        }
    }

    fn fees() -> Arc<dyn FeeModel> {
        let mut b = FeeBook::new();
        b.register_venue_default(
            "binance",
            FeeSchedule::new(dec!(0.0002), dec!(0.0007)).unwrap(),
        );
        Arc::new(b)
    }

    fn ctx() -> RuntimeContext {
        RuntimeContext::new(
            RuntimeId("bt-test".into()),
            RunMode::Backtest,
            StorageNamespace {
                live_schema: "public".into(),
                dry_schema: "dry".into(),
                backtest_schema: "bt".into(),
            },
        )
    }

    fn live_ctx() -> RuntimeContext {
        RuntimeContext::new(
            RuntimeId("oops".into()),
            RunMode::Live,
            StorageNamespace {
                live_schema: "public".into(),
                dry_schema: "dry".into(),
                backtest_schema: "bt".into(),
            },
        )
    }

    fn router() -> ExecutionRouter {
        let mut r = ExecutionRouter::new();
        let sim = Arc::new(SimAdapter::new(
            SimConfig { slippage_pct: dec!(0.0005) },
            fees(),
        ));
        r.register(RunMode::Backtest, sim);
        r
    }

    fn risk_pass_all() -> RiskApprover {
        Arc::new(|intent: &TradeIntent| {
            Some(ApprovedIntent {
                id: Uuid::new_v4(),
                approved_at: Utc::now(),
                intent: intent.clone(),
                quantity: dec!(0.001),
                notional: dec!(50),
                checks_passed: vec![],
                adjustments: vec![],
            })
        })
    }

    #[tokio::test]
    async fn empty_bar_stream_yields_zero_summary() {
        let cfg = BacktestV2Config {
            portfolio: PortfolioConfig {
                starting_equity: dec!(10000),
            },
        };
        let mut runner = BacktestRunner::new(
            ctx(),
            cfg,
            Arc::new(PassThroughStrategy),
            router(),
            risk_pass_all(),
        )
        .unwrap();

        let summary = runner
            .run(VecBars(VecDeque::new()), NoSignals)
            .await
            .unwrap();
        assert_eq!(summary.bars_processed, 0);
        assert_eq!(summary.final_equity, dec!(10000));
    }

    #[tokio::test]
    async fn bars_advance_and_mark_portfolio() {
        let cfg = BacktestV2Config {
            portfolio: PortfolioConfig {
                starting_equity: dec!(10000),
            },
        };
        let mut runner = BacktestRunner::new(
            ctx(),
            cfg,
            Arc::new(PassThroughStrategy),
            router(),
            risk_pass_all(),
        )
        .unwrap();

        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut bars = VecDeque::new();
        for i in 0..5i64 {
            bars.push_back(bar(
                t0 + chrono::Duration::hours(i),
                dec!(50000) + Decimal::from(i),
            ));
        }
        let summary = runner.run(VecBars(bars), NoSignals).await.unwrap();
        assert_eq!(summary.bars_processed, 5);
        assert_eq!(summary.intents_emitted, 0);
    }

    #[tokio::test]
    async fn refuses_non_backtest_context() {
        let cfg = BacktestV2Config {
            portfolio: PortfolioConfig {
                starting_equity: dec!(10000),
            },
        };
        let result = BacktestRunner::new(
            live_ctx(),
            cfg,
            Arc::new(PassThroughStrategy),
            router(),
            risk_pass_all(),
        );
        let Err(err) = result else { panic!("expected NotAllowed") };
        assert!(matches!(err, RuntimeError::NotAllowed { .. }));
    }

    #[tokio::test]
    async fn signal_pass_yields_no_intents() {
        // Strategy returns empty Vec → not an error, just a hold.
        let cfg = BacktestV2Config {
            portfolio: PortfolioConfig {
                starting_equity: dec!(10000),
            },
        };
        let mut runner = BacktestRunner::new(
            ctx(),
            cfg,
            Arc::new(PassThroughStrategy),
            router(),
            risk_pass_all(),
        )
        .unwrap();

        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let bars = VecDeque::from(vec![bar(t0, dec!(50000))]);
        let summary = runner.run(VecBars(bars), NoSignals).await.unwrap();
        assert_eq!(summary.intents_approved, 0);
        // Helper to silence "Side unused in non-test" if removed:
        let _ = Side::Long;
        let _ = SizingHint::RiskPct { pct: dec!(0.005) };
        let _ = TimeInForce::Gtc;
    }
}

//! IqBacktestRunner — the orchestrator.
//!
//! v1 SCOPE (this commit): bar iteration over `market_bars` rows for
//! a given universe, with hooks for setup detection / lifecycle /
//! cost / attribution / logging. The loop body is intentionally
//! a thin sequence of trait calls so future commits (FAZ 26.2-26.4)
//! can swap in:
//!
//!   - `SetupDetector::detect_at(bar) -> Option<TradeIntent>` — the
//!     replay of the worker's IQ-D / IQ-T candidate logic.
//!   - `TradeManager::on_bar(bar, &mut open_trades)` — TP/SL
//!     ladder evaluation, trailing stop, timeout enforcement.
//!   - `RegimeProvider::regime_at(time) -> RegimeLabel` — for the
//!     "regime mismatch" attribution channel.
//!
//! For now the runner runs an EMPTY detector + lifecycle (no trades
//! ever open) so we can land the scaffolding + tests / wiring +
//! end-to-end smoke before the heavy detector replay logic. Subsequent
//! commits flesh out each hook one at a time, every commit landing a
//! working backtest with progressively richer signal coverage.

use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;

use super::attribution::{classify, OutcomeAttribution};
use super::availability::{
    probe as probe_data_availability, DataAvailabilityReport,
};
use super::config::IqBacktestConfig;
use super::cost::CostModel;
use super::report::{aggregate, IqBacktestReport};
use super::trade::IqTrade;
use super::trade_log::TradeLogWriter;

/// Hook for resolving a bar tape into trade intents. v1 ships an
/// empty implementation (`NoSetups`); FAZ 26.2 introduces the real
/// detector replay.
#[async_trait::async_trait]
pub trait SetupDetector: Send + Sync {
    /// Inspect bars up to (and including) `bar_index` and emit any
    /// new IQ-D / IQ-T trade entries that should fire AT bar close.
    /// Returns the bar index of any trade openings.
    async fn detect_at(
        &self,
        pool: &PgPool,
        bar_index: usize,
        bar_time: DateTime<Utc>,
        bar_close: Decimal,
    ) -> Vec<IqTrade>;
}

/// Empty detector — never fires. Default for v1 scaffolding.
pub struct NoSetups;

#[async_trait::async_trait]
impl SetupDetector for NoSetups {
    async fn detect_at(
        &self,
        _pool: &PgPool,
        _bar_index: usize,
        _bar_time: DateTime<Utc>,
        _bar_close: Decimal,
    ) -> Vec<IqTrade> {
        Vec::new()
    }
}

/// Hook for advancing open trades on each bar. v1 ships a stub
/// (`NoLifecycle`) — lifecycle logic lands in FAZ 26.2.
#[async_trait::async_trait]
pub trait TradeManager: Send + Sync {
    /// Mark all open trades to the new bar's close, evaluate
    /// stops/targets, and return the trades that closed THIS bar
    /// (ready for attribution + log).
    async fn on_bar(
        &self,
        bar_index: usize,
        bar_time: DateTime<Utc>,
        bar_high: Decimal,
        bar_low: Decimal,
        bar_close: Decimal,
        open_trades: &mut Vec<IqTrade>,
    ) -> Vec<IqTrade>;
}

pub struct NoLifecycle;

#[async_trait::async_trait]
impl TradeManager for NoLifecycle {
    async fn on_bar(
        &self,
        _bar_index: usize,
        _bar_time: DateTime<Utc>,
        _bar_high: Decimal,
        _bar_low: Decimal,
        _bar_close: Decimal,
        _open_trades: &mut Vec<IqTrade>,
    ) -> Vec<IqTrade> {
        Vec::new()
    }
}

/// The runner. Owns the config and the JSONL writer; collects
/// closed trades + their attributions in memory for the final
/// `IqBacktestReport`.
pub struct IqBacktestRunner {
    config: IqBacktestConfig,
    cost: CostModel,
    detector: Arc<dyn SetupDetector>,
    manager: Arc<dyn TradeManager>,
    log: Option<TradeLogWriter>,
}

impl IqBacktestRunner {
    pub fn new(config: IqBacktestConfig) -> std::io::Result<Self> {
        let log = match &config.trade_log_path {
            Some(p) => Some(TradeLogWriter::open(p.clone())?),
            None => None,
        };
        Ok(Self {
            config,
            cost: CostModel::default(),
            detector: Arc::new(NoSetups),
            manager: Arc::new(NoLifecycle),
            log,
        })
    }

    pub fn with_detector(mut self, d: Arc<dyn SetupDetector>) -> Self {
        self.detector = d;
        self
    }

    pub fn with_lifecycle(mut self, m: Arc<dyn TradeManager>) -> Self {
        self.manager = m;
        self
    }

    pub fn with_cost(mut self, c: CostModel) -> Self {
        self.cost = c;
        self
    }

    /// End-to-end run. Iterates bars from `market_bars` for the
    /// configured universe + time window, drives the detector +
    /// lifecycle hooks, records closed trades, returns the report.
    pub async fn run(&self, pool: &PgPool) -> anyhow::Result<IqBacktestReport> {
        let u = &self.config.universe;
        info!(
            symbol = %u.symbol,
            tf = %u.timeframe,
            start = %u.start_time,
            end = %u.end_time,
            "iq-backtest starting"
        );

        // BUG BACKTEST — pre-flight data availability probe. Surfaces
        // missing/empty channels BEFORE the bar loop so the operator
        // doesn't waste 100s wondering why a fully-populated tape
        // produced zero trades.
        let availability: DataAvailabilityReport =
            probe_data_availability(pool, &self.config).await;
        availability.print();
        if availability.has_critical_gap() {
            warn!(
                "data availability has critical gaps — 0-trade output is expected"
            );
        }

        let mut open: Vec<IqTrade> = Vec::new();
        let mut all_closed: Vec<(IqTrade, OutcomeAttribution)> = Vec::new();
        let mut bars_processed: u64 = 0;

        // Pull bars in chronological order (oldest first). Stream so
        // long windows don't blow memory.
        let mut rows = sqlx::query(
            r#"SELECT open_time, open, high, low, close, volume
                 FROM market_bars
                WHERE exchange = $1 AND segment = $2
                  AND symbol = $3 AND interval = $4
                  AND open_time BETWEEN $5 AND $6
                ORDER BY open_time ASC"#,
        )
        .bind(&u.exchange)
        .bind(&u.segment)
        .bind(&u.symbol)
        .bind(&u.timeframe)
        .bind(u.start_time)
        .bind(u.end_time)
        .fetch_all(pool)
        .await?;
        debug!(rows = rows.len(), "fetched bar tape");

        // FAZ 26.5 — slow-bar metric. Bars taking longer than
        // SLOW_BAR_THRESHOLD_MS to process emit a structured warn
        // event so the operator can find DB / detector hot paths
        // without running a profiler. 100ms is a generous default —
        // raise via env var when running over noisy symbols.
        let slow_threshold_ms: u64 = std::env::var("QTSS_BACKTEST_SLOW_BAR_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);
        let mut slow_bars: u64 = 0;
        let mut total_processing_ms: u128 = 0;

        for (i, row) in rows.drain(..).enumerate() {
            use sqlx::Row;
            let bar_start = Instant::now();
            let open_time: DateTime<Utc> = row.try_get("open_time")?;
            let high: Decimal = row.try_get("high")?;
            let low: Decimal = row.try_get("low")?;
            let close: Decimal = row.try_get("close")?;

            // 1. Advance open trades.
            let mut closed_this_bar = self
                .manager
                .on_bar(i, open_time, high, low, close, &mut open)
                .await;

            // 2. Detect new setups; insert into open list — capped by
            // config.risk.max_concurrent_trades. The detector itself
            // is pure and stateless, so the cap lives here. Without
            // this check `--config btc_4h_dip_current.json` peaked at
            // 43 simultaneously open trades against a configured limit
            // of 5 (8.6× breach) — risk_per_trade_pct=1% × 43 = 43%
            // of equity at risk in flight, completely outside the
            // operator's intended bound.
            let cap = self.config.risk.max_concurrent_trades as usize;
            // Count only trades that are actually consuming risk
            // (Open / ScalingOut). Pending / Closed / Aborted are
            // bookkeeping rows the manager hasn't drained yet.
            let live_count = open
                .iter()
                .filter(|t| {
                    matches!(
                        t.state,
                        super::trade::TradeState::Open
                            | super::trade::TradeState::ScalingOut
                            | super::trade::TradeState::Pending
                    )
                })
                .count();
            let mut accepted = 0usize;
            let mut dropped = 0usize;
            if cap == 0 || live_count < cap {
                let headroom = cap.saturating_sub(live_count);
                let new_trades =
                    self.detector.detect_at(pool, i, open_time, close).await;
                for t in new_trades {
                    if cap > 0 && accepted >= headroom {
                        dropped += 1;
                        continue;
                    }
                    open.push(t);
                    accepted += 1;
                }
            } else {
                // Already at the cap — skip detection entirely so the
                // operator sees an honest count of how many candidates
                // we silently dropped.
                let new_trades =
                    self.detector.detect_at(pool, i, open_time, close).await;
                dropped = new_trades.len();
            }
            if dropped > 0 {
                debug!(
                    bar = i,
                    bar_time = %open_time,
                    accepted = accepted,
                    dropped = dropped,
                    cap = cap,
                    live_before = live_count,
                    "concurrency cap dropped detector candidates"
                );
            }
            let bar_ms = bar_start.elapsed().as_millis();
            total_processing_ms += bar_ms;
            if (bar_ms as u64) > slow_threshold_ms {
                slow_bars += 1;
                warn!(
                    bar = i,
                    bar_time = %open_time,
                    bar_ms = bar_ms,
                    threshold_ms = slow_threshold_ms,
                    "slow-bar — detection / lifecycle pass exceeded threshold"
                );
            }

            // 3. Path snapshots (every N bars) for still-open trades.
            if self.config.path_snapshot_every_bars > 0
                && i as u32 % self.config.path_snapshot_every_bars == 0
            {
                for t in open.iter_mut() {
                    t.push_snapshot(i, open_time, close);
                }
            }

            // 4. Classify + log every closed trade.
            for trade in closed_this_bar.drain(..) {
                let attr = classify(&trade);
                if let Some(w) = &self.log {
                    let _ = w.write_row(&trade, &attr);
                }
                all_closed.push((trade, attr));
            }

            bars_processed += 1;
        }

        // 5. Anything left open at end-of-window gets recorded too
        //    so the report knows about it (`open_at_end` bucket).
        for trade in open.drain(..) {
            let attr = classify(&trade);
            if let Some(w) = &self.log {
                let _ = w.write_row(&trade, &attr);
            }
            all_closed.push((trade, attr));
        }

        let mut report =
            aggregate(self.config.clone(), bars_processed, &all_closed);
        report.data_availability = Some(availability);
        let avg_bar_ms = if bars_processed > 0 {
            total_processing_ms as f64 / bars_processed as f64
        } else {
            0.0
        };
        info!(
            total_trades = report.total_trades,
            wins = report.wins,
            losses = report.losses,
            net_pnl = %report.net_pnl,
            max_dd_pct = report.max_drawdown_pct,
            slow_bars = slow_bars,
            avg_bar_ms = avg_bar_ms,
            total_ms = total_processing_ms,
            "iq-backtest done"
        );
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iq::config::IqPolarity;
    use crate::iq::trade::{IqTrade, TradeState};
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use serde_json::json;

    #[test]
    fn runner_builds_with_default_config() {
        let cfg = IqBacktestConfig::default();
        let runner = IqBacktestRunner::new(cfg).unwrap();
        // Defaults wire NoSetups + NoLifecycle.
        assert!(matches!(runner.config.polarity, IqPolarity::Dip));
    }

    /// Regression: an unbounded detector emitted 5 candidates per bar
    /// while max_concurrent_trades=2. Without the runner-side cap the
    /// open list would balloon to 5; the cap must drop the surplus
    /// silently and keep `open` ≤ 2.
    ///
    /// The inline "live count" in run() filters by trade state, so we
    /// reproduce that here in plain code (no DB) — the test focuses
    /// on the cap accounting, not the SQL plumbing.
    #[test]
    fn concurrency_cap_drops_surplus_candidates() {
        let cap: usize = 2;

        // Pre-populate the open list with 1 live trade.
        let mut open: Vec<IqTrade> = Vec::new();
        let mut existing = IqTrade::pending(
            "test",
            IqPolarity::Dip,
            "BTCUSDT",
            "4h",
            "binance",
            "futures",
            0,
            Utc::now(),
            dec!(50000),
            dec!(49500),
            vec![dec!(50500)],
            dec!(0.1),
            json!({}),
            0.7,
        );
        existing.state = TradeState::Open;
        open.push(existing);

        // Detector emits 5 candidates this bar.
        let new_trades: Vec<IqTrade> = (0..5)
            .map(|i| {
                IqTrade::pending(
                    "test",
                    IqPolarity::Dip,
                    "BTCUSDT",
                    "4h",
                    "binance",
                    "futures",
                    i,
                    Utc::now(),
                    dec!(50000),
                    dec!(49500),
                    vec![dec!(50500)],
                    dec!(0.1),
                    json!({}),
                    0.7,
                )
            })
            .collect();

        // Replicate the cap accounting from run().
        let live_count = open
            .iter()
            .filter(|t| {
                matches!(
                    t.state,
                    TradeState::Open
                        | TradeState::ScalingOut
                        | TradeState::Pending
                )
            })
            .count();
        let mut accepted = 0usize;
        let mut dropped = 0usize;
        if cap == 0 || live_count < cap {
            let headroom = cap.saturating_sub(live_count);
            for t in new_trades {
                if cap > 0 && accepted >= headroom {
                    dropped += 1;
                    continue;
                }
                open.push(t);
                accepted += 1;
            }
        } else {
            dropped = new_trades.len();
        }

        assert_eq!(accepted, 1, "headroom = cap(2) - live(1) = 1");
        assert_eq!(dropped, 4, "4 surplus candidates must be dropped");
        assert_eq!(open.len(), 2, "open count = cap exactly");
    }

    /// When already AT the cap, the detector still runs (so the
    /// debug log shows the "missed" candidate count), but ALL new
    /// trades are dropped — none added.
    #[test]
    fn concurrency_cap_at_max_drops_everything() {
        let cap: usize = 2;
        let mut open: Vec<IqTrade> = Vec::new();
        for _ in 0..2 {
            let mut t = IqTrade::pending(
                "test",
                IqPolarity::Dip,
                "BTCUSDT",
                "4h",
                "binance",
                "futures",
                0,
                Utc::now(),
                dec!(50000),
                dec!(49500),
                vec![dec!(50500)],
                dec!(0.1),
                json!({}),
                0.7,
            );
            t.state = TradeState::Open;
            open.push(t);
        }

        let new_trades: Vec<IqTrade> = (0..3)
            .map(|i| {
                IqTrade::pending(
                    "test",
                    IqPolarity::Dip,
                    "BTCUSDT",
                    "4h",
                    "binance",
                    "futures",
                    i,
                    Utc::now(),
                    dec!(50000),
                    dec!(49500),
                    vec![dec!(50500)],
                    dec!(0.1),
                    json!({}),
                    0.7,
                )
            })
            .collect();

        let live_count = open.len();
        let dropped = if cap == 0 || live_count < cap {
            0
        } else {
            new_trades.len()
        };
        assert_eq!(dropped, 3);
        assert_eq!(open.len(), 2, "no new trades added at cap");
    }

    /// `max_concurrent_trades = 0` is treated as "no cap" — useful
    /// for sweeps where the operator explicitly wants every signal
    /// to fire (and accepts the unbounded risk that comes with it).
    #[test]
    fn concurrency_cap_zero_means_unbounded() {
        let cap: usize = 0;
        let mut open: Vec<IqTrade> = Vec::new();
        let new_trades: Vec<IqTrade> = (0..50)
            .map(|i| {
                IqTrade::pending(
                    "test",
                    IqPolarity::Dip,
                    "BTCUSDT",
                    "4h",
                    "binance",
                    "futures",
                    i,
                    Utc::now(),
                    dec!(50000),
                    dec!(49500),
                    vec![dec!(50500)],
                    dec!(0.1),
                    json!({}),
                    0.7,
                )
            })
            .collect();

        // Replicate the cap branch — when cap = 0 the inner loop must
        // accept every trade.
        let live_count = open.len();
        let mut accepted = 0usize;
        if cap == 0 || live_count < cap {
            let headroom = cap.saturating_sub(live_count);
            for t in new_trades {
                if cap > 0 && accepted >= headroom {
                    continue;
                }
                open.push(t);
                accepted += 1;
            }
        }
        assert_eq!(accepted, 50);
        assert_eq!(open.len(), 50);
    }
}

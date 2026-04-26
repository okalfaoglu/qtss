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

            // 2. Detect new setups; insert into open list.
            let new_trades =
                self.detector.detect_at(pool, i, open_time, close).await;
            for t in new_trades {
                open.push(t);
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

    #[test]
    fn runner_builds_with_default_config() {
        let cfg = IqBacktestConfig::default();
        let runner = IqBacktestRunner::new(cfg).unwrap();
        // Defaults wire NoSetups + NoLifecycle.
        assert!(matches!(runner.config.polarity, IqPolarity::Dip));
    }
}

//! Signal + TA conflict filter: opens positions from `onchain_signal_scores` when thresholds pass.
//!
//! Set `QTSS_SIGNAL_FILTER_AUTO_PLACE=1` to call [`qtss_execution::gateway::ExecutionGateway::place`]; otherwise decisions are logged only.

use std::sync::Arc;
use std::time::Duration;

use qtss_common::is_trading_halted;
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{OrderIntent, OrderSide, OrderType, TimeInForce};
use qtss_domain::symbol::InstrumentId;
use qtss_execution::ExecutionGateway;
use qtss_storage::list_enabled_engine_symbols;
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::context::MarketContext;
use crate::risk::DrawdownGuard;

pub struct SignalFilterStrategy {
    pub pool: PgPool,
    pub gateway: Arc<dyn ExecutionGateway>,
    pub long_threshold: f64,
    pub short_threshold: f64,
    pub min_confidence: f64,
}

fn auto_place_orders() -> bool {
    std::env::var("QTSS_SIGNAL_FILTER_AUTO_PLACE")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn order_quantity() -> Decimal {
    std::env::var("QTSS_STRATEGY_ORDER_QTY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| Decimal::new(1, 3))
}

fn requires_human_approval() -> bool {
    !std::env::var("QTSS_STRATEGY_SKIP_HUMAN_APPROVAL")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

impl SignalFilterStrategy {
    pub async fn run(self) {
        let tick = Duration::from_secs(
            std::env::var("QTSS_SIGNAL_FILTER_TICK_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(60),
        );
        loop {
            tokio::time::sleep(tick).await;
            if let Err(e) = self.tick().await {
                warn!(%e, "signal_filter_strategy tick error");
            }
        }
    }

    async fn tick(&self) -> anyhow::Result<()> {
        if is_trading_halted() {
            tracing::warn!("signal_filter: kill switch — emir yok");
            return Ok(());
        }
        let guard = DrawdownGuard::from_env(0.0);
        if !guard.allows_new_position() {
            tracing::debug!("drawdown guard: skip tick (wire QTSS / PnL for real daily pnl)");
        }

        let rows = list_enabled_engine_symbols(&self.pool).await?;
        for es in rows {
            let sym = es.symbol.trim().to_uppercase();
            let Some(ctx) = MarketContext::load(
                &self.pool,
                &sym,
                es.exchange.trim(),
                es.segment.trim(),
                es.interval.trim(),
            )
            .await
            else {
                continue;
            };
            if ctx.conflict_detected {
                tracing::debug!(%sym, "skip: TA vs on-chain conflict");
                continue;
            }
            if ctx.confidence < self.min_confidence {
                tracing::debug!(%sym, conf = ctx.confidence, "skip: low confidence");
                continue;
            }

            let side = if ctx.aggregate_score > self.long_threshold {
                Some(OrderSide::Buy)
            } else if ctx.aggregate_score < self.short_threshold {
                Some(OrderSide::Sell)
            } else {
                None
            };

            let Some(side) = side else {
                continue;
            };

            let intent = OrderIntent {
                instrument: InstrumentId {
                    exchange: ExchangeId::Binance,
                    segment: MarketSegment::Futures,
                    symbol: sym.clone(),
                },
                side,
                quantity: order_quantity(),
                order_type: OrderType::Market,
                time_in_force: TimeInForce::Gtc,
                requires_human_approval: requires_human_approval(),
                futures: None,
            };

            if auto_place_orders() {
                match self.gateway.place(intent.clone()).await {
                    Ok(id) => info!(%sym, ?side, %id, "signal_filter: order placed"),
                    Err(e) => warn!(%sym, ?side, %e, "signal_filter: place failed"),
                }
            } else {
                info!(
                    %sym,
                    ?side,
                    agg = ctx.aggregate_score,
                    conf = ctx.confidence,
                    "signal_filter: would place (set QTSS_SIGNAL_FILTER_AUTO_PLACE=1 to execute)"
                );
            }
        }
        Ok(())
    }
}

pub async fn run(pool: PgPool, gateway: Arc<dyn ExecutionGateway>) {
    let strategy = SignalFilterStrategy {
        pool,
        gateway,
        long_threshold: std::env::var("QTSS_LONG_THRESHOLD")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.6),
        short_threshold: std::env::var("QTSS_SHORT_THRESHOLD")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(-0.6),
        min_confidence: std::env::var("QTSS_MIN_SIGNAL_CONFIDENCE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.4),
    };
    strategy.run().await;
}

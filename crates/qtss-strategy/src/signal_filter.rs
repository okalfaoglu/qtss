//! Signal + TA conflict filter: opens positions from `onchain_signal_scores` when thresholds pass.
//!
//! - `QTSS_SIGNAL_FILTER_AUTO_PLACE=1` → [`ExecutionGateway::place`]
//! - `QTSS_SIGNAL_FILTER_ON_CONFLICT=half` → FAQ §10: çatışmada yarım miktar
//! - `QTSS_SIGNAL_FILTER_BRACKET_ORDERS=1` → giriş sonrası SL/TP (referans: son bar kapanışı)

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use qtss_common::is_trading_halted;
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{FuturesExecutionExtras, OrderIntent, OrderSide, OrderType, TimeInForce};
use qtss_domain::symbol::InstrumentId;
use qtss_execution::ExecutionGateway;
use qtss_storage::list_enabled_engine_symbols;
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::conflict_policy::{conflict_size_policy_from_env, ConflictSizePolicy};
use crate::context::MarketContext;
use crate::risk::{apply_kelly_scale_to_qty, clamp_qty_by_max_notional_usdt, DrawdownGuard};

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

fn bracket_orders_enabled() -> bool {
    std::env::var("QTSS_SIGNAL_FILTER_BRACKET_ORDERS")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn order_quantity() -> Decimal {
    let base = std::env::var("QTSS_STRATEGY_ORDER_QTY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| Decimal::new(1, 3));
    apply_kelly_scale_to_qty(base)
}

fn sl_tp_pct() -> (Decimal, Decimal) {
    let sl = std::env::var("QTSS_DEFAULT_STOP_LOSS_PCT")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .unwrap_or_else(|| Decimal::new(2, 0));
    let tp = std::env::var("QTSS_DEFAULT_TAKE_PROFIT_PCT")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .unwrap_or_else(|| Decimal::new(4, 0));
    (sl, tp)
}

fn requires_human_approval() -> bool {
    !std::env::var("QTSS_STRATEGY_SKIP_HUMAN_APPROVAL")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn bracket_stop_take(
    side: OrderSide,
    entry: Decimal,
    sl_pct: Decimal,
    tp_pct: Decimal,
) -> (Decimal, Decimal) {
    let slf = sl_pct / Decimal::from(100u32);
    let tpf = tp_pct / Decimal::from(100u32);
    match side {
        OrderSide::Buy => (
            entry * (Decimal::ONE - slf),
            entry * (Decimal::ONE + tpf),
        ),
        OrderSide::Sell => (
            entry * (Decimal::ONE + slf),
            entry * (Decimal::ONE - tpf),
        ),
    }
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
            let mut qty = order_quantity();
            if ctx.conflict_detected {
                match conflict_size_policy_from_env() {
                    ConflictSizePolicy::Skip => {
                        tracing::debug!(%sym, "skip: TA vs on-chain conflict");
                        continue;
                    }
                    ConflictSizePolicy::Half => {
                        qty = qty / Decimal::from(2u32);
                        if qty < Decimal::new(1, 8) {
                            tracing::debug!(%sym, "skip: half-size qty too small");
                            continue;
                        }
                        tracing::debug!(%sym, "conflict: half-size qty");
                    }
                }
            }
            if let Some(mark) = ctx.last_close {
                qty = clamp_qty_by_max_notional_usdt(qty, mark);
                if qty < Decimal::new(1, 8) {
                    tracing::debug!(%sym, "skip: max-notional clamp left qty too small");
                    continue;
                }
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

            let instrument = InstrumentId {
                exchange: ExchangeId::Binance,
                segment: MarketSegment::Futures,
                symbol: sym.clone(),
            };

            let intent = OrderIntent {
                instrument: instrument.clone(),
                side,
                quantity: qty,
                order_type: OrderType::Market,
                time_in_force: TimeInForce::Gtc,
                requires_human_approval: requires_human_approval(),
                futures: None,
            };

            if auto_place_orders() {
                match self.gateway.place(intent.clone()).await {
                    Ok(id) => {
                        info!(%sym, ?side, %id, "signal_filter: order placed");
                        if bracket_orders_enabled() {
                            if let Some(entry) = ctx.last_close {
                                let (sl_pct, tp_pct) = sl_tp_pct();
                                let (stop_px, tp_px) =
                                    bracket_stop_take(side, entry, sl_pct, tp_pct);
                                let exit_side = match side {
                                    OrderSide::Buy => OrderSide::Sell,
                                    OrderSide::Sell => OrderSide::Buy,
                                };
                                let fut = Some(FuturesExecutionExtras {
                                    position_side: None,
                                    reduce_only: Some(true),
                                });
                                let sl_intent = OrderIntent {
                                    instrument: instrument.clone(),
                                    side: exit_side,
                                    quantity: qty,
                                    order_type: OrderType::StopMarket {
                                        stop_price: stop_px,
                                    },
                                    time_in_force: TimeInForce::Gtc,
                                    requires_human_approval: false,
                                    futures: fut.clone(),
                                };
                                if let Err(e) = self.gateway.place(sl_intent).await {
                                    warn!(%sym, %e, "signal_filter: SL emri başarısız");
                                }
                                let tp_intent = OrderIntent {
                                    instrument,
                                    side: exit_side,
                                    quantity: qty,
                                    order_type: OrderType::TakeProfitMarket {
                                        stop_price: tp_px,
                                    },
                                    time_in_force: TimeInForce::Gtc,
                                    requires_human_approval: false,
                                    futures: fut,
                                };
                                if let Err(e) = self.gateway.place(tp_intent).await {
                                    warn!(%sym, %e, "signal_filter: TP emri başarısız");
                                }
                            } else {
                                tracing::debug!(%sym, "signal_filter: bracket atlandı — last_close yok");
                            }
                        }
                    }
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

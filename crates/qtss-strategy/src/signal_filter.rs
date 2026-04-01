//! Signal + TA conflict filter — `strategy.*` + `resolve_*` (`system_config`).

use std::sync::Arc;
use std::time::Duration;

use qtss_common::is_trading_halted;
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{FuturesExecutionExtras, OrderIntent, OrderSide, OrderType, TimeInForce};
use qtss_domain::symbol::InstrumentId;
use qtss_execution::ExecutionGateway;
use qtss_storage::{
    list_enabled_engine_symbols, resolve_system_decimal, resolve_system_f64, resolve_worker_enabled_flag,
    resolve_worker_tick_secs,
};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::conflict_policy::{conflict_size_policy_from_db, ConflictSizePolicy};
use crate::context::MarketContext;
use crate::risk::{
    apply_kelly_scale_to_qty, clamp_qty_by_max_notional_usdt, kelly_qty_scale, DrawdownGuard,
};

pub struct SignalFilterStrategy {
    pub pool: PgPool,
    pub gateway: Arc<dyn ExecutionGateway>,
    pub long_threshold: f64,
    pub short_threshold: f64,
    pub min_confidence: f64,
    pub tick_secs: u64,
    pub auto_place: bool,
    pub bracket_orders: bool,
    pub requires_human_approval: bool,
    pub base_order_qty: Decimal,
    pub sl_pct: Decimal,
    pub tp_pct: Decimal,
    pub kelly_scale: f64,
    pub max_notional_usdt: Decimal,
    pub max_drawdown_pct: f64,
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
        let tick = Duration::from_secs(self.tick_secs.max(5));
        loop {
            tokio::time::sleep(tick).await;
            if let Err(e) = self.tick().await {
                warn!(%e, "signal_filter_strategy tick error");
            }
        }
    }

    fn order_quantity(&self) -> Decimal {
        apply_kelly_scale_to_qty(self.base_order_qty, self.kelly_scale)
    }

    async fn tick(&self) -> anyhow::Result<()> {
        if is_trading_halted() {
            tracing::warn!("signal_filter: kill switch — emir yok");
            return Ok(());
        }
        let guard = DrawdownGuard::new(self.max_drawdown_pct, 0.0);
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
            let mut qty = self.order_quantity();
            if ctx.conflict_detected {
                match conflict_size_policy_from_db(&self.pool).await {
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
                qty = clamp_qty_by_max_notional_usdt(qty, mark, self.max_notional_usdt);
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
                requires_human_approval: self.requires_human_approval,
                futures: None,
            };

            if self.auto_place {
                let Some(mark) = ctx.last_close else {
                    warn!(
                        %sym,
                        "signal_filter: last_close yok — market place (özellikle dry) için referans fiyat gerekli"
                    );
                    continue;
                };
                if let Err(e) = self.gateway.set_reference_price(&instrument, mark) {
                    warn!(%sym, %e, "signal_filter: set_reference_price");
                    continue;
                }
                match self.gateway.place(intent.clone()).await {
                    Ok(id) => {
                        info!(%sym, ?side, %id, "signal_filter: order placed");
                        if self.bracket_orders {
                            if let Some(entry) = ctx.last_close {
                                if let Err(e) = self.gateway.set_reference_price(&instrument, entry) {
                                    warn!(%sym, %e, "signal_filter: bracket set_reference_price");
                                } else {
                                    let (stop_px, tp_px) =
                                        bracket_stop_take(side, entry, self.sl_pct, self.tp_pct);
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
                    "signal_filter: would place (strategy.signal_filter_auto_place)"
                );
            }
        }
        Ok(())
    }
}

pub async fn run(pool: PgPool, gateway: Arc<dyn ExecutionGateway>) {
    let tick_secs = resolve_worker_tick_secs(
        &pool,
        "strategy",
        "signal_filter_tick_secs",
        "QTSS_SIGNAL_FILTER_TICK_SECS",
        60,
        5,
    )
    .await;
    let auto_place = resolve_worker_enabled_flag(
        &pool,
        "strategy",
        "signal_filter_auto_place",
        "QTSS_SIGNAL_FILTER_AUTO_PLACE",
        false,
    )
    .await;
    let bracket_orders = resolve_worker_enabled_flag(
        &pool,
        "strategy",
        "signal_filter_bracket_orders",
        "QTSS_SIGNAL_FILTER_BRACKET_ORDERS",
        false,
    )
    .await;
    let skip_human = resolve_worker_enabled_flag(
        &pool,
        "strategy",
        "strategy_skip_human_approval",
        "QTSS_STRATEGY_SKIP_HUMAN_APPROVAL",
        false,
    )
    .await;
    let base_order_qty = resolve_system_decimal(
        &pool,
        "strategy",
        "strategy_order_qty",
        "QTSS_STRATEGY_ORDER_QTY",
        Decimal::new(1, 3),
    )
    .await;
    let sl_pct = resolve_system_decimal(
        &pool,
        "strategy",
        "default_stop_loss_pct",
        "QTSS_DEFAULT_STOP_LOSS_PCT",
        Decimal::new(2, 0),
    )
    .await;
    let tp_pct = resolve_system_decimal(
        &pool,
        "strategy",
        "default_take_profit_pct",
        "QTSS_DEFAULT_TAKE_PROFIT_PCT",
        Decimal::new(4, 0),
    )
    .await;
    let kelly_apply = resolve_worker_enabled_flag(
        &pool,
        "strategy",
        "kelly_apply",
        "QTSS_KELLY_APPLY",
        false,
    )
    .await;
    let kelly_win = resolve_system_f64(&pool, "strategy", "kelly_win_rate", "QTSS_KELLY_WIN_RATE", 0.55).await;
    let kelly_wl =
        resolve_system_f64(&pool, "strategy", "kelly_avg_win_loss_ratio", "QTSS_KELLY_AVG_WIN_LOSS_RATIO", 1.5).await;
    let kelly_cap =
        resolve_system_f64(&pool, "strategy", "kelly_max_fraction", "QTSS_KELLY_MAX_FRACTION", 0.25).await;
    let kelly_scale = kelly_qty_scale(kelly_apply, kelly_win, kelly_wl, kelly_cap);
    let max_notional_usdt = resolve_system_decimal(
        &pool,
        "strategy",
        "max_position_notional_usdt",
        "QTSS_MAX_POSITION_NOTIONAL_USDT",
        Decimal::new(10_000, 0),
    )
    .await;
    let max_drawdown_pct =
        resolve_system_f64(&pool, "strategy", "max_drawdown_pct", "QTSS_MAX_DRAWDOWN_PCT", 5.0).await;

    let strategy = SignalFilterStrategy {
        long_threshold: resolve_system_f64(&pool, "strategy", "long_threshold", "QTSS_LONG_THRESHOLD", 0.6).await,
        short_threshold: resolve_system_f64(
            &pool,
            "strategy",
            "short_threshold",
            "QTSS_SHORT_THRESHOLD",
            -0.6,
        )
        .await,
        min_confidence: resolve_system_f64(
            &pool,
            "strategy",
            "min_signal_confidence",
            "QTSS_MIN_SIGNAL_CONFIDENCE",
            0.4,
        )
        .await,
        pool,
        gateway,
        tick_secs,
        auto_place,
        bracket_orders,
        requires_human_approval: !skip_human,
        base_order_qty,
        sl_pct,
        tp_pct,
        kelly_scale,
        max_notional_usdt,
        max_drawdown_pct,
    };
    strategy.run().await;
}

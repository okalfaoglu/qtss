//! Whale / Nansen momentum — `strategy.*` (`system_config`).

use std::sync::Arc;
use std::time::Duration;

use qtss_common::is_trading_halted;
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{OrderIntent, OrderSide, OrderType, TimeInForce};
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
    apply_kelly_scale_to_qty, clamp_qty_by_max_notional_usdt, kelly_qty_scale,
};

struct WhaleParams {
    tick_secs: u64,
    momentum_threshold: f64,
    funding_crowding_block: f64,
    auto_place: bool,
    requires_human_approval: bool,
    base_qty: Decimal,
    kelly_scale: f64,
    max_notional_usdt: Decimal,
}

async fn load_whale_params(pool: &PgPool) -> WhaleParams {
    let kelly_apply = resolve_worker_enabled_flag(
        pool,
        "strategy",
        "kelly_apply",
        "QTSS_KELLY_APPLY",
        false,
    )
    .await;
    let kelly_win = resolve_system_f64(pool, "strategy", "kelly_win_rate", "QTSS_KELLY_WIN_RATE", 0.55).await;
    let kelly_wl =
        resolve_system_f64(pool, "strategy", "kelly_avg_win_loss_ratio", "QTSS_KELLY_AVG_WIN_LOSS_RATIO", 1.5).await;
    let kelly_cap =
        resolve_system_f64(pool, "strategy", "kelly_max_fraction", "QTSS_KELLY_MAX_FRACTION", 0.25).await;
    let skip_human = resolve_worker_enabled_flag(
        pool,
        "strategy",
        "strategy_skip_human_approval",
        "QTSS_STRATEGY_SKIP_HUMAN_APPROVAL",
        false,
    )
    .await;
    WhaleParams {
        tick_secs: resolve_worker_tick_secs(
            pool,
            "strategy",
            "whale_momentum_tick_secs",
            "QTSS_WHALE_MOMENTUM_TICK_SECS",
            120,
            30,
        )
        .await,
        momentum_threshold: resolve_system_f64(
            pool,
            "strategy",
            "whale_momentum_threshold",
            "QTSS_WHALE_MOMENTUM_THRESHOLD",
            0.45,
        )
        .await,
        funding_crowding_block: resolve_system_f64(
            pool,
            "strategy",
            "whale_funding_crowding_block",
            "QTSS_WHALE_FUNDING_CROWDING_BLOCK",
            0.0002,
        )
        .await,
        auto_place: resolve_worker_enabled_flag(
            pool,
            "strategy",
            "whale_momentum_auto_place",
            "QTSS_WHALE_MOMENTUM_AUTO_PLACE",
            false,
        )
        .await,
        requires_human_approval: !skip_human,
        base_qty: resolve_system_decimal(
            pool,
            "strategy",
            "strategy_order_qty",
            "QTSS_STRATEGY_ORDER_QTY",
            Decimal::new(1, 3),
        )
        .await,
        kelly_scale: kelly_qty_scale(kelly_apply, kelly_win, kelly_wl, kelly_cap),
        max_notional_usdt: resolve_system_decimal(
            pool,
            "strategy",
            "max_position_notional_usdt",
            "QTSS_MAX_POSITION_NOTIONAL_USDT",
            Decimal::new(10_000, 0),
        )
        .await,
    }
}

fn order_qty(p: &WhaleParams) -> Decimal {
    apply_kelly_scale_to_qty(p.base_qty, p.kelly_scale)
}

async fn tick_once(pool: &PgPool, gateway: Arc<dyn ExecutionGateway>, p: &WhaleParams) -> anyhow::Result<()> {
    let th = p.momentum_threshold;
    let rows = list_enabled_engine_symbols(pool).await?;
    for es in rows {
        let sym = es.symbol.trim().to_uppercase();
        let Some(ctx) = MarketContext::load(
            pool,
            &sym,
            es.exchange.trim(),
            es.segment.trim(),
            es.interval.trim(),
        )
        .await
        else {
            continue;
        };
        let nf = ctx.nansen_netflow_score.unwrap_or(0.0);
        let np = ctx.nansen_perp_score.unwrap_or(0.0);
        let mom = nf * 0.6 + np * 0.4;
        if mom.abs() < th {
            continue;
        }
        let mut qty = order_qty(p);
        if ctx.conflict_detected {
            match conflict_size_policy_from_db(pool).await {
                ConflictSizePolicy::Skip => {
                    tracing::debug!(%sym, "whale_momentum: TA vs on-chain conflict — atlandı");
                    continue;
                }
                ConflictSizePolicy::Half => {
                    qty = qty / Decimal::from(2u32);
                    if qty < Decimal::new(1, 8) {
                        tracing::debug!(%sym, "whale_momentum: çatışmada yarım miktar çok küçük");
                        continue;
                    }
                    tracing::debug!(%sym, "whale_momentum: çatışmada yarım miktar");
                }
            }
        }
        if let Some(mark) = ctx.last_close {
            qty = clamp_qty_by_max_notional_usdt(qty, mark, p.max_notional_usdt);
            if qty < Decimal::new(1, 8) {
                tracing::debug!(%sym, "whale_momentum: max-notional clamp — atlandı");
                continue;
            }
        }
        let fund = ctx.funding_score.unwrap_or(0.0);
        let fblock = p.funding_crowding_block;
        if mom > 0.0 && fund > fblock {
            tracing::debug!(%sym, "whale_momentum: long yönünde kalabalık funding — atlandı");
            continue;
        }
        if mom < 0.0 && fund < -fblock {
            tracing::debug!(%sym, "whale_momentum: short yönünde ters funding — atlandı");
            continue;
        }
        let side = if mom > 0.0 {
            OrderSide::Buy
        } else {
            OrderSide::Sell
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
            requires_human_approval: p.requires_human_approval,
            futures: None,
        };
        if p.auto_place {
            let Some(mark) = ctx.last_close else {
                warn!(
                    %sym,
                    "whale_momentum: last_close yok — dry için referans fiyat gerekli"
                );
                continue;
            };
            if let Err(e) = gateway.set_reference_price(&instrument, mark) {
                warn!(%sym, %e, "whale_momentum: set_reference_price");
                continue;
            }
            match gateway.place(intent).await {
                Ok(id) => info!(%sym, ?side, %id, "whale_momentum: emir"),
                Err(e) => warn!(%sym, %e, "whale_momentum: place başarısız"),
            }
        } else {
            info!(%sym, ?side, mom, "whale_momentum: sinyal (strategy.whale_momentum_auto_place)");
        }
    }
    Ok(())
}

pub async fn run(pool: PgPool, gateway: Arc<dyn ExecutionGateway>) {
    let p = load_whale_params(&pool).await;
    let tick = Duration::from_secs(p.tick_secs.max(30));
    info!(poll_secs = tick.as_secs(), "whale_momentum stratejisi");
    loop {
        tokio::time::sleep(tick).await;
        if is_trading_halted() {
            continue;
        }
        if let Err(e) = tick_once(&pool, gateway.clone(), &p).await {
            warn!(%e, "whale_momentum tick");
        }
    }
}

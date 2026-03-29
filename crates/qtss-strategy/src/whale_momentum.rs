//! Whale / Nansen momentum: `nansen_netflow_score` + `nansen_perp_score`; funding ile çakışmayı azaltır (dev guide ADIM 7).

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

fn tick_secs() -> u64 {
    std::env::var("QTSS_WHALE_MOMENTUM_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120)
        .max(30)
}

fn momentum_threshold() -> f64 {
    std::env::var("QTSS_WHALE_MOMENTUM_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.45)
}

fn funding_crowding_block() -> f64 {
    std::env::var("QTSS_WHALE_FUNDING_CROWDING_BLOCK")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0002)
}

fn auto_place() -> bool {
    std::env::var("QTSS_WHALE_MOMENTUM_AUTO_PLACE")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn order_qty() -> Decimal {
    std::env::var("QTSS_STRATEGY_ORDER_QTY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| Decimal::new(1, 3))
}

pub async fn run(pool: PgPool, gateway: Arc<dyn ExecutionGateway>) {
    let tick = Duration::from_secs(tick_secs());
    info!(poll_secs = tick.as_secs(), "whale_momentum stratejisi");
    loop {
        tokio::time::sleep(tick).await;
        if is_trading_halted() {
            continue;
        }
        if let Err(e) = tick_once(&pool, gateway.clone()).await {
            warn!(%e, "whale_momentum tick");
        }
    }
}

async fn tick_once(pool: &PgPool, gateway: Arc<dyn ExecutionGateway>) -> anyhow::Result<()> {
    let th = momentum_threshold();
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
        let fund = ctx.funding_score.unwrap_or(0.0);
        if mom > 0.0 && fund > funding_crowding_block() {
            tracing::debug!(%sym, "whale_momentum: long yönünde kalabalık funding — atlandı");
            continue;
        }
        if mom < 0.0 && fund < -funding_crowding_block() {
            tracing::debug!(%sym, "whale_momentum: short yönünde ters funding — atlandı");
            continue;
        }
        let side = if mom > 0.0 {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };
        let intent = OrderIntent {
            instrument: InstrumentId {
                exchange: ExchangeId::Binance,
                segment: MarketSegment::Futures,
                symbol: sym.clone(),
            },
            side,
            quantity: order_qty(),
            order_type: OrderType::Market,
            time_in_force: TimeInForce::Gtc,
            requires_human_approval: true,
            futures: None,
        };
        if auto_place() {
            match gateway.place(intent).await {
                Ok(id) => info!(%sym, ?side, %id, "whale_momentum: emir"),
                Err(e) => warn!(%sym, %e, "whale_momentum: place başarısız"),
            }
        } else {
            info!(%sym, ?side, mom, "whale_momentum: sinyal (QTSS_WHALE_MOMENTUM_AUTO_PLACE=1 ile yürüt)");
        }
    }
    Ok(())
}

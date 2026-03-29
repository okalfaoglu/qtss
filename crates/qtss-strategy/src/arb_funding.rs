//! Funding eşiği — Binance premium `data_snapshots` + isteğe bağlı paper iki bacak (dev guide §3.7, ADIM 7).
//!
//! Pozitif funding: spot AL + USDT-M SHORT. `QTSS_ARB_FUNDING_DRY_TWO_LEG=1` iken [`ExecutionGateway::set_reference_price`]
//! + iki market emri (paper’da futures short tabanı sıfırdan açılabilir).

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use qtss_common::is_trading_halted;
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{OrderIntent, OrderSide, OrderType, TimeInForce};
use qtss_domain::symbol::InstrumentId;
use qtss_execution::ExecutionGateway;
use qtss_storage::fetch_data_snapshot;
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::risk::{apply_kelly_scale_to_qty, clamp_qty_by_max_notional_usdt};

fn tick_secs() -> u64 {
    std::env::var("QTSS_ARB_FUNDING_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300)
        .max(60)
}

fn threshold() -> f64 {
    std::env::var("QTSS_ARB_FUNDING_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0001)
}

fn dry_two_leg() -> bool {
    std::env::var("QTSS_ARB_FUNDING_DRY_TWO_LEG")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn leg_quantity() -> Decimal {
    let base = std::env::var("QTSS_ARB_FUNDING_ORDER_QTY")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .or_else(|| {
            std::env::var("QTSS_STRATEGY_ORDER_QTY")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or_else(|| Decimal::new(1, 3));
    apply_kelly_scale_to_qty(base)
}

fn parse_premium_mark_decimal(j: &Value) -> Option<Decimal> {
    for key in ["markPrice", "indexPrice"] {
        if let Some(s) = j.get(key).and_then(|x| x.as_str()) {
            if let Ok(d) = Decimal::from_str(s.trim()) {
                if d > Decimal::ZERO {
                    return Some(d);
                }
            }
        }
    }
    None
}

pub async fn run(pool: PgPool, gateway: Arc<dyn ExecutionGateway>) {
    let tick = Duration::from_secs(tick_secs());
    let base = std::env::var("QTSS_ARB_FUNDING_SYMBOL_BASE")
        .unwrap_or_else(|_| "btc".into())
        .trim()
        .to_lowercase();
    let key = format!("binance_premium_{base}usdt");
    let symbol = format!("{}USDT", base.to_uppercase());
    let spot_inst = InstrumentId {
        exchange: ExchangeId::Binance,
        segment: MarketSegment::Spot,
        symbol: symbol.clone(),
    };
    let fut_inst = InstrumentId {
        exchange: ExchangeId::Binance,
        segment: MarketSegment::Futures,
        symbol,
    };
    info!(%key, poll_secs = tick.as_secs(), "arb_funding izleme");
    loop {
        tokio::time::sleep(tick).await;
        if is_trading_halted() {
            continue;
        }
        let row = match fetch_data_snapshot(&pool, &key).await {
            Ok(r) => r,
            Err(e) => {
                warn!(%e, "arb_funding fetch_data_snapshot");
                continue;
            }
        };
        let Some(j) = row.and_then(|r| r.response_json) else {
            continue;
        };
        let fr = j
            .get("lastFundingRate")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let th = threshold();
        if fr.abs() < th {
            continue;
        }
        let mark = parse_premium_mark_decimal(&j);
        let qty = leg_quantity();

        if fr > th {
            info!(funding_rate = fr, "arb_funding: pozitif funding — spot AL + futures SHORT");
            if dry_two_leg() {
                if let Some(px) = mark {
                    let leg_qty = clamp_qty_by_max_notional_usdt(qty, px);
                    if leg_qty <= Decimal::ZERO {
                        continue;
                    }
                    if let Err(e) = gateway.set_reference_price(&spot_inst, px) {
                        warn!(%e, "arb_funding set_reference_price spot");
                    }
                    if let Err(e) = gateway.set_reference_price(&fut_inst, px) {
                        warn!(%e, "arb_funding set_reference_price futures");
                    }
                    let spot_buy = OrderIntent {
                        instrument: spot_inst.clone(),
                        side: OrderSide::Buy,
                        quantity: leg_qty,
                        order_type: OrderType::Market,
                        time_in_force: TimeInForce::Gtc,
                        requires_human_approval: false,
                        futures: None,
                    };
                    match gateway.place(spot_buy).await {
                        Ok(id) => info!(%id, "arb_funding: paper spot AL"),
                        Err(e) => warn!(%e, "arb_funding: spot place"),
                    }
                    let fut_short = OrderIntent {
                        instrument: fut_inst.clone(),
                        side: OrderSide::Sell,
                        quantity: leg_qty,
                        order_type: OrderType::Market,
                        time_in_force: TimeInForce::Gtc,
                        requires_human_approval: false,
                        futures: None,
                    };
                    match gateway.place(fut_short).await {
                        Ok(id) => info!(%id, "arb_funding: paper futures SHORT"),
                        Err(e) => warn!(%e, "arb_funding: futures place"),
                    }
                } else {
                    warn!("arb_funding: markPrice/indexPrice yok — iki bacak atlandı");
                }
            }
        } else {
            info!(funding_rate = fr, "arb_funding: negatif funding — spot SAT + futures LONG (paper iki bacak: spot taban gerekir)");
            if dry_two_leg() {
                let Some(px) = mark else {
                    warn!("arb_funding: mark yok — negatif bacak atlandı");
                    continue;
                };
                let leg_qty = clamp_qty_by_max_notional_usdt(qty, px);
                if leg_qty <= Decimal::ZERO {
                    continue;
                }
                let _ = gateway.set_reference_price(&spot_inst, px);
                let _ = gateway.set_reference_price(&fut_inst, px);
                let spot_sell = OrderIntent {
                    instrument: spot_inst.clone(),
                    side: OrderSide::Sell,
                    quantity: leg_qty,
                    order_type: OrderType::Market,
                    time_in_force: TimeInForce::Gtc,
                    requires_human_approval: false,
                    futures: None,
                };
                match gateway.place(spot_sell).await {
                    Ok(id) => info!(%id, "arb_funding: paper spot SAT"),
                    Err(e) => warn!(%e, "arb_funding: negatif bacak spot SAT (çoğu paper defterde taban yok)"),
                }
                let fut_long = OrderIntent {
                    instrument: fut_inst.clone(),
                    side: OrderSide::Buy,
                    quantity: leg_qty,
                    order_type: OrderType::Market,
                    time_in_force: TimeInForce::Gtc,
                    requires_human_approval: false,
                    futures: None,
                };
                match gateway.place(fut_long).await {
                    Ok(id) => info!(%id, "arb_funding: paper futures LONG"),
                    Err(e) => warn!(%e, "arb_funding: futures LONG place"),
                }
            }
        }
    }
}

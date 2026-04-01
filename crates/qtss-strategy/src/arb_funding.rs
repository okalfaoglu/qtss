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
use qtss_storage::{
    fetch_data_snapshot, resolve_system_decimal, resolve_system_f64, resolve_system_string,
    resolve_worker_enabled_flag, resolve_worker_tick_secs,
};
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::risk::{
    apply_kelly_scale_to_qty, clamp_qty_by_max_notional_usdt, kelly_qty_scale,
};

struct ArbFundingParams {
    tick_secs: u64,
    threshold: f64,
    dry_two_leg: bool,
    leg_qty: Decimal,
    symbol_base: String,
    max_notional_usdt: Decimal,
}

async fn load_arb_params(pool: &PgPool) -> ArbFundingParams {
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
    let kelly_scale = kelly_qty_scale(kelly_apply, kelly_win, kelly_wl, kelly_cap);
    let mut base = resolve_system_decimal(
        pool,
        "strategy",
        "arb_funding_order_qty",
        "QTSS_ARB_FUNDING_ORDER_QTY",
        Decimal::ZERO,
    )
    .await;
    if base <= Decimal::ZERO {
        base = resolve_system_decimal(
            pool,
            "strategy",
            "strategy_order_qty",
            "QTSS_STRATEGY_ORDER_QTY",
            Decimal::new(1, 3),
        )
        .await;
    }
    let leg_qty = apply_kelly_scale_to_qty(base, kelly_scale);
    ArbFundingParams {
        tick_secs: resolve_worker_tick_secs(
            pool,
            "strategy",
            "arb_funding_tick_secs",
            "QTSS_ARB_FUNDING_TICK_SECS",
            300,
            60,
        )
        .await,
        threshold: resolve_system_f64(
            pool,
            "strategy",
            "arb_funding_threshold",
            "QTSS_ARB_FUNDING_THRESHOLD",
            0.0001,
        )
        .await,
        dry_two_leg: resolve_worker_enabled_flag(
            pool,
            "strategy",
            "arb_funding_dry_two_leg",
            "QTSS_ARB_FUNDING_DRY_TWO_LEG",
            false,
        )
        .await,
        leg_qty,
        symbol_base: resolve_system_string(pool, "strategy", "arb_funding_symbol_base", "QTSS_ARB_FUNDING_SYMBOL_BASE", "btc")
            .await
            .trim()
            .to_lowercase(),
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
    let p = load_arb_params(&pool).await;
    let tick = Duration::from_secs(p.tick_secs.max(60));
    let base = p.symbol_base.clone();
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
        let th = p.threshold;
        if fr.abs() < th {
            continue;
        }
        let mark = parse_premium_mark_decimal(&j);
        let qty = p.leg_qty;

        if fr > th {
            info!(funding_rate = fr, "arb_funding: pozitif funding — spot AL + futures SHORT");
            if p.dry_two_leg {
                if let Some(px) = mark {
                    let leg_qty = clamp_qty_by_max_notional_usdt(qty, px, p.max_notional_usdt);
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
            if p.dry_two_leg {
                let Some(px) = mark else {
                    warn!("arb_funding: mark yok — negatif bacak atlandı");
                    continue;
                };
                let leg_qty = clamp_qty_by_max_notional_usdt(qty, px, p.max_notional_usdt);
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

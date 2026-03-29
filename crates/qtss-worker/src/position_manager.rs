//! Pozisyon özeti + SL/TP kontrolü (dev guide ADIM 9, §3.5).
//!
//! `exchange_orders` dolumlarından net pozisyon tahmini; `market_bars` son kapanış ile eşik.
//! `QTSS_POSITION_MANAGER_DRY_CLOSE_ENABLED=1` iken [`DryRunGateway`] ile simüle kapatma dener.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{FuturesExecutionExtras, OrderIntent, OrderSide, OrderType, TimeInForce};
use qtss_domain::symbol::InstrumentId;
use qtss_execution::{DryRunGateway, ExecutionGateway};
use qtss_storage::{list_recent_bars, ExchangeOrderRepository, ExchangeOrderRow};
use rust_decimal::Decimal;
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

fn enabled() -> bool {
    std::env::var("QTSS_POSITION_MANAGER_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn dry_close_enabled() -> bool {
    std::env::var("QTSS_POSITION_MANAGER_DRY_CLOSE_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn tick_secs() -> u64 {
    std::env::var("QTSS_POSITION_MANAGER_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10)
        .max(5)
}

fn sl_pct() -> Decimal {
    std::env::var("QTSS_DEFAULT_STOP_LOSS_PCT")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .unwrap_or_else(|| Decimal::new(2, 0))
}

fn tp_pct() -> Decimal {
    std::env::var("QTSS_DEFAULT_TAKE_PROFIT_PCT")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .unwrap_or_else(|| Decimal::new(4, 0))
}

fn min_qty_filter() -> Decimal {
    Decimal::new(1, 8)
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct PosKey {
    user_id: Uuid,
    exchange: String,
    segment: String,
    symbol: String,
}

#[derive(Clone, Default)]
struct LongBook {
    qty: Decimal,
    cost: Decimal,
}

fn parse_decimal_field(v: &JsonValue, k: &str) -> Option<Decimal> {
    v.get(k)
        .and_then(|x| x.as_str())
        .and_then(|s| Decimal::from_str(s.trim()).ok())
}

fn parse_executed_qty(venue: &JsonValue) -> Option<Decimal> {
    parse_decimal_field(venue, "executedQty")
}

fn parse_avg_price(venue: &JsonValue, qty: Decimal) -> Option<Decimal> {
    parse_decimal_field(venue, "avgPrice").or_else(|| {
        let qq = parse_decimal_field(venue, "cummulativeQuoteQty")?;
        if qty > Decimal::ZERO {
            Some(qq / qty)
        } else {
            None
        }
    })
}

fn intent_side(intent: &JsonValue) -> Option<OrderSide> {
    let s = intent.get("side")?.as_str()?.trim().to_ascii_lowercase();
    match s.as_str() {
        "buy" => Some(OrderSide::Buy),
        "sell" => Some(OrderSide::Sell),
        _ => None,
    }
}

fn update_long_book(book: &mut LongBook, side: OrderSide, qty: Decimal, price: Decimal) {
    match side {
        OrderSide::Buy => {
            book.cost += price * qty;
            book.qty += qty;
        }
        OrderSide::Sell => {
            let take = qty.min(book.qty);
            if take > Decimal::ZERO && book.qty > Decimal::ZERO {
                let avg = book.cost / book.qty;
                book.cost -= avg * take;
                book.qty -= take;
            }
        }
    }
    if book.qty <= Decimal::ZERO {
        book.qty = Decimal::ZERO;
        book.cost = Decimal::ZERO;
    }
}

fn aggregate_long_books(rows: &[ExchangeOrderRow]) -> HashMap<PosKey, LongBook> {
    let mut sorted: Vec<_> = rows.iter().collect();
    sorted.sort_by_key(|r| r.created_at);
    let mut m: HashMap<PosKey, LongBook> = HashMap::new();
    for row in sorted {
        let Some(venue) = row.venue_response.as_ref() else {
            continue;
        };
        let Some(qty) = parse_executed_qty(venue) else {
            continue;
        };
        if qty <= Decimal::ZERO {
            continue;
        };
        let Some(side) = intent_side(&row.intent) else {
            continue;
        };
        let Some(price) = parse_avg_price(venue, qty) else {
            continue;
        };
        let key = PosKey {
            user_id: row.user_id,
            exchange: row.exchange.trim().to_string(),
            segment: row.segment.trim().to_string(),
            symbol: row.symbol.trim().to_uppercase(),
        };
        let b = m.entry(key).or_default();
        update_long_book(b, side, qty, price);
    }
    m
}

async fn last_close_price(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    interval: &str,
) -> Option<Decimal> {
    let bars = list_recent_bars(pool, exchange, segment, symbol, interval, 1)
        .await
        .ok()?;
    bars.into_iter().next().map(|b| b.close)
}

pub async fn position_manager_loop(pool: PgPool) {
    if !enabled() {
        info!("QTSS_POSITION_MANAGER_ENABLED kapalı — position_manager_loop çıkıyor");
        return;
    }
    let tick = Duration::from_secs(tick_secs());
    let repo = ExchangeOrderRepository::new(pool.clone());
    let gateway: Option<Arc<DryRunGateway>> = if dry_close_enabled() {
        Some(crate::strategy_runner::dry_gateway_from_env())
    } else {
        None
    };
    let bar_interval = std::env::var("QTSS_POSITION_MANAGER_BAR_INTERVAL").unwrap_or_else(|_| "1m".into());
    info!(
        poll_secs = tick.as_secs(),
        dry_close = gateway.is_some(),
        "position_manager_loop: SL/TP izleme"
    );
    let sl = sl_pct() / Decimal::from(100u32);
    let tp = tp_pct() / Decimal::from(100u32);
    let min_q = min_qty_filter();
    loop {
        tokio::time::sleep(tick).await;
        let rows = match repo.list_recent_filled_orders_global(1500).await {
            Ok(r) => r,
            Err(e) => {
                warn!(%e, "position_manager list_recent_filled_orders_global");
                continue;
            }
        };
        let books = aggregate_long_books(&rows);
        for (key, book) in books {
            if book.qty < min_q {
                continue;
            }
            let entry = if book.qty > Decimal::ZERO {
                book.cost / book.qty
            } else {
                continue;
            };
            let Some(mark) = last_close_price(
                &pool,
                &key.exchange,
                &key.segment,
                &key.symbol,
                bar_interval.trim(),
            )
            .await
            else {
                tracing::debug!(symbol = %key.symbol, "position_manager: bar yok");
                continue;
            };
            let sl_price = entry * (Decimal::ONE - sl);
            let tp_price = entry * (Decimal::ONE + tp);
            let hit_sl = mark <= sl_price;
            let hit_tp = mark >= tp_price;
            if !hit_sl && !hit_tp {
                continue;
            }
            warn!(
                user_id = %key.user_id,
                symbol = %key.symbol,
                segment = %key.segment,
                net_qty = %book.qty,
                entry = %entry,
                mark = %mark,
                hit_sl,
                hit_tp,
                "position_manager: SL/TP eşiği (kapatma dry modda opsiyonel)"
            );
            if let Some(ref gw) = gateway {
                let inst = InstrumentId {
                    exchange: ExchangeId::Binance,
                    segment: if key.segment.eq_ignore_ascii_case("futures") {
                        MarketSegment::Futures
                    } else {
                        MarketSegment::Spot
                    },
                    symbol: key.symbol.clone(),
                };
                if let Err(e) = gw.set_mark(&inst, mark) {
                    warn!(%e, "position_manager dry set_mark");
                    continue;
                }
                let close_side = OrderSide::Sell;
                let futures = if key.segment.eq_ignore_ascii_case("futures") {
                    Some(FuturesExecutionExtras {
                        position_side: None,
                        reduce_only: Some(true),
                    })
                } else {
                    None
                };
                let intent = OrderIntent {
                    instrument: inst,
                    side: close_side,
                    quantity: book.qty,
                    order_type: OrderType::Market,
                    time_in_force: TimeInForce::Gtc,
                    requires_human_approval: false,
                    futures,
                };
                match gw.place(intent).await {
                    Ok(cid) => info!(%cid, symbol = %key.symbol, "position_manager: dry kapatma emri"),
                    Err(e) => warn!(%e, symbol = %key.symbol, "position_manager: dry place başarısız"),
                }
            }
        }
    }
}

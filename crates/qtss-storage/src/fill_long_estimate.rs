//! Net long estimate from filled `exchange_orders` (same rules as `position_manager`).

use std::collections::HashMap;
use std::str::FromStr;

use qtss_domain::orders::OrderSide;
use rust_decimal::Decimal;
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::exchange_orders::ExchangeOrderRow;

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct FillPositionKey {
    pub user_id: Uuid,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
}

#[derive(Clone, Default)]
struct LongBook {
    qty: Decimal,
    cost: Decimal,
}

#[derive(Default)]
pub(crate) struct BookWithOrg {
    book: LongBook,
    org_id: Option<Uuid>,
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

/// Aggregates net long books per (user, exchange, segment, symbol) from fill-like rows.
pub(crate) fn aggregate_long_books_from_fills(
    rows: &[ExchangeOrderRow],
) -> HashMap<FillPositionKey, BookWithOrg> {
    let mut sorted: Vec<_> = rows.iter().collect();
    sorted.sort_by_key(|r| r.created_at);
    let mut m: HashMap<FillPositionKey, BookWithOrg> = HashMap::new();
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
        let key = FillPositionKey {
            user_id: row.user_id,
            exchange: row.exchange.trim().to_string(),
            segment: row.segment.trim().to_string(),
            symbol: row.symbol.trim().to_uppercase(),
        };
        let e = m.entry(key).or_default();
        if e.org_id.is_none() {
            e.org_id = Some(row.org_id);
        }
        update_long_book(&mut e.book, side, qty, price);
    }
    m
}

/// Distinct symbols with estimated net long ≥ `min_qty`.
pub fn symbols_with_positive_long_from_fills(
    rows: &[ExchangeOrderRow],
    min_qty: Decimal,
) -> Vec<String> {
    let m = aggregate_long_books_from_fills(rows);
    let mut syms: Vec<String> = m
        .into_iter()
        .filter(|(_, v)| v.book.qty >= min_qty)
        .map(|(k, _)| k.symbol)
        .collect();
    syms.sort();
    syms.dedup();
    syms
}

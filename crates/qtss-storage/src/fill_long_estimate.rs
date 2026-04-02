//! Net position estimate from filled `exchange_orders` (same rules as `position_manager`).
//! Tracks both long and short books for futures bidirectional support.

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

#[derive(Clone, Default)]
struct ShortBook {
    qty: Decimal,  // absolute short quantity (positive number)
    cost: Decimal,
}

#[derive(Default)]
pub(crate) struct BookWithOrg {
    book: LongBook,
    short: ShortBook,
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

/// Update short book: sell opens short, buy closes short.
fn update_short_book(book: &mut ShortBook, side: OrderSide, qty: Decimal, price: Decimal) {
    match side {
        OrderSide::Sell => {
            book.cost += price * qty;
            book.qty += qty;
        }
        OrderSide::Buy => {
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

/// For futures: buy opens/adds long and closes short; sell opens/adds short and closes long.
fn update_books_bidirectional(
    long: &mut LongBook,
    short: &mut ShortBook,
    side: OrderSide,
    qty: Decimal,
    price: Decimal,
) {
    match side {
        OrderSide::Buy => {
            // First close short, remainder opens long
            let close_short = qty.min(short.qty);
            if close_short > Decimal::ZERO {
                update_short_book(short, OrderSide::Buy, close_short, price);
            }
            let remainder = qty - close_short;
            if remainder > Decimal::ZERO {
                update_long_book(long, OrderSide::Buy, remainder, price);
            }
        }
        OrderSide::Sell => {
            // First close long, remainder opens short
            let close_long = qty.min(long.qty);
            if close_long > Decimal::ZERO {
                update_long_book(long, OrderSide::Sell, close_long, price);
            }
            let remainder = qty - close_long;
            if remainder > Decimal::ZERO {
                update_short_book(short, OrderSide::Sell, remainder, price);
            }
        }
    }
}

/// Aggregates net long books per (user, exchange, segment, symbol) from fill-like rows.
pub(crate) fn aggregate_long_books_from_fills(rows: &[ExchangeOrderRow]) -> HashMap<FillPositionKey, BookWithOrg> {
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
        let is_futures = row.segment.trim().eq_ignore_ascii_case("futures")
            || row.segment.trim().eq_ignore_ascii_case("usdm");
        if is_futures {
            update_books_bidirectional(&mut e.book, &mut e.short, side, qty, price);
        } else {
            update_long_book(&mut e.book, side, qty, price);
        }
    }
    m
}

/// Distinct symbols with any open position (long OR short) ≥ `min_qty`.
pub fn symbols_with_open_positions_from_fills(
    rows: &[ExchangeOrderRow],
    min_qty: Decimal,
) -> Vec<String> {
    let m = aggregate_long_books_from_fills(rows);
    let mut syms: Vec<String> = m
        .into_iter()
        .filter(|(_, v)| v.book.qty >= min_qty || v.short.qty >= min_qty)
        .map(|(k, _)| k.symbol)
        .collect();
    syms.sort();
    syms.dedup();
    syms
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    fn make_order(segment: &str, symbol: &str, side: &str, qty: &str, price: &str) -> ExchangeOrderRow {
        ExchangeOrderRow {
            id: Uuid::new_v4(),
            org_id: Uuid::nil(),
            user_id: Uuid::nil(),
            exchange: "binance".into(),
            segment: segment.into(),
            symbol: symbol.into(),
            client_order_id: Uuid::new_v4(),
            status: "filled".into(),
            intent: json!({ "side": side }),
            venue_order_id: None,
            venue_response: Some(json!({
                "executedQty": qty,
                "avgPrice": price,
            })),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn spot_long_only() {
        let rows = vec![
            make_order("spot", "BTCUSDT", "buy", "1.0", "50000"),
        ];
        let min = Decimal::new(1, 8);
        let long = symbols_with_positive_long_from_fills(&rows, min);
        let all = symbols_with_open_positions_from_fills(&rows, min);
        assert_eq!(long, vec!["BTCUSDT"]);
        assert_eq!(all, vec!["BTCUSDT"]);
    }

    #[test]
    fn spot_sell_closes_long_no_short() {
        let rows = vec![
            make_order("spot", "ETHUSDT", "buy", "10.0", "3000"),
            make_order("spot", "ETHUSDT", "sell", "10.0", "3100"),
        ];
        let min = Decimal::new(1, 8);
        let long = symbols_with_positive_long_from_fills(&rows, min);
        let all = symbols_with_open_positions_from_fills(&rows, min);
        assert!(long.is_empty());
        // Spot sell should NOT create short
        assert!(all.is_empty());
    }

    #[test]
    fn futures_short_detected() {
        let rows = vec![
            make_order("futures", "BTCUSDT", "sell", "0.5", "60000"),
        ];
        let min = Decimal::new(1, 8);
        let long = symbols_with_positive_long_from_fills(&rows, min);
        let all = symbols_with_open_positions_from_fills(&rows, min);
        // Long list should be empty (no long position)
        assert!(long.is_empty());
        // Open positions should include the short
        assert_eq!(all, vec!["BTCUSDT"]);
    }

    #[test]
    fn futures_bidirectional_buy_closes_short() {
        let rows = vec![
            make_order("futures", "ETHUSDT", "sell", "5.0", "3000"),
            make_order("futures", "ETHUSDT", "buy", "5.0", "2900"),
        ];
        let min = Decimal::new(1, 8);
        let all = symbols_with_open_positions_from_fills(&rows, min);
        // Short was closed by buy → no open position
        assert!(all.is_empty());
    }

    #[test]
    fn futures_buy_exceeds_short_creates_long() {
        let rows = vec![
            make_order("futures", "SOLUSDT", "sell", "10.0", "100"),
            make_order("futures", "SOLUSDT", "buy", "15.0", "95"),
        ];
        let min = Decimal::new(1, 8);
        let long = symbols_with_positive_long_from_fills(&rows, min);
        let all = symbols_with_open_positions_from_fills(&rows, min);
        // 15 buy - 10 short close = 5 long
        assert_eq!(long, vec!["SOLUSDT"]);
        assert_eq!(all, vec!["SOLUSDT"]);
    }

    #[test]
    fn futures_usdm_segment_treated_as_futures() {
        let rows = vec![
            make_order("usdm", "XRPUSDT", "sell", "100.0", "0.50"),
        ];
        let min = Decimal::new(1, 8);
        let all = symbols_with_open_positions_from_fills(&rows, min);
        assert_eq!(all, vec!["XRPUSDT"]);
    }
}

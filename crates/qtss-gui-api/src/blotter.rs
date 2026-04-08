//! `/v2/blotter` wire types -- Faz 5 Adim (g).
//!
//! The Order Blotter merges submitted orders and recent fills into a
//! single chronological feed for the operator. The wire DTO is
//! deliberately narrower than the storage rows -- the blotter card
//! only renders venue/symbol/side/qty/price/status, so we strip the
//! UUIDs, raw venue blobs, and the JSON intent envelope to keep the
//! payload tight.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// One row in the blotter table. `kind` distinguishes order vs fill so
/// the React side can pick an icon and avoid sorting on a heterogeneous
/// `Option<status>` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BlotterEntry {
    Order {
        at: DateTime<Utc>,
        venue: String,
        segment: String,
        symbol: String,
        side: String,
        order_type: String,
        quantity: Option<Decimal>,
        price: Option<Decimal>,
        status: String,
        venue_order_id: Option<i64>,
    },
    Fill {
        at: DateTime<Utc>,
        venue: String,
        segment: String,
        symbol: String,
        venue_order_id: i64,
        venue_trade_id: Option<i64>,
        price: Option<Decimal>,
        quantity: Option<Decimal>,
        fee: Option<Decimal>,
        fee_asset: Option<String>,
    },
}

impl BlotterEntry {
    /// Sort key (newest first) used by [`merge_blotter`].
    pub fn at(&self) -> DateTime<Utc> {
        match self {
            BlotterEntry::Order { at, .. } | BlotterEntry::Fill { at, .. } => *at,
        }
    }
}

/// Whole `/v2/blotter` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlotterFeed {
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<BlotterEntry>,
}

/// Pure merger -- the route handler hands in already-fetched orders
/// and fills, this sorts them newest-first and trims to `limit`.
pub fn merge_blotter(orders: Vec<BlotterEntry>, fills: Vec<BlotterEntry>, limit: usize) -> Vec<BlotterEntry> {
    let mut all: Vec<BlotterEntry> = orders.into_iter().chain(fills.into_iter()).collect();
    all.sort_by(|a, b| b.at().cmp(&a.at()));
    all.truncate(limit);
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn order(secs: i64) -> BlotterEntry {
        BlotterEntry::Order {
            at: DateTime::from_timestamp(1_700_000_000 + secs, 0).unwrap(),
            venue: "binance".into(),
            segment: "spot".into(),
            symbol: "BTCUSDT".into(),
            side: "buy".into(),
            order_type: "limit".into(),
            quantity: Some(dec!(0.1)),
            price: Some(dec!(50000)),
            status: "submitted".into(),
            venue_order_id: Some(123),
        }
    }

    fn fill(secs: i64) -> BlotterEntry {
        BlotterEntry::Fill {
            at: DateTime::from_timestamp(1_700_000_000 + secs, 0).unwrap(),
            venue: "binance".into(),
            segment: "spot".into(),
            symbol: "BTCUSDT".into(),
            venue_order_id: 123,
            venue_trade_id: Some(7),
            price: Some(dec!(50001)),
            quantity: Some(dec!(0.05)),
            fee: Some(dec!(0.001)),
            fee_asset: Some("BNB".into()),
        }
    }

    #[test]
    fn merge_sorts_newest_first() {
        let merged = merge_blotter(vec![order(10), order(30)], vec![fill(20), fill(40)], 10);
        let times: Vec<i64> = merged.iter().map(|e| e.at().timestamp()).collect();
        assert_eq!(times, vec![1_700_000_040, 1_700_000_030, 1_700_000_020, 1_700_000_010]);
    }

    #[test]
    fn merge_respects_limit() {
        let merged = merge_blotter(vec![order(10), order(20), order(30)], vec![fill(15)], 2);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn merge_handles_empty_sides() {
        assert!(merge_blotter(vec![], vec![], 10).is_empty());
        let only_orders = merge_blotter(vec![order(1), order(2)], vec![], 10);
        assert_eq!(only_orders.len(), 2);
    }

    #[test]
    fn json_round_trip_tags_kind() {
        let feed = BlotterFeed {
            generated_at: Utc::now(),
            entries: vec![order(10), fill(20)],
        };
        let j = serde_json::to_string(&feed).unwrap();
        assert!(j.contains("\"kind\":\"order\""));
        assert!(j.contains("\"kind\":\"fill\""));
        let back: BlotterFeed = serde_json::from_str(&j).unwrap();
        assert_eq!(back.entries.len(), 2);
    }
}

#![allow(dead_code)]
//! `GET /v2/blotter` -- Faz 5 Adim (g).
//!
//! Merges the user's recent submitted orders (`exchange_orders`) and
//! recent fills (`exchange_fills`) into a single chronological feed
//! for the Order Blotter card. The DTOs in qtss_gui_api::blotter strip
//! UUIDs and raw venue blobs so the wire stays tight.

use std::str::FromStr;

use axum::extract::{Extension, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

use qtss_gui_api::{merge_blotter, BlotterEntry, BlotterFeed};
use qtss_storage::{ExchangeFillRow, ExchangeOrderRow};

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct BlotterQuery {
    /// Combined cap on the merged feed (default 100).
    pub limit: Option<usize>,
    /// Per-source fetch cap (default = 2 * limit).
    pub source_limit: Option<i64>,
}

pub fn v2_blotter_router() -> Router<SharedState> {
    Router::new().route("/v2/blotter", get(get_blotter))
}

async fn get_blotter(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<BlotterQuery>,
) -> Result<Json<BlotterFeed>, ApiError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::bad_request("invalid token sub"))?;
    let limit = q.limit.unwrap_or(100).clamp(1, 1_000);
    let source_limit = q
        .source_limit
        .unwrap_or((limit as i64).saturating_mul(2))
        .clamp(1, 1_000);

    let orders = st
        .exchange_orders
        .list_for_user(user_id, source_limit)
        .await?;
    let fills = st
        .exchange_fills
        .list_recent_for_user(user_id, source_limit)
        .await?;

    let order_entries: Vec<BlotterEntry> = orders.into_iter().map(order_to_entry).collect();
    let fill_entries: Vec<BlotterEntry> = fills.into_iter().map(fill_to_entry).collect();
    let entries = merge_blotter(order_entries, fill_entries, limit);

    Ok(Json(BlotterFeed {
        generated_at: Utc::now(),
        entries,
    }))
}

fn order_to_entry(row: ExchangeOrderRow) -> BlotterEntry {
    let side = json_string(&row.intent, "side").unwrap_or_else(|| "unknown".into());
    let order_type = json_string(&row.intent, "kind")
        .or_else(|| json_string(&row.intent, "type"))
        .unwrap_or_else(|| "unknown".into());
    let quantity = json_decimal(&row.intent, "quantity")
        .or_else(|| json_decimal(&row.intent, "qty"));
    let price = json_decimal(&row.intent, "price")
        .or_else(|| json_decimal(&row.intent, "limit_price"));

    BlotterEntry::Order {
        at: row.updated_at,
        venue: row.exchange,
        segment: row.segment,
        symbol: row.symbol,
        side,
        order_type,
        quantity,
        price,
        status: row.status,
        venue_order_id: row.venue_order_id,
    }
}

fn fill_to_entry(row: ExchangeFillRow) -> BlotterEntry {
    BlotterEntry::Fill {
        at: row.event_time,
        venue: row.exchange,
        segment: row.segment,
        symbol: row.symbol,
        venue_order_id: row.venue_order_id,
        venue_trade_id: row.venue_trade_id,
        price: row.fill_price,
        quantity: row.fill_quantity,
        fee: row.fee,
        fee_asset: row.fee_asset,
    }
}

fn json_string(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn json_decimal(v: &serde_json::Value, key: &str) -> Option<Decimal> {
    let raw = v.get(key)?;
    if let Some(s) = raw.as_str() {
        return Decimal::from_str(s).ok();
    }
    if let Some(n) = raw.as_f64() {
        use rust_decimal::prelude::FromPrimitive;
        return Decimal::from_f64(n);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_decimal_handles_string_and_number() {
        let v = json!({ "price": "50000.12", "qty": 0.5 });
        assert_eq!(json_decimal(&v, "price"), Some(Decimal::from_str("50000.12").unwrap()));
        assert!(json_decimal(&v, "qty").is_some());
    }

    #[test]
    fn json_string_returns_none_for_missing() {
        let v = json!({ "side": "buy" });
        assert_eq!(json_string(&v, "side").as_deref(), Some("buy"));
        assert!(json_string(&v, "missing").is_none());
    }

    #[test]
    fn query_parses() {
        let q: BlotterQuery = serde_urlencoded::from_str("limit=50&source_limit=200").unwrap();
        assert_eq!(q.limit, Some(50));
        assert_eq!(q.source_limit, Some(200));
    }
}

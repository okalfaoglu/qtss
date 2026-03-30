//! Binance canlı emir — kullanıcının `exchange_accounts` kaydından anahtar okunur.

use std::sync::Arc;

use axum::extract::{Extension, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use qtss_binance::{
    venue_order_id_from_binance_order_response, BinanceClient, BinanceClientConfig,
};
use qtss_domain::exchange::MarketSegment;
use qtss_domain::orders::OrderIntent;
use qtss_execution::{BinanceLiveGateway, ExecutionError};
use qtss_storage::ExchangeOrderRow;

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct PlaceBinanceBody {
    pub intent: OrderIntent,
}

#[derive(Deserialize)]
pub struct CancelBinanceBody {
    pub client_order_id: Uuid,
    pub symbol: String,
    /// `spot` veya `futures` / `fapi` / `usdt_futures`
    pub segment: String,
}

#[derive(Deserialize)]
pub struct ListBinanceOrdersQuery {
    #[serde(default = "list_binance_orders_default_limit")]
    limit: i64,
    since: Option<String>,
}

fn list_binance_orders_default_limit() -> i64 {
    200
}

pub fn orders_binance_read_router() -> Router<SharedState> {
    Router::new().route("/orders/binance", get(list_my_orders))
}

pub fn orders_binance_write_router() -> Router<SharedState> {
    Router::new()
        .route("/orders/binance/place", post(place_order))
        .route("/orders/binance/cancel", post(cancel_order))
}

async fn list_my_orders(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<ListBinanceOrdersQuery>,
) -> Result<Json<Vec<ExchangeOrderRow>>, ApiError> {
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("geçersiz token sub"))?;
    let lim = q.limit.clamp(1, 1000);
    let since_dt: Option<DateTime<Utc>> = match &q.since {
        None => None,
        Some(raw) => {
            let t = raw.trim();
            if t.is_empty() {
                None
            } else {
                Some(
                    DateTime::parse_from_rfc3339(t)
                        .map_err(|_| {
                            ApiError::bad_request(
                                "invalid since — use RFC3339 e.g. 2026-01-01T00:00:00Z",
                            )
                        })?
                        .with_timezone(&Utc),
                )
            }
        }
    };
    let rows = st
        .exchange_orders
        .list_for_user_filtered(user_id, since_dt, lim)
        .await?;
    Ok(Json(rows))
}

fn segment_db_key(segment: MarketSegment) -> Result<&'static str, ApiError> {
    match segment {
        MarketSegment::Spot => Ok("spot"),
        MarketSegment::Futures => Ok("futures"),
        _ => Err(ApiError::bad_request(
            "bu segment için exchange_accounts eşlemesi yok",
        )),
    }
}

async fn place_order(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<PlaceBinanceBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let org_id = Uuid::parse_str(&claims.org_id)
        .map_err(|_| ApiError::bad_request("geçersiz token org_id"))?;
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("geçersiz token sub"))?;
    let seg = segment_db_key(body.intent.instrument.segment)?;
    let creds = st
        .exchange_accounts
        .binance_for_user(user_id, seg)
        .await?
        .ok_or_else(|| {
            ApiError::bad_request(format!(
                "Binance {} API anahtarı yok — exchange_accounts tablosuna ekleyin",
                seg
            ))
        })?;
    let cfg = BinanceClientConfig::mainnet_with_keys(creds.api_key, creds.api_secret);
    let client = Arc::new(
        BinanceClient::new(cfg).map_err(|e| ApiError::internal(e.to_string()))?,
    );
    let intent = body.intent;
    let symbol = intent.instrument.symbol.clone();
    let intent_record = intent.clone();
    let gw = BinanceLiveGateway::new(client);
    let (id, venue_json) = gw
        .place_with_venue_response(intent)
        .await
        .map_err(|e: ExecutionError| ApiError::internal(e.to_string()))?;
    let venue_oid = venue_order_id_from_binance_order_response(&venue_json);
    st.exchange_orders
        .insert_submitted(
            org_id,
            user_id,
            "binance",
            seg,
            &symbol,
            id,
            &intent_record,
            venue_oid,
            Some(venue_json),
        )
        .await?;
    Ok(Json(serde_json::json!({
        "client_order_id": id,
        "status": "accepted"
    })))
}

async fn cancel_order(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<CancelBinanceBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("geçersiz token sub"))?;
    let seg = match body.segment.as_str() {
        "futures" | "fapi" | "usdt_futures" => "futures",
        _ => "spot",
    };
    let creds = st
        .exchange_accounts
        .binance_for_user(user_id, seg)
        .await?
        .ok_or_else(|| ApiError::bad_request(format!("Binance {seg} API anahtarı yok")))?;
    let cfg = BinanceClientConfig::mainnet_with_keys(creds.api_key, creds.api_secret);
    let client =
        BinanceClient::new(cfg).map_err(|e| ApiError::internal(e.to_string()))?;
    let cid = body.client_order_id.as_simple().to_string();
    match seg {
        "futures" => {
            client
                .fapi_cancel_order(&body.symbol, None, Some(&cid))
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
        }
        _ => {
            client
                .spot_cancel_order(&body.symbol, None, Some(&cid), None)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
        }
    }
    let _ = st
        .exchange_orders
        .mark_canceled(user_id, body.client_order_id)
        .await?;
    Ok(Json(serde_json::json!({ "status": "canceled" })))
}

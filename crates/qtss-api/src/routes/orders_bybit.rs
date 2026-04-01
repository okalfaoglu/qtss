//! Bybit USDT linear — keys from `exchange_accounts` (`exchange = bybit`, `segment = futures`).

use axum::extract::{Extension, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::OrderIntent;
use qtss_execution::{venue_order_id_from_bybit_v5_response, BybitLiveGateway, ExecutionError};

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct PlaceBybitBody {
    pub intent: OrderIntent,
}

#[derive(Deserialize)]
pub struct CancelBybitBody {
    pub client_order_id: Uuid,
    /// Linear symbol, e.g. `BTCUSDT`
    pub symbol: String,
}

pub fn orders_bybit_write_router() -> Router<SharedState> {
    Router::new()
        .route("/orders/bybit/place", post(place_order))
        .route("/orders/bybit/cancel", post(cancel_order))
}

async fn place_order(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<PlaceBybitBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let org_id = Uuid::parse_str(&claims.org_id)
        .map_err(|_| ApiError::bad_request("invalid token org_id"))?;
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("invalid token sub"))?;

    if body.intent.instrument.exchange != ExchangeId::Bybit {
        return Err(ApiError::bad_request(
            "intent.instrument.exchange must be bybit",
        ));
    }
    if body.intent.instrument.segment != MarketSegment::Futures {
        return Err(ApiError::bad_request(
            "bybit API: only futures (linear) segment is supported",
        ));
    }

    let seg = "futures";
    let creds = st
        .exchange_accounts
        .credentials_for_user(user_id, "bybit", seg)
        .await?
        .ok_or_else(|| {
            ApiError::bad_request(
                "No Bybit futures API key — add a row to exchange_accounts (exchange=bybit)",
            )
        })?;

    let gw = BybitLiveGateway::mainnet(creds.api_key, creds.api_secret);
    let intent = body.intent;
    let symbol = intent.instrument.symbol.clone();
    let intent_record = intent.clone();
    let (id, venue_json) = gw
        .place_with_venue_response(intent)
        .await
        .map_err(|e: ExecutionError| ApiError::internal(e.to_string()))?;
    let venue_oid = venue_order_id_from_bybit_v5_response(&venue_json);
    st.exchange_orders
        .insert_submitted(
            org_id,
            user_id,
            "bybit",
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
    Json(body): Json<CancelBybitBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("invalid token sub"))?;
    let seg = "futures";
    let creds = st
        .exchange_accounts
        .credentials_for_user(user_id, "bybit", seg)
        .await?
        .ok_or_else(|| ApiError::bad_request("No Bybit futures API key"))?;

    let gw = BybitLiveGateway::mainnet(creds.api_key, creds.api_secret);
    gw.cancel_linear_by_order_link(&body.symbol, &body.client_order_id)
        .await
        .map_err(|e: ExecutionError| ApiError::internal(e.to_string()))?;

    let _ = st
        .exchange_orders
        .mark_canceled(user_id, body.client_order_id)
        .await?;
    Ok(Json(serde_json::json!({ "status": "canceled" })))
}

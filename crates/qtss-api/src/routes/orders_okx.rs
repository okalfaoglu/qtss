//! OKX USDT SWAP — keys from `exchange_accounts` (`exchange = okx`, `segment = futures`); passphrase required.

use axum::extract::{Extension, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::OrderIntent;
use qtss_execution::{
    okx_usdt_swap_inst_id, venue_order_id_from_okx_v5_response, ExecutionError, OkxLiveGateway,
};

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct PlaceOkxBody {
    pub intent: OrderIntent,
}

#[derive(Deserialize)]
pub struct CancelOkxBody {
    pub client_order_id: Uuid,
    /// Same as place intent symbol, e.g. `BTCUSDT` → `BTC-USDT-SWAP`
    pub symbol: String,
}

pub fn orders_okx_write_router() -> Router<SharedState> {
    Router::new()
        .route("/orders/okx/place", post(place_order))
        .route("/orders/okx/cancel", post(cancel_order))
}

fn okx_gateway_from_creds(
    creds: qtss_storage::ExchangeCredentials,
) -> Result<OkxLiveGateway, ApiError> {
    let passphrase = creds
        .passphrase
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| {
            ApiError::bad_request(
                "OKX requires passphrase — set exchange_accounts.passphrase for okx futures",
            )
        })?;
    Ok(OkxLiveGateway::mainnet(
        creds.api_key,
        creds.api_secret,
        passphrase,
    ))
}

async fn place_order(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<PlaceOkxBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let org_id = Uuid::parse_str(&claims.org_id)
        .map_err(|_| ApiError::bad_request("invalid token org_id"))?;
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("invalid token sub"))?;

    if body.intent.instrument.exchange != ExchangeId::Okx {
        return Err(ApiError::bad_request(
            "intent.instrument.exchange must be okx",
        ));
    }
    if body.intent.instrument.segment != MarketSegment::Futures {
        return Err(ApiError::bad_request(
            "okx API: only futures (USDT SWAP) segment is supported",
        ));
    }

    let seg = "futures";
    let creds = st
        .exchange_accounts
        .credentials_for_user(user_id, "okx", seg)
        .await?
        .ok_or_else(|| {
            ApiError::bad_request(
                "No OKX futures API key — add exchange_accounts (exchange=okx)",
            )
        })?;

    let gw = okx_gateway_from_creds(creds)?;
    let intent = body.intent;
    let symbol = intent.instrument.symbol.clone();
    let intent_record = intent.clone();
    let (id, venue_json) = gw
        .place_with_venue_response(intent)
        .await
        .map_err(|e: ExecutionError| ApiError::internal(e.to_string()))?;
    let venue_oid = venue_order_id_from_okx_v5_response(&venue_json);
    st.exchange_orders
        .insert_submitted(
            org_id,
            user_id,
            "okx",
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
    Json(body): Json<CancelOkxBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("invalid token sub"))?;
    let seg = "futures";
    let creds = st
        .exchange_accounts
        .credentials_for_user(user_id, "okx", seg)
        .await?
        .ok_or_else(|| ApiError::bad_request("No OKX futures API key"))?;

    let gw = okx_gateway_from_creds(creds)?;
    let inst_id = okx_usdt_swap_inst_id(&body.symbol);
    gw.cancel_swap_by_cl_ord_id(&inst_id, &body.client_order_id)
        .await
        .map_err(|e: ExecutionError| ApiError::internal(e.to_string()))?;

    let _ = st
        .exchange_orders
        .mark_canceled(user_id, body.client_order_id)
        .await?;
    Ok(Json(serde_json::json!({ "status": "canceled" })))
}

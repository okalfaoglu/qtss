//! Binance spot / USDT-M futures: açık emir listesi ↔ yerel `exchange_orders` (venue_order_id).

use axum::extract::{Extension, State};
use axum::routing::post;
use axum::{Json, Router};
use uuid::Uuid;

use qtss_binance::{BinanceClient, BinanceClientConfig};
use qtss_execution::{
    reconcile_binance_futures_open_orders, reconcile_binance_spot_open_orders,
    ExchangeOrderVenueSnapshot, ReconcileReport,
};

use crate::oauth::AccessClaims;
use crate::state::SharedState;

pub fn reconcile_router() -> Router<SharedState> {
    Router::new()
        .route("/reconcile/binance/futures", post(reconcile_binance_futures))
        .route("/reconcile/binance", post(reconcile_binance_spot))
}

async fn reconcile_binance_spot(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
) -> Result<Json<ReconcileReport>, String> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| "geçersiz token sub".to_string())?;
    let creds = st
        .exchange_accounts
        .binance_for_user(user_id, "spot")
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "Binance spot API anahtarı yok — exchange_accounts tablosuna ekleyin".to_string()
        })?;
    let cfg = BinanceClientConfig::mainnet_with_keys(creds.api_key, creds.api_secret);
    let client = BinanceClient::new(cfg).map_err(|e| e.to_string())?;
    let remote = client
        .spot_open_orders(None)
        .await
        .map_err(|e| e.to_string())?;

    let rows = st
        .exchange_orders
        .list_for_user(user_id, 500)
        .await
        .map_err(|e| e.to_string())?;

    let local: Vec<ExchangeOrderVenueSnapshot> = rows
        .into_iter()
        .filter(|r| r.exchange == "binance" && r.segment == "spot")
        .filter_map(|r| {
            r.venue_order_id.map(|id| ExchangeOrderVenueSnapshot {
                venue_order_id: id,
                status: r.status,
            })
        })
        .collect();

    let report = reconcile_binance_spot_open_orders(&remote, &local).map_err(|e| e.to_string())?;
    Ok(Json(report))
}

async fn reconcile_binance_futures(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
) -> Result<Json<ReconcileReport>, String> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| "geçersiz token sub".to_string())?;
    let creds = st
        .exchange_accounts
        .binance_for_user(user_id, "futures")
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "Binance futures API anahtarı yok — exchange_accounts tablosuna ekleyin".to_string()
        })?;
    let cfg = BinanceClientConfig::mainnet_with_keys(creds.api_key, creds.api_secret);
    let client = BinanceClient::new(cfg).map_err(|e| e.to_string())?;
    let remote = client
        .fapi_open_orders(None)
        .await
        .map_err(|e| e.to_string())?;

    let rows = st
        .exchange_orders
        .list_for_user(user_id, 500)
        .await
        .map_err(|e| e.to_string())?;

    let local: Vec<ExchangeOrderVenueSnapshot> = rows
        .into_iter()
        .filter(|r| r.exchange == "binance" && r.segment == "futures")
        .filter_map(|r| {
            r.venue_order_id.map(|id| ExchangeOrderVenueSnapshot {
                venue_order_id: id,
                status: r.status,
            })
        })
        .collect();

    let report =
        reconcile_binance_futures_open_orders(&remote, &local).map_err(|e| e.to_string())?;
    Ok(Json(report))
}

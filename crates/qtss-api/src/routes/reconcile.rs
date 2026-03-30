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
use qtss_reconcile::{
    apply_binance_futures_open_orders_patch, apply_binance_spot_open_orders_patch,
    BinanceOpenOrdersPatchConfig,
};

use crate::error::ApiError;
use crate::metrics::{record_reconcile_futures, record_reconcile_spot};
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
) -> Result<Json<ReconcileReport>, ApiError> {
    match reconcile_binance_spot_inner(claims, st).await {
        Ok(report) => {
            let rows = report.status_updates_applied.unwrap_or(0);
            record_reconcile_spot(true, rows);
            Ok(Json(report))
        }
        Err(e) => {
            record_reconcile_spot(false, 0);
            Err(e)
        }
    }
}

async fn reconcile_binance_spot_inner(
    claims: AccessClaims,
    st: SharedState,
) -> Result<ReconcileReport, ApiError> {
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("geçersiz token sub"))?;
    let creds = st
        .exchange_accounts
        .binance_for_user(user_id, "spot")
        .await?
        .ok_or_else(|| {
            ApiError::bad_request(
                "Binance spot API anahtarı yok — exchange_accounts tablosuna ekleyin",
            )
        })?;
    let cfg = BinanceClientConfig::mainnet_with_keys(creds.api_key, creds.api_secret);
    let client = BinanceClient::new(cfg).map_err(|e| ApiError::internal(e.to_string()))?;
    let remote = client
        .spot_open_orders(None)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let rows = st.exchange_orders.list_for_user(user_id, 500).await?;

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

    let mut report = reconcile_binance_spot_open_orders(&remote, &local)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let patch_cfg = BinanceOpenOrdersPatchConfig::http_spot();
    let n = apply_binance_spot_open_orders_patch(
        &st.exchange_orders,
        &client,
        user_id,
        &remote,
        &local,
        &patch_cfg,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    if n > 0 {
        report.status_updates_applied = Some(n);
    }
    Ok(report)
}

async fn reconcile_binance_futures(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
) -> Result<Json<ReconcileReport>, ApiError> {
    match reconcile_binance_futures_inner(claims, st).await {
        Ok(report) => {
            let rows = report.status_updates_applied.unwrap_or(0);
            record_reconcile_futures(true, rows);
            Ok(Json(report))
        }
        Err(e) => {
            record_reconcile_futures(false, 0);
            Err(e)
        }
    }
}

async fn reconcile_binance_futures_inner(
    claims: AccessClaims,
    st: SharedState,
) -> Result<ReconcileReport, ApiError> {
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("geçersiz token sub"))?;
    let creds = st
        .exchange_accounts
        .binance_for_user(user_id, "futures")
        .await?
        .ok_or_else(|| {
            ApiError::bad_request(
                "Binance futures API anahtarı yok — exchange_accounts tablosuna ekleyin",
            )
        })?;
    let cfg = BinanceClientConfig::mainnet_with_keys(creds.api_key, creds.api_secret);
    let client = BinanceClient::new(cfg).map_err(|e| ApiError::internal(e.to_string()))?;
    let remote = client
        .fapi_open_orders(None)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let rows = st.exchange_orders.list_for_user(user_id, 500).await?;

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

    let mut report = reconcile_binance_futures_open_orders(&remote, &local)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let patch_cfg = BinanceOpenOrdersPatchConfig::http_futures();
    let n = apply_binance_futures_open_orders_patch(
        &st.exchange_orders,
        &client,
        user_id,
        &remote,
        &local,
        &patch_cfg,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    if n > 0 {
        report.status_updates_applied = Some(n);
    }
    Ok(report)
}

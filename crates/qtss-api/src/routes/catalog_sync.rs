use axum::extract::{Extension, State};
use axum::routing::post;
use axum::{Json, Router};
use qtss_binance::{sync_full_binance_catalog, BinanceClient, BinanceClientConfig, CatalogSyncStats};
use qtss_storage::CatalogRepository;

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

pub fn catalog_sync_router() -> Router<SharedState> {
    Router::new().route("/catalog/sync/binance", post(sync_binance))
}

async fn sync_binance(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
) -> Result<Json<CatalogSyncStats>, ApiError> {
    let cfg = BinanceClientConfig::public_mainnet();
    let client = BinanceClient::new(cfg).map_err(|e| ApiError::internal(e.to_string()))?;
    let catalog = CatalogRepository::new(st.pool.clone());
    let stats = sync_full_binance_catalog(&client, &catalog)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(stats))
}

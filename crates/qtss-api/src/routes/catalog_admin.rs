//! Borsa / piyasa / enstrüman / mum aralığı kataloğu — CRUD (viewer okur, ops yazar).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use qtss_storage::{ui_segment_to_market_keys, CatalogRepository, ExchangeRow};

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;
use axum::Extension;

#[derive(Deserialize)]
pub struct MarketsListQuery {
    pub exchange_code: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    500
}

#[derive(Deserialize)]
pub struct InstrumentsListQuery {
    pub market_id: Uuid,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

#[derive(Deserialize)]
pub struct InstrumentSuggestQuery {
    pub exchange_code: String,
    /// Toolbar: `spot` | `futures` | `fapi` | `usdt_futures`
    pub segment: String,
    pub query: String,
    #[serde(default = "default_suggest_limit")]
    pub limit: i64,
}

fn default_suggest_limit() -> i64 {
    40
}

#[derive(Serialize)]
pub struct InstrumentSuggestionRow {
    pub native_symbol: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub status: String,
}

fn sanitize_instrument_prefix(raw: &str) -> Option<String> {
    let u: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(32)
        .collect();
    if u.is_empty() {
        None
    } else {
        Some(u.to_uppercase())
    }
}

#[derive(Deserialize)]
pub struct PostExchangeBody {
    pub code: String,
    pub display_name: String,
    #[serde(default = "default_true")]
    pub is_active: bool,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
pub struct PatchExchangeBody {
    pub display_name: Option<String>,
    pub is_active: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct PostMarketBody {
    pub exchange_code: String,
    pub segment: String,
    #[serde(default)]
    pub contract_kind: String,
    pub display_name: Option<String>,
    #[serde(default = "default_true")]
    pub is_active: bool,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Deserialize)]
pub struct PatchMarketBody {
    pub display_name: Option<String>,
    pub is_active: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct PostInstrumentBody {
    pub market_id: Uuid,
    pub native_symbol: String,
    pub base_asset: String,
    pub quote_asset: String,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default = "default_true")]
    pub is_trading: bool,
    pub price_filter: Option<serde_json::Value>,
    pub lot_filter: Option<serde_json::Value>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

fn default_status() -> String {
    "unknown".into()
}

#[derive(Deserialize)]
pub struct PatchInstrumentBody {
    pub base_asset: Option<String>,
    pub quote_asset: Option<String>,
    pub status: Option<String>,
    pub is_trading: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct PostBarIntervalBody {
    pub code: String,
    pub label: Option<String>,
    pub duration_seconds: Option<i32>,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default = "default_true")]
    pub is_active: bool,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Deserialize)]
pub struct PatchBarIntervalBody {
    pub label: Option<String>,
    pub duration_seconds: Option<i32>,
    pub sort_order: Option<i32>,
    pub is_active: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct MarketWithExchangeRow {
    #[serde(flatten)]
    pub market: qtss_storage::MarketRow,
    pub exchange_code: String,
}

async fn list_exchanges_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
) -> Result<Json<Vec<ExchangeRow>>, ApiError> {
    let cat = CatalogRepository::new(st.pool.clone());
    let rows = cat.list_exchanges().await?;
    Ok(Json(rows))
}

async fn list_markets_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<MarketsListQuery>,
) -> Result<Json<Vec<MarketWithExchangeRow>>, ApiError> {
    let cat = CatalogRepository::new(st.pool.clone());
    let lim = q.limit.clamp(1, 2000);
    let markets = match q
        .exchange_code
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Some(code) => cat.list_markets_by_exchange_code(code).await?,
        None => cat.list_markets_all(lim).await?,
    };
    let mut out = Vec::with_capacity(markets.len());
    for m in markets {
        let ex = cat.get_exchange_by_id(m.exchange_id).await?;
        let exchange_code = ex.map(|e| e.code).unwrap_or_default();
        out.push(MarketWithExchangeRow {
            market: m,
            exchange_code,
        });
    }
    Ok(Json(out))
}

async fn list_instruments_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<InstrumentsListQuery>,
) -> Result<Json<Vec<qtss_storage::InstrumentRow>>, (StatusCode, String)> {
    let cat = CatalogRepository::new(st.pool.clone());
    let lim = q.limit.clamp(1, 2000);
    cat.list_instruments_for_market(q.market_id, lim)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        .map(Json)
}

async fn instrument_suggestions_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<InstrumentSuggestQuery>,
) -> Result<Json<Vec<InstrumentSuggestionRow>>, ApiError> {
    let ex = q.exchange_code.trim();
    if ex.is_empty() {
        return Err(ApiError::bad_request("exchange_code gerekli"));
    }
    let Some(prefix) = sanitize_instrument_prefix(&q.query) else {
        return Ok(Json(vec![]));
    };
    let (m_seg, m_ck) = ui_segment_to_market_keys(&q.segment);
    let cat = CatalogRepository::new(st.pool.clone());
    let lim = q.limit.clamp(1, 200);
    let rows = cat
        .search_tradable_instruments_prefix(ex, m_seg, m_ck, &prefix, lim)
        .await?;
    let out: Vec<InstrumentSuggestionRow> = rows
        .into_iter()
        .map(|r| InstrumentSuggestionRow {
            native_symbol: r.native_symbol,
            base_asset: r.base_asset,
            quote_asset: r.quote_asset,
            status: r.status,
        })
        .collect();
    Ok(Json(out))
}

async fn list_bar_intervals_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
) -> Result<Json<Vec<qtss_storage::BarIntervalRow>>, ApiError> {
    let cat = CatalogRepository::new(st.pool.clone());
    let rows = cat.list_bar_intervals().await?;
    Ok(Json(rows))
}

async fn post_exchange_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<PostExchangeBody>,
) -> Result<Json<ExchangeRow>, (StatusCode, String)> {
    let code = body.code.trim();
    if code.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "code gerekli".into()));
    }
    let cat = CatalogRepository::new(st.pool.clone());
    cat.upsert_exchange(
        code,
        body.display_name.trim(),
        body.is_active,
        body.metadata,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    .map(Json)
}

async fn patch_exchange_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchExchangeBody>,
) -> Result<Json<ExchangeRow>, ApiError> {
    let cat = CatalogRepository::new(st.pool.clone());
    let row = cat
        .update_exchange(
            id,
            body.display_name.as_deref(),
            body.is_active,
            body.metadata,
        )
        .await?
        .ok_or_else(|| ApiError::not_found("exchange bulunamadı"))?;
    Ok(Json(row))
}

async fn delete_exchange_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let cat = CatalogRepository::new(st.pool.clone());
    let n = cat.delete_exchange(id).await?;
    if n == 0 {
        return Err(ApiError::not_found("exchange bulunamadı"));
    }
    Ok(Json(json!({ "deleted": n })))
}

async fn post_market_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<PostMarketBody>,
) -> Result<Json<qtss_storage::MarketRow>, (StatusCode, String)> {
    let cat = CatalogRepository::new(st.pool.clone());
    cat.upsert_market(
        body.exchange_code.trim(),
        body.segment.trim(),
        body.contract_kind.trim(),
        body.display_name.as_deref(),
        body.is_active,
        body.metadata,
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
    .map(Json)
}

async fn patch_market_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchMarketBody>,
) -> Result<Json<qtss_storage::MarketRow>, ApiError> {
    let cat = CatalogRepository::new(st.pool.clone());
    let row = cat
        .update_market(
            id,
            body.display_name.as_deref(),
            body.is_active,
            body.metadata,
        )
        .await?
        .ok_or_else(|| ApiError::not_found("market bulunamadı"))?;
    Ok(Json(row))
}

async fn delete_market_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let cat = CatalogRepository::new(st.pool.clone());
    let n = cat.delete_market(id).await?;
    if n == 0 {
        return Err(ApiError::not_found("market bulunamadı"));
    }
    Ok(Json(json!({ "deleted": n })))
}

async fn post_instrument_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<PostInstrumentBody>,
) -> Result<Json<qtss_storage::InstrumentRow>, ApiError> {
    let cat = CatalogRepository::new(st.pool.clone());
    let row = cat
        .upsert_instrument(
            body.market_id,
            body.native_symbol.trim(),
            body.base_asset.trim(),
            body.quote_asset.trim(),
            body.status.trim(),
            body.is_trading,
            body.price_filter,
            body.lot_filter,
            body.metadata,
        )
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(row))
}

async fn patch_instrument_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchInstrumentBody>,
) -> Result<Json<qtss_storage::InstrumentRow>, ApiError> {
    let cat = CatalogRepository::new(st.pool.clone());
    let row = cat
        .update_instrument(
            id,
            body.base_asset.as_deref(),
            body.quote_asset.as_deref(),
            body.status.as_deref(),
            body.is_trading,
            body.metadata,
        )
        .await?
        .ok_or_else(|| ApiError::not_found("instrument bulunamadı"))?;
    Ok(Json(row))
}

async fn delete_instrument_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let cat = CatalogRepository::new(st.pool.clone());
    let n = cat.delete_instrument(id).await?;
    if n == 0 {
        return Err(ApiError::not_found("instrument bulunamadı"));
    }
    Ok(Json(json!({ "deleted": n })))
}

async fn post_bar_interval_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<PostBarIntervalBody>,
) -> Result<Json<qtss_storage::BarIntervalRow>, ApiError> {
    let code = body.code.trim();
    if code.is_empty() {
        return Err(ApiError::bad_request("code gerekli"));
    }
    let cat = CatalogRepository::new(st.pool.clone());
    let row = cat
        .upsert_bar_interval(
            code,
            body.label.as_deref(),
            body.duration_seconds,
            body.sort_order,
            body.is_active,
            body.metadata,
        )
        .await?;
    Ok(Json(row))
}

async fn patch_bar_interval_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchBarIntervalBody>,
) -> Result<Json<qtss_storage::BarIntervalRow>, ApiError> {
    let cat = CatalogRepository::new(st.pool.clone());
    let cur = cat
        .list_bar_intervals()
        .await?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| ApiError::not_found("interval bulunamadı"))?;
    let row = cat
        .upsert_bar_interval(
            &cur.code,
            body.label.as_deref().or(cur.label.as_deref()),
            body.duration_seconds.or(cur.duration_seconds),
            body.sort_order.unwrap_or(cur.sort_order),
            body.is_active.unwrap_or(cur.is_active),
            body.metadata.unwrap_or(cur.metadata),
        )
        .await?;
    Ok(Json(row))
}

async fn delete_bar_interval_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let cat = CatalogRepository::new(st.pool.clone());
    let n = cat.delete_bar_interval(id).await?;
    if n == 0 {
        return Err(ApiError::not_found("interval bulunamadı"));
    }
    Ok(Json(json!({ "deleted": n })))
}

pub fn catalog_read_router() -> Router<SharedState> {
    Router::new()
        .route("/catalog/exchanges", get(list_exchanges_api))
        .route("/catalog/markets", get(list_markets_api))
        .route("/catalog/instruments", get(list_instruments_api))
        .route(
            "/catalog/instrument-suggestions",
            get(instrument_suggestions_api),
        )
        .route("/catalog/bar-intervals", get(list_bar_intervals_api))
}

pub fn catalog_write_router() -> Router<SharedState> {
    Router::new()
        .route("/catalog/exchanges", post(post_exchange_api))
        .route(
            "/catalog/exchanges/{id}",
            patch(patch_exchange_api).delete(delete_exchange_api),
        )
        .route("/catalog/markets", post(post_market_api))
        .route(
            "/catalog/markets/{id}",
            patch(patch_market_api).delete(delete_market_api),
        )
        .route("/catalog/instruments", post(post_instrument_api))
        .route(
            "/catalog/instruments/{id}",
            patch(patch_instrument_api).delete(delete_instrument_api),
        )
        .route("/catalog/bar-intervals", post(post_bar_interval_api))
        .route(
            "/catalog/bar-intervals/{id}",
            patch(patch_bar_interval_api).delete(delete_bar_interval_api),
        )
}

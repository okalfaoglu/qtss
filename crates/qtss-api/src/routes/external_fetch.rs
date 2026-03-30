//! DB tanımlı HTTP kaynakları — son yanıt `data_snapshots` (yalnızca `external_data_sources` anahtarları).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use qtss_storage::{
    delete_external_source, fetch_data_snapshot_for_external_http_source, list_external_sources,
    list_snapshots_for_external_http_sources, upsert_external_source, DataSnapshotRow,
    ExternalDataSourceRow,
};

use crate::error::ApiError;
use crate::state::SharedState;

fn valid_source_key(key: &str) -> bool {
    let mut it = key.chars();
    let Some(first) = it.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() || key.len() > 64 {
        return false;
    }
    key.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn status_code_from_meta(meta: &Option<Value>) -> Option<i16> {
    meta.as_ref()
        .and_then(|m| m.get("http_status"))
        .and_then(|x| x.as_i64())
        .map(|x| x as i16)
}

pub fn external_fetch_read_router() -> Router<SharedState> {
    Router::new()
        .route("/analysis/external-fetch/sources", get(list_sources_api))
        .route(
            "/analysis/external-fetch/snapshots",
            get(list_snapshots_api),
        )
        .route(
            "/analysis/external-fetch/snapshots/{key}",
            get(get_snapshot_api),
        )
}

/// `trader` / `admin` — kaynak tanımı yazımı.
pub fn external_fetch_write_router() -> Router<SharedState> {
    Router::new()
        .route("/analysis/external-fetch/sources", post(upsert_source_api))
        .route(
            "/analysis/external-fetch/sources/{key}",
            delete(delete_source_api),
        )
}

async fn list_sources_api(
    State(st): State<SharedState>,
) -> Result<Json<Vec<ExternalDataSourceRow>>, ApiError> {
    let rows = list_external_sources(&st.pool).await?;
    Ok(Json(rows))
}

#[derive(Serialize)]
struct SnapshotListItem {
    pub source_key: String,
    pub computed_at: DateTime<Utc>,
    pub status_code: Option<i16>,
    pub error: Option<String>,
    pub has_response: bool,
}

async fn list_snapshots_api(
    State(st): State<SharedState>,
) -> Result<Json<Vec<SnapshotListItem>>, ApiError> {
    let rows = list_snapshots_for_external_http_sources(&st.pool).await?;
    let out: Vec<SnapshotListItem> = rows
        .into_iter()
        .map(|r| SnapshotListItem {
            source_key: r.source_key,
            computed_at: r.computed_at,
            status_code: status_code_from_meta(&r.meta_json),
            error: r.error,
            has_response: r.response_json.is_some(),
        })
        .collect();
    Ok(Json(out))
}

/// Eski `external_data_snapshots` alanları + `meta_json` (HTTP meta birleşik tabloda).
#[derive(Serialize)]
struct ExternalHttpSnapshotResponse {
    pub source_key: String,
    pub request_json: Value,
    pub response_json: Option<Value>,
    pub status_code: Option<i16>,
    pub meta_json: Option<Value>,
    pub computed_at: DateTime<Utc>,
    pub error: Option<String>,
}

impl From<DataSnapshotRow> for ExternalHttpSnapshotResponse {
    fn from(r: DataSnapshotRow) -> Self {
        let status_code = status_code_from_meta(&r.meta_json);
        ExternalHttpSnapshotResponse {
            source_key: r.source_key,
            request_json: r.request_json,
            response_json: r.response_json,
            status_code,
            meta_json: r.meta_json,
            computed_at: r.computed_at,
            error: r.error,
        }
    }
}

async fn get_snapshot_api(
    State(st): State<SharedState>,
    Path(key): Path<String>,
) -> Result<Json<ExternalHttpSnapshotResponse>, ApiError> {
    let key = key.trim();
    if !valid_source_key(key) {
        return Err(ApiError::bad_request("geçersiz source key"));
    }
    let row = fetch_data_snapshot_for_external_http_source(&st.pool, key).await?;
    let Some(row) = row else {
        return Err(ApiError::not_found(
            "snapshot yok — kaynak tanımlı mı ve worker çekti mi kontrol edin",
        ));
    };
    Ok(Json(ExternalHttpSnapshotResponse::from(row)))
}

#[derive(Deserialize)]
struct UpsertExternalSourceBody {
    pub key: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// `GET` veya `POST` (büyük/küçük harf duyarsız).
    #[serde(default)]
    pub method: Option<String>,
    pub url: String,
    #[serde(default)]
    pub headers_json: Option<Value>,
    #[serde(default)]
    pub body_json: Option<Value>,
    #[serde(default)]
    pub tick_secs: Option<i32>,
    pub description: Option<String>,
}

fn default_true() -> bool {
    true
}

fn normalize_http_method(m: &str) -> Result<&'static str, ApiError> {
    match m.trim().to_ascii_uppercase().as_str() {
        "GET" => Ok("GET"),
        "POST" => Ok("POST"),
        _ => Err(ApiError::bad_request(
            "method yalnızca GET veya POST olabilir",
        )),
    }
}

async fn upsert_source_api(
    State(st): State<SharedState>,
    Json(body): Json<UpsertExternalSourceBody>,
) -> Result<Json<ExternalDataSourceRow>, ApiError> {
    let key = body.key.trim();
    if !valid_source_key(key) {
        return Err(ApiError::bad_request(
            "geçersiz key (1–64 karakter, [a-zA-Z0-9_-], rakam/harf ile başlar)",
        ));
    }
    let url = body.url.trim();
    if url.is_empty() {
        return Err(ApiError::bad_request("url boş olamaz"));
    }
    let method_raw = body.method.as_deref().unwrap_or("GET");
    let method = normalize_http_method(method_raw)?;
    let tick = body.tick_secs.unwrap_or(300).max(30);
    let headers = body.headers_json.unwrap_or_else(|| json!({}));
    if !headers.is_object() {
        return Err(ApiError::bad_request(
            "headers_json bir JSON nesnesi olmalı",
        ));
    }
    let desc = body
        .description
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let row = upsert_external_source(
        &st.pool,
        key,
        body.enabled,
        method,
        url,
        &headers,
        body.body_json.as_ref(),
        tick,
        desc,
    )
    .await?;
    Ok(Json(row))
}

async fn delete_source_api(
    State(st): State<SharedState>,
    Path(key): Path<String>,
) -> Result<StatusCode, ApiError> {
    let key = key.trim();
    if !valid_source_key(key) {
        return Err(ApiError::bad_request("geçersiz source key"));
    }
    let n = delete_external_source(&st.pool, key).await?;
    if n == 0 {
        return Err(ApiError::not_found("kayıt yok"));
    }
    Ok(StatusCode::NO_CONTENT)
}

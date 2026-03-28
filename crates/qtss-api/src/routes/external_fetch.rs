//! DB tanımlı HTTP kaynakları — son snapshot okuma (`external_data_snapshots`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use qtss_storage::{
    delete_external_source, fetch_external_snapshot, list_external_snapshots, list_external_sources,
    upsert_external_source, ExternalDataSnapshotRow, ExternalDataSourceRow,
};

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

pub fn external_fetch_read_router() -> Router<SharedState> {
    Router::new()
        .route("/analysis/external-fetch/sources", get(list_sources_api))
        .route("/analysis/external-fetch/snapshots", get(list_snapshots_api))
        .route(
            "/analysis/external-fetch/snapshots/{key}",
            get(get_snapshot_api),
        )
}

async fn list_sources_api(
    State(st): State<SharedState>,
) -> Result<Json<Vec<ExternalDataSourceRow>>, String> {
    list_external_sources(&st.pool)
        .await
        .map(Json)
        .map_err(|e| e.to_string())
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
) -> Result<Json<Vec<SnapshotListItem>>, String> {
    let rows = list_external_snapshots(&st.pool)
        .await
        .map_err(|e| e.to_string())?;
    let out: Vec<SnapshotListItem> = rows
        .into_iter()
        .map(|r| SnapshotListItem {
            source_key: r.source_key,
            computed_at: r.computed_at,
            status_code: r.status_code,
            error: r.error,
            has_response: r.response_json.is_some(),
        })
        .collect();
    Ok(Json(out))
}

async fn get_snapshot_api(
    State(st): State<SharedState>,
    Path(key): Path<String>,
) -> Result<Json<ExternalDataSnapshotRow>, (StatusCode, String)> {
    let key = key.trim();
    if !valid_source_key(key) {
        return Err((
            StatusCode::BAD_REQUEST,
            "geçersiz source key".to_string(),
        ));
    }
    let row = fetch_external_snapshot(&st.pool, key)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let Some(row) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            "snapshot yok — kaynak tanımlı mı ve worker çekti mi kontrol edin".to_string(),
        ));
    };
    Ok(Json(row))
}

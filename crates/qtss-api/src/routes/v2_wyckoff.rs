//! Wyckoff structure API endpoints (Faz 10).
//!
//! GET /v2/wyckoff/active            — all active structures
//! GET /v2/wyckoff/structure/:id     — single structure detail
//! GET /v2/wyckoff/history/:symbol   — symbol history
//! GET /v2/wyckoff/overlay/:symbol/:interval — chart overlay data

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::SharedState;
use qtss_storage::{
    find_active_wyckoff_structure, get_wyckoff_structure, list_active_wyckoff_structures,
    list_wyckoff_history,
};

#[derive(Debug, Deserialize)]
pub struct ActiveQuery {
    pub symbol: Option<String>,
    pub interval: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<i64>,
}

pub fn v2_wyckoff_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/wyckoff/active", get(get_active))
        .route("/v2/wyckoff/structure/{id}", get(get_structure))
        .route("/v2/wyckoff/history/{symbol}", get(get_history))
        .route(
            "/v2/wyckoff/overlay/{symbol}/{interval}",
            get(get_overlay),
        )
}

async fn get_active(
    State(st): State<SharedState>,
    Query(q): Query<ActiveQuery>,
) -> Result<Json<JsonValue>, ApiError> {
    let rows = list_active_wyckoff_structures(
        &st.pool,
        q.symbol.as_deref(),
        q.interval.as_deref(),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "structures": rows })))
}

async fn get_structure(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<JsonValue>, ApiError> {
    let row = get_wyckoff_structure(&st.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    match row {
        Some(r) => Ok(Json(json!(r))),
        None => Err(ApiError::not_found("structure not found")),
    }
}

async fn get_history(
    State(st): State<SharedState>,
    Path(symbol): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<JsonValue>, ApiError> {
    let limit = q.limit.unwrap_or(20).clamp(1, 200);
    let rows = list_wyckoff_history(&st.pool, &symbol, limit)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "history": rows })))
}

/// Chart overlay: returns range box, creek/ice, events with timestamps.
async fn get_overlay(
    State(st): State<SharedState>,
    Path((symbol, interval)): Path<(String, String)>,
) -> Result<Json<JsonValue>, ApiError> {
    let structure = find_active_wyckoff_structure(&st.pool, &symbol, &interval)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let s = match structure {
        Some(s) => s,
        None => return Ok(Json(json!({ "overlay": null }))),
    };
    Ok(Json(json!({
        "overlay": {
            "id": s.id,
            "schematic": s.schematic,
            "phase": s.current_phase,
            "confidence": s.confidence,
            "range": {
                "top": s.range_top,
                "bottom": s.range_bottom,
            },
            "creek": s.creek_level,
            "ice": s.ice_level,
            "slope_deg": s.slope_deg,
            "events": s.events_json,
            "started_at": s.started_at,
        }
    })))
}

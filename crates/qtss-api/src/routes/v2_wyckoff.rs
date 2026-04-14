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
    list_phase_groups_by_timeframe, list_recent_wyckoff_structures, list_wyckoff_history,
    v2_setups::{list_v2_setups_filtered, SetupFilter},
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

#[derive(Debug, Deserialize)]
pub struct RecentQuery {
    /// `active` | `completed` | `failed` | `all` (default `all`).
    pub status: Option<String>,
    pub exchange: Option<String>,
    pub symbol: Option<String>,
    pub interval: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct PhaseGroupsQuery {
    pub exchange: Option<String>,
    pub symbol: Option<String>,
    pub interval: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WyckoffSetupsQuery {
    pub limit: Option<i64>,
    pub mode: Option<String>,
    pub state: Option<String>,
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    pub profile: Option<String>,
    pub venue: Option<String>,
}

pub fn v2_wyckoff_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/wyckoff/active", get(get_active))
        .route("/v2/wyckoff/recent", get(get_recent))
        .route("/v2/wyckoff/structure/{id}", get(get_structure))
        .route("/v2/wyckoff/history/{symbol}", get(get_history))
        .route(
            "/v2/wyckoff/overlay/{symbol}/{interval}",
            get(get_overlay),
        )
        .route("/v2/wyckoff/setups", get(get_wyckoff_setups))
        .route("/v2/wyckoff/phase-groups", get(get_phase_groups))
}

/// Wyckoff-scoped setup feed. Thin wrapper around
/// `list_v2_setups_filtered` with `alt_type LIKE 'wyckoff_%'` pre-applied.
/// Default `state` filter: armed+active. Pass `state=closed` for history.
async fn get_wyckoff_setups(
    State(st): State<SharedState>,
    Query(q): Query<WyckoffSetupsQuery>,
) -> Result<Json<JsonValue>, ApiError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 2_000);
    let states = q
        .state
        .as_deref()
        .map(|s| vec![s.to_string()])
        .unwrap_or_default();
    let filter = SetupFilter {
        limit,
        venue_class: q.venue,
        states,
        profile: q.profile,
        alt_type_like: Some("wyckoff_%".to_string()),
        symbol: q.symbol,
        timeframe: q.timeframe,
        mode: q.mode,
    };
    let rows = list_v2_setups_filtered(&st.pool, &filter).await?;
    Ok(Json(json!({
        "generated_at": chrono::Utc::now(),
        "count": rows.len(),
        "entries": rows,
    })))
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

/// Cross-symbol recent feed. `status=active|completed|failed|all`
/// (default `all`). Ordered by lifecycle timestamp DESC so the most
/// recently closed structures surface first.
async fn get_recent(
    State(st): State<SharedState>,
    Query(q): Query<RecentQuery>,
) -> Result<Json<JsonValue>, ApiError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let rows = list_recent_wyckoff_structures(
        &st.pool,
        q.status.as_deref(),
        q.exchange.as_deref(),
        q.symbol.as_deref(),
        q.interval.as_deref(),
        limit,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "structures": rows })))
}

/// Phase counts grouped per (exchange, symbol, interval). Backs the
/// "phases grouped by timeframe" summary in the GUI — aggregates the
/// entire history since the first stored structure.
async fn get_phase_groups(
    State(st): State<SharedState>,
    Query(q): Query<PhaseGroupsQuery>,
) -> Result<Json<JsonValue>, ApiError> {
    let rows = list_phase_groups_by_timeframe(
        &st.pool,
        q.exchange.as_deref(),
        q.symbol.as_deref(),
        q.interval.as_deref(),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "groups": rows })))
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

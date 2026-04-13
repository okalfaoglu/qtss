//! Wave projections API — list/manage projected formations.

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use uuid::Uuid;

use qtss_storage::wave_projections;

use crate::error::ApiError;
use crate::state::SharedState;

// ─── Wire types ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ProjectionWire {
    pub id: String,
    pub source_wave_id: String,
    pub alt_group: String,
    pub projected_kind: String,
    pub projected_label: String,
    pub direction: String,
    pub degree: String,
    pub fib_basis: Option<String>,
    pub projected_legs: serde_json::Value,
    pub probability: f32,
    pub rank: i32,
    pub state: String,
    pub elimination_reason: Option<String>,
    pub bars_validated: i32,
    pub invalidation_price: Option<String>,
    pub time_start_est: Option<String>,
    pub time_end_est: Option<String>,
    pub price_target_min: Option<String>,
    pub price_target_max: Option<String>,
}

impl From<wave_projections::WaveProjectionRow> for ProjectionWire {
    fn from(r: wave_projections::WaveProjectionRow) -> Self {
        Self {
            id: r.id.to_string(),
            source_wave_id: r.source_wave_id.to_string(),
            alt_group: r.alt_group.to_string(),
            projected_kind: r.projected_kind,
            projected_label: r.projected_label,
            direction: r.direction,
            degree: r.degree,
            fib_basis: r.fib_basis,
            projected_legs: r.projected_legs,
            probability: r.probability,
            rank: r.rank,
            state: r.state,
            elimination_reason: r.elimination_reason,
            bars_validated: r.bars_validated,
            invalidation_price: r.invalidation_price.map(|p| p.to_string()),
            time_start_est: r.time_start_est.map(|t| t.to_rfc3339()),
            time_end_est: r.time_end_est.map(|t| t.to_rfc3339()),
            price_target_min: r.price_target_min.map(|p| p.to_string()),
            price_target_max: r.price_target_max.map(|p| p.to_string()),
        }
    }
}

// ─── Router ─────────────────────────────────────────────────────────

pub fn v2_wave_projections_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/wave-projections/source/{wave_id}", get(get_by_source))
        .route("/v2/wave-projections/alt-group/{alt_group}", get(get_by_alt_group))
        .route("/v2/wave-projections/{venue}/{symbol}/{tf}", get(get_active))
}

/// Projections originating from a specific wave.
async fn get_by_source(
    State(st): State<SharedState>,
    Path(wave_id): Path<String>,
) -> Result<Json<Vec<ProjectionWire>>, ApiError> {
    let id: Uuid = wave_id.parse().map_err(|_| ApiError::bad_request("invalid uuid"))?;
    let rows = wave_projections::list_by_source(&st.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(rows.into_iter().map(ProjectionWire::from).collect()))
}

/// All alternatives in a group.
async fn get_by_alt_group(
    State(st): State<SharedState>,
    Path(alt_group): Path<String>,
) -> Result<Json<Vec<ProjectionWire>>, ApiError> {
    let id: Uuid = alt_group.parse().map_err(|_| ApiError::bad_request("invalid uuid"))?;
    let rows = wave_projections::list_by_alt_group(&st.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(rows.into_iter().map(ProjectionWire::from).collect()))
}

/// Active projections for a symbol+timeframe.
async fn get_active(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
) -> Result<Json<Vec<ProjectionWire>>, ApiError> {
    let rows = wave_projections::list_active_projections(&st.pool, &venue, &symbol, &tf)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(rows.into_iter().map(ProjectionWire::from).collect()))
}

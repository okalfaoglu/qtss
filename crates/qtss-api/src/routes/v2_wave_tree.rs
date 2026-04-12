//! Elliott Deep wave hierarchy — lazy TF-by-TF drill-down.
//!
//! `GET /v2/wave-tree/{venue}/{symbol}/tf/{tf}` — formations + wave segments
//! at a single timeframe. Optional `?time_start=&time_end=` to scope within
//! a parent wave's range. Frontend calls this lazily as user clicks deeper.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use qtss_storage::wave_chain;

use crate::error::ApiError;
use crate::state::SharedState;

// ─── Wire types ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct WaveSegmentWire {
    pub id: String,
    pub wave_number: Option<String>,
    pub direction: String,
    pub price_start: String,
    pub price_end: String,
    pub time_start: Option<DateTime<Utc>>,
    pub time_end: Option<DateTime<Utc>>,
    pub structural_score: f32,
    pub state: String,
    /// How many children this wave has at the next lower TF.
    pub child_count: usize,
}

#[derive(Debug, Serialize)]
pub struct FormationWire {
    pub id: String,
    pub kind: String,
    pub direction: String,
    pub degree: String,
    pub state: String,
    pub price_start: String,
    pub price_end: String,
    pub time_start: Option<DateTime<Utc>>,
    pub time_end: Option<DateTime<Utc>>,
    pub avg_score: f32,
    pub waves: Vec<WaveSegmentWire>,
}

#[derive(Debug, Serialize)]
pub struct TfLevelResponse {
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub formations: Vec<FormationWire>,
}

#[derive(Debug, Default, Deserialize)]
pub struct TfQuery {
    #[serde(default)]
    pub time_start: Option<DateTime<Utc>>,
    #[serde(default)]
    pub time_end: Option<DateTime<Utc>>,
    /// If true, only return formations with at least one active wave.
    #[serde(default)]
    pub active_only: Option<bool>,
}

// ─── Router ──────────────────────────────────────────────────────────

pub fn v2_wave_tree_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/wave-tree/{venue}/{symbol}/tf/{tf}", get(get_tf_level))
        .route("/v2/wave-tree/{venue}/{symbol}/{wave_id}/children", get(get_wave_children))
}

/// Get formations at a specific TF, optionally scoped to a time range.
async fn get_tf_level(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<TfQuery>,
) -> Result<Json<TfLevelResponse>, ApiError> {
    let rows = wave_chain::list_waves_at_tf(
        &st.pool, &venue, &symbol, &tf,
        q.time_start, q.time_end, 200,
    ).await.map_err(|e| ApiError::internal(e.to_string()))?;

    // Group wave segments into formations by shared parent_id
    let mut groups: std::collections::BTreeMap<String, Vec<wave_chain::WaveChainRow>> =
        std::collections::BTreeMap::new();
    for row in rows {
        let key = row.parent_id.map(|p| p.to_string()).unwrap_or_else(|| format!("solo_{}", row.id));
        groups.entry(key).or_default().push(row);
    }

    let mut formations = Vec::new();
    for (_key, mut waves) in groups {
        waves.sort_by_key(|w| w.time_start);
        let first = &waves[0];
        let last = &waves[waves.len() - 1];
        let any_active = waves.iter().any(|w| w.state == "active");

        // Count children for each wave segment
        let mut wire_waves = Vec::new();
        for w in &waves {
            let child_count = wave_chain::count_children(&st.pool, w.id)
                .await
                .unwrap_or(0) as usize;
            wire_waves.push(WaveSegmentWire {
                id: w.id.to_string(),
                wave_number: w.wave_number.clone(),
                direction: w.direction.clone(),
                price_start: w.price_start.to_string(),
                price_end: w.price_end.to_string(),
                time_start: w.time_start,
                time_end: w.time_end,
                structural_score: w.structural_score,
                state: w.state.clone(),
                child_count,
            });
        }

        let avg_score = waves.iter().map(|w| w.structural_score).sum::<f32>() / waves.len() as f32;
        formations.push(FormationWire {
            id: first.id.to_string(),
            kind: first.kind.clone(),
            direction: first.direction.clone(),
            degree: first.degree.clone(),
            state: if any_active { "active".into() } else { "completed".into() },
            price_start: first.price_start.to_string(),
            price_end: last.price_end.to_string(),
            time_start: first.time_start,
            time_end: last.time_end,
            avg_score,
            waves: wire_waves,
        });
    }

    // Filter by active_only if requested
    let active_only = q.active_only.unwrap_or(false);
    if active_only {
        formations.retain(|f| f.state == "active");
    }

    Ok(Json(TfLevelResponse {
        exchange: venue,
        symbol,
        timeframe: tf,
        formations,
    }))
}

/// Get direct children of a wave segment (for inline expansion).
async fn get_wave_children(
    State(st): State<SharedState>,
    Path((_venue, _symbol, wave_id)): Path<(String, String, String)>,
) -> Result<Json<Vec<WaveSegmentWire>>, ApiError> {
    let id: Uuid = wave_id.parse().map_err(|_| ApiError::bad_request("invalid uuid"))?;
    let children = wave_chain::list_children(&st.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut result = Vec::new();
    for c in &children {
        let gc_count = wave_chain::count_children(&st.pool, c.id)
            .await
            .unwrap_or(0) as usize;
        result.push(WaveSegmentWire {
            id: c.id.to_string(),
            wave_number: c.wave_number.clone(),
            direction: c.direction.clone(),
            price_start: c.price_start.to_string(),
            price_end: c.price_end.to_string(),
            time_start: c.time_start,
            time_end: c.time_end,
            structural_score: c.structural_score,
            state: c.state.clone(),
            child_count: gc_count,
        });
    }

    Ok(Json(result))
}

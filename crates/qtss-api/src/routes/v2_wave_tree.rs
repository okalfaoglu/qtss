//! `GET /v2/wave-tree/{venue}/{symbol}` — Elliott Deep wave hierarchy.
//!
//! Returns all active wave_chain rows for a symbol, plus ancestor chains
//! for each root wave. The frontend builds an interactive tree view.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use qtss_storage::wave_chain;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Serialize)]
pub struct WaveNodeWire {
    pub id: String,
    pub parent_id: Option<String>,
    pub timeframe: String,
    pub degree: String,
    pub kind: String,
    pub direction: String,
    pub wave_number: Option<String>,
    pub price_start: String,
    pub price_end: String,
    pub time_start: Option<DateTime<Utc>>,
    pub time_end: Option<DateTime<Utc>>,
    pub structural_score: f32,
    pub state: String,
    pub children: Vec<WaveNodeWire>,
}

#[derive(Debug, Serialize)]
pub struct WaveTreeResponse {
    pub exchange: String,
    pub symbol: String,
    pub roots: Vec<WaveNodeWire>,
    pub total_waves: usize,
}

#[derive(Debug, Deserialize)]
pub struct WaveTreeQuery {
    pub limit: Option<i64>,
}

pub fn v2_wave_tree_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/wave-tree/{venue}/{symbol}", get(get_wave_tree))
        .route("/v2/wave-tree/{venue}/{symbol}/{wave_id}/ancestors", get(get_ancestors))
}

async fn get_wave_tree(
    State(st): State<SharedState>,
    Path((venue, symbol)): Path<(String, String)>,
    Query(q): Query<WaveTreeQuery>,
) -> Result<Json<WaveTreeResponse>, ApiError> {
    let limit = q.limit.unwrap_or(200).min(1000);
    let rows = wave_chain::list_waves_for_symbol(&st.pool, &venue, &symbol, limit)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let total_waves = rows.len();

    // Build tree: collect all rows, then nest children under parents
    let mut nodes: std::collections::HashMap<Uuid, WaveNodeWire> = std::collections::HashMap::new();
    let mut order: Vec<Uuid> = Vec::new();

    for row in &rows {
        order.push(row.id);
        nodes.insert(row.id, WaveNodeWire {
            id: row.id.to_string(),
            parent_id: row.parent_id.map(|p| p.to_string()),
            timeframe: row.timeframe.clone(),
            degree: row.degree.clone(),
            kind: row.kind.clone(),
            direction: row.direction.clone(),
            wave_number: row.wave_number.clone(),
            price_start: row.price_start.to_string(),
            price_end: row.price_end.to_string(),
            time_start: row.time_start,
            time_end: row.time_end,
            structural_score: row.structural_score,
            state: row.state.clone(),
            children: Vec::new(),
        });
    }

    // Collect parent→child edges, then nest
    let mut child_map: std::collections::HashMap<Uuid, Vec<Uuid>> = std::collections::HashMap::new();
    let mut root_ids: Vec<Uuid> = Vec::new();
    for row in &rows {
        match row.parent_id {
            Some(pid) if nodes.contains_key(&pid) => {
                child_map.entry(pid).or_default().push(row.id);
            }
            _ => root_ids.push(row.id),
        }
    }

    // Recursively build tree (max depth bounded by degree hierarchy ~9)
    fn build_tree(
        id: Uuid,
        nodes: &mut std::collections::HashMap<Uuid, WaveNodeWire>,
        child_map: &std::collections::HashMap<Uuid, Vec<Uuid>>,
    ) -> Option<WaveNodeWire> {
        let children_ids = child_map.get(&id).cloned().unwrap_or_default();
        let children: Vec<WaveNodeWire> = children_ids
            .into_iter()
            .filter_map(|cid| build_tree(cid, nodes, child_map))
            .collect();
        let mut node = nodes.remove(&id)?;
        node.children = children;
        Some(node)
    }

    let roots: Vec<WaveNodeWire> = root_ids
        .into_iter()
        .filter_map(|rid| build_tree(rid, &mut nodes, &child_map))
        .collect();

    Ok(Json(WaveTreeResponse {
        exchange: venue,
        symbol,
        roots,
        total_waves,
    }))
}

#[derive(Debug, Serialize)]
pub struct AncestorChainResponse {
    pub chain: Vec<WaveNodeWire>,
}

async fn get_ancestors(
    State(st): State<SharedState>,
    Path((_venue, _symbol, wave_id)): Path<(String, String, String)>,
) -> Result<Json<AncestorChainResponse>, ApiError> {
    let id: Uuid = wave_id.parse().map_err(|_| ApiError::bad_request("invalid uuid"))?;
    let rows = wave_chain::get_ancestor_chain(&st.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let chain = rows
        .into_iter()
        .map(|row| WaveNodeWire {
            id: row.id.to_string(),
            parent_id: row.parent_id.map(|p| p.to_string()),
            timeframe: row.timeframe,
            degree: row.degree,
            kind: row.kind,
            direction: row.direction,
            wave_number: row.wave_number,
            price_start: row.price_start.to_string(),
            price_end: row.price_end.to_string(),
            time_start: row.time_start,
            time_end: row.time_end,
            structural_score: row.structural_score,
            state: row.state,
            children: Vec::new(),
        })
        .collect();

    Ok(Json(AncestorChainResponse { chain }))
}

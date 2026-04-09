//! `GET /v2/tbm` — Faz 7.6 / A5.
//!
//! TBM pillar dashboard read path. Surfaces the latest TBM
//! detections (`family='tbm'`) and projects each row's `raw_meta`
//! into a pillar-shaped wire payload so the v2 frontend can render
//! a per-symbol breakdown without re-implementing the JSON parsing.
//!
//! The TBM detector loop already writes its score + per-pillar
//! breakdown into `qtss_v2_detections.raw_meta` (see
//! `crates/qtss-worker/src/v2_tbm_detector.rs::pillar_meta`). This
//! route is a thin projection — no recomputation, no joins, no
//! recomputed scoring (CLAUDE.md #3: keep layers separated).

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use qtss_storage::{DetectionFilter, DetectionRow, V2DetectionRepository};

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct TbmQuery {
    pub exchange: Option<String>,
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    pub mode: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TbmFeed {
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<TbmEntry>,
}

#[derive(Debug, Serialize)]
pub struct TbmEntry {
    pub id: String,
    pub detected_at: DateTime<Utc>,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub subkind: String,
    pub state: String,
    pub mode: String,
    /// Blended pillar total (0..100).
    pub total: f64,
    /// Signal label (None | Weak | Moderate | Strong | VeryStrong).
    pub signal: String,
    pub pillars: Vec<TbmPillar>,
    pub details: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TbmPillar {
    pub kind: String,
    pub score: f64,
    pub weight: f64,
}

pub fn v2_tbm_router() -> Router<SharedState> {
    Router::new().route("/v2/tbm", get(get_tbm))
}

async fn get_tbm(
    State(st): State<SharedState>,
    Query(q): Query<TbmQuery>,
) -> Result<Json<TbmFeed>, ApiError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 1_000);
    let repo = V2DetectionRepository::new(st.pool.clone());
    let rows = repo
        .list_filtered(DetectionFilter {
            exchange: q.exchange.as_deref(),
            symbol: q.symbol.as_deref(),
            timeframe: q.timeframe.as_deref(),
            family: Some("tbm"),
            state: None,
            mode: q.mode.as_deref(),
            limit,
        })
        .await?;

    Ok(Json(TbmFeed {
        generated_at: Utc::now(),
        entries: rows.into_iter().map(row_to_entry).collect(),
    }))
}

fn row_to_entry(row: DetectionRow) -> TbmEntry {
    let total = row
        .raw_meta
        .get("tbm_score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let signal = row
        .raw_meta
        .get("signal")
        .and_then(|v| v.as_str())
        .unwrap_or("None")
        .to_string();
    let pillars = row
        .raw_meta
        .get("pillars")
        .and_then(|v| v.get("pillars"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_pillar).collect::<Vec<_>>())
        .unwrap_or_default();
    let details = row
        .raw_meta
        .get("details")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    TbmEntry {
        id: row.id.to_string(),
        detected_at: row.detected_at,
        exchange: row.exchange,
        symbol: row.symbol,
        timeframe: row.timeframe,
        subkind: row.subkind,
        state: row.state,
        mode: row.mode,
        total,
        signal,
        pillars,
        details,
    }
}

fn parse_pillar(v: &serde_json::Value) -> Option<TbmPillar> {
    Some(TbmPillar {
        kind: v.get("kind").and_then(|x| x.as_str())?.to_string(),
        score: v.get("score").and_then(|x| x.as_f64()).unwrap_or(0.0),
        weight: v.get("weight").and_then(|x| x.as_f64()).unwrap_or(0.0),
    })
}

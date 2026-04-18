//! `GET /v2/setups` — Faz 8.0.
//!
//! Read-only feed for the Setup Engine. Lists active/closed setup
//! lifecycle rows and exposes a per-setup event timeline. No write
//! endpoints in Faz 8.0 — manual close arrives in a later phase.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use qtss_storage::v2_setup_events::{list_events_for_setup, V2SetupEventRow};
use qtss_storage::v2_setups::{
    fetch_v2_setup, list_v2_setups_filtered, SetupFilter, V2SetupRow,
};

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct SetupsQuery {
    pub limit: Option<i64>,
    pub venue: Option<String>,
    pub state: Option<String>,
    pub profile: Option<String>,
    /// `LIKE` pattern on `alt_type`. E.g. `wyckoff_%` for the Wyckoff feed.
    pub alt_type_like: Option<String>,
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SetupFeed {
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<SetupEntry>,
}

#[derive(Debug, Serialize)]
pub struct SetupEntry {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub venue_class: String,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub profile: String,
    pub alt_type: Option<String>,
    pub state: String,
    pub direction: String,
    pub entry_price: Option<f32>,
    pub entry_sl: Option<f32>,
    pub koruma: Option<f32>,
    pub target_ref: Option<f32>,
    pub risk_pct: Option<f32>,
    pub close_reason: Option<String>,
    pub close_price: Option<f32>,
    pub closed_at: Option<DateTime<Utc>>,
    pub pnl_pct: Option<f32>,
    pub risk_mode: Option<String>,
    /// Faz 9.3.3 — LightGBM P(win) stamped at setup-open. NULL when
    /// the inference sidecar was disabled/unreachable or no active
    /// model was loaded.
    pub ai_score: Option<f32>,
    /// Faz 9.7.5 — `true` once the setup watcher has flipped this
    /// setup into trailing-stop mode (SL ratchets on each new extreme).
    pub trail_mode: Option<bool>,
    pub confluence_id: Option<Uuid>,
    pub raw_meta: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct SetupEventsResponse {
    pub setup_id: Uuid,
    pub events: Vec<SetupEventEntry>,
}

#[derive(Debug, Serialize)]
pub struct SetupEventEntry {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub delivery_state: String,
    pub delivered_at: Option<DateTime<Utc>>,
    pub retries: i32,
}

pub fn v2_setups_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/setups", get(get_setups))
        .route("/v2/setups/{id}", get(get_setup_by_id))
        .route("/v2/setups/{id}/events", get(get_setup_events))
}

async fn get_setups(
    State(st): State<SharedState>,
    Query(q): Query<SetupsQuery>,
) -> Result<Json<SetupFeed>, ApiError> {
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
        alt_type_like: q.alt_type_like,
        symbol: q.symbol,
        timeframe: q.timeframe,
        mode: q.mode,
    };
    let rows = list_v2_setups_filtered(&st.pool, &filter).await?;
    Ok(Json(SetupFeed {
        generated_at: Utc::now(),
        entries: rows.into_iter().map(row_to_entry).collect(),
    }))
}

async fn get_setup_by_id(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<SetupEntry>, ApiError> {
    let row = fetch_v2_setup(&st.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("setup {id} not found")))?;
    Ok(Json(row_to_entry(row)))
}

async fn get_setup_events(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<SetupEventsResponse>, ApiError> {
    let rows = list_events_for_setup(&st.pool, id).await?;
    Ok(Json(SetupEventsResponse {
        setup_id: id,
        events: rows.into_iter().map(event_row_to_entry).collect(),
    }))
}

fn row_to_entry(row: V2SetupRow) -> SetupEntry {
    SetupEntry {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        venue_class: row.venue_class,
        exchange: row.exchange,
        symbol: row.symbol,
        timeframe: row.timeframe,
        profile: row.profile,
        alt_type: row.alt_type,
        state: row.state,
        direction: row.direction,
        entry_price: row.entry_price,
        entry_sl: row.entry_sl,
        koruma: row.koruma,
        target_ref: row.target_ref,
        risk_pct: row.risk_pct,
        close_reason: row.close_reason,
        close_price: row.close_price,
        closed_at: row.closed_at,
        pnl_pct: row.pnl_pct,
        risk_mode: row.risk_mode,
        ai_score: row.ai_score,
        trail_mode: row.trail_mode,
        confluence_id: row.confluence_id,
        raw_meta: row.raw_meta,
    }
}

fn event_row_to_entry(row: V2SetupEventRow) -> SetupEventEntry {
    SetupEventEntry {
        id: row.id,
        created_at: row.created_at,
        event_type: row.event_type,
        payload: row.payload,
        delivery_state: row.delivery_state,
        delivered_at: row.delivered_at,
        retries: row.retries,
    }
}

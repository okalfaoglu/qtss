//! Faz 9.1.3 — Confluence Inspector backend.
//!
//! Exposes the `qtss_v2_setup_rejections` audit trail so the GUI can
//! surface "why didn't we trade X?" for every vetoed candidate:
//!
//!   * `GET  /v2/setup-rejections`           — filtered list
//!   * `GET  /v2/setup-rejections/summary`   — count per reason bucket
//!
//! Filters are query params; none are required. See [`ListQuery`] /
//! [`SummaryQuery`].

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use qtss_storage::{
    list_setup_rejections_filtered, summarize_setup_rejections, RejectionFilter,
    V2SetupRejectionRow,
};

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<i64>,
    pub venue: Option<String>,
    pub reason: Option<String>,
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    pub since_hours: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SummaryQuery {
    pub since_hours: Option<i64>,
    pub venue: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RejectionFeed {
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<RejectionEntry>,
}

#[derive(Debug, Serialize)]
pub struct RejectionEntry {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub venue_class: String,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub profile: String,
    pub direction: String,
    pub reject_reason: String,
    pub confluence_id: Option<Uuid>,
    pub raw_meta: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct RejectionSummary {
    pub generated_at: DateTime<Utc>,
    pub since_hours: i64,
    pub venue_class: Option<String>,
    pub total: i64,
    pub by_reason: Vec<ReasonBucket>,
}

#[derive(Debug, Serialize)]
pub struct ReasonBucket {
    pub reason: String,
    pub n: i64,
}

pub fn v2_setup_rejections_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/setup-rejections", get(list))
        .route("/v2/setup-rejections/summary", get(summary))
}

async fn list(
    State(st): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<RejectionFeed>, ApiError> {
    let filter = RejectionFilter {
        limit: q.limit.unwrap_or(200).clamp(1, 2_000),
        venue_class: q.venue,
        reason: q.reason,
        symbol: q.symbol,
        timeframe: q.timeframe,
        since_hours: q.since_hours,
    };
    let rows = list_setup_rejections_filtered(&st.pool, &filter).await?;
    Ok(Json(RejectionFeed {
        generated_at: Utc::now(),
        entries: rows.into_iter().map(row_to_entry).collect(),
    }))
}

async fn summary(
    State(st): State<SharedState>,
    Query(q): Query<SummaryQuery>,
) -> Result<Json<RejectionSummary>, ApiError> {
    let since_hours = q.since_hours.unwrap_or(24).clamp(1, 24 * 90);
    let venue = q.venue.clone();
    let buckets =
        summarize_setup_rejections(&st.pool, since_hours, venue.as_deref()).await?;
    let total: i64 = buckets.iter().map(|b| b.n).sum();
    Ok(Json(RejectionSummary {
        generated_at: Utc::now(),
        since_hours,
        venue_class: venue,
        total,
        by_reason: buckets
            .into_iter()
            .map(|b| ReasonBucket {
                reason: b.reject_reason,
                n: b.n,
            })
            .collect(),
    }))
}

fn row_to_entry(row: V2SetupRejectionRow) -> RejectionEntry {
    RejectionEntry {
        id: row.id,
        created_at: row.created_at,
        venue_class: row.venue_class,
        exchange: row.exchange,
        symbol: row.symbol,
        timeframe: row.timeframe,
        profile: row.profile,
        direction: row.direction,
        reject_reason: row.reject_reason,
        confluence_id: row.confluence_id,
        raw_meta: row.raw_meta,
    }
}

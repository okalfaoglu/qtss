//! `GET /v2/selected-candidates` — Faz 9.8.22.
//!
//! Read-only window into the selector queue. Operators need to see
//! what's pending / claimed / placed / errored without tailing DB logs.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use qtss_storage::SelectedCandidateRow;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct SelectedCandidatesQuery {
    /// Filter by status: "pending" | "claimed" | "placed" | "errored" | "rejected".
    pub status: Option<String>,
    /// Filter by mode: "dry" | "live" | "backtest".
    pub mode: Option<String>,
    /// Cap on rows (default 100, max 500).
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SelectedCandidateView {
    pub id: i64,
    pub setup_id: Uuid,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub direction: String,
    pub entry_price: Decimal,
    pub sl_price: Decimal,
    pub tp_ladder: serde_json::Value,
    pub risk_pct: Decimal,
    pub mode: String,
    pub status: String,
    pub reject_reason: Option<String>,
    pub last_error: Option<String>,
    pub attempts: i32,
    pub selector_score: Option<Decimal>,
    pub selector_meta: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub placed_at: Option<DateTime<Utc>>,
}

pub fn v2_selected_candidates_router() -> Router<SharedState> {
    Router::new().route("/v2/selected-candidates", get(list))
}

async fn list(
    State(st): State<SharedState>,
    Query(q): Query<SelectedCandidatesQuery>,
) -> Result<Json<Vec<SelectedCandidateView>>, ApiError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let rows = sqlx::query_as::<_, SelectedCandidateRow>(
        r#"
        SELECT id, setup_id, exchange, symbol, timeframe, direction,
               entry_price, sl_price, tp_ladder, risk_pct, mode, status,
               reject_reason, attempts, last_error, selector_score,
               selector_meta, created_at, claimed_at, placed_at
          FROM selected_candidates
         WHERE ($1::text IS NULL OR status = $1)
           AND ($2::text IS NULL OR mode = $2)
         ORDER BY id DESC
         LIMIT $3
        "#,
    )
    .bind(q.status.as_deref())
    .bind(q.mode.as_deref())
    .bind(limit)
    .fetch_all(&st.pool)
    .await
    .map_err(|e| ApiError::internal(format!("selected_candidates: {e}")))?;

    let views: Vec<SelectedCandidateView> = rows.into_iter().map(to_view).collect();
    Ok(Json(views))
}

fn to_view(r: SelectedCandidateRow) -> SelectedCandidateView {
    SelectedCandidateView {
        id: r.id,
        setup_id: r.setup_id,
        exchange: r.exchange,
        symbol: r.symbol,
        timeframe: r.timeframe,
        direction: r.direction,
        entry_price: r.entry_price,
        sl_price: r.sl_price,
        tp_ladder: r.tp_ladder,
        risk_pct: r.risk_pct,
        mode: r.mode,
        status: r.status,
        reject_reason: r.reject_reason,
        last_error: r.last_error,
        attempts: r.attempts,
        selector_score: r.selector_score,
        selector_meta: r.selector_meta,
        created_at: r.created_at,
        claimed_at: r.claimed_at,
        placed_at: r.placed_at,
    }
}

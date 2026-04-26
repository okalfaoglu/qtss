//! `GET /v2/iq-backtest/*` — FAZ 26.6.
//!
//! Read API for the IQ-D / IQ-T backtest module
//! (`qtss-backtest::iq`). The runs table `iq_backtest_runs` is the
//! queryable index of completed backtest runs.
//!
//! Endpoints:
//!   GET  /v2/iq-backtest/runs            — list recent runs
//!   GET  /v2/iq-backtest/runs/:id        — single run detail
//!
//! Writes (dispatch a run) live in a separate module — backtest
//! runs can take minutes; the dispatch endpoint queues the work
//! into a background task and returns 202 immediately. That ships
//! with the GUI Backtest Studio commit.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use qtss_backtest::iq::persistence;
use qtss_backtest::iq::persistence::PersistedRun;

use crate::error::ApiError;
use crate::state::SharedState;

pub fn v2_iq_backtest_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/iq-backtest/runs", get(list_runs))
        .route("/v2/iq-backtest/runs/{id}", get(get_run))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    /// Default 50, capped at 200.
    pub limit: Option<u32>,
}

async fn list_runs(
    State(st): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<PersistedRun>>, ApiError> {
    let limit = q.limit.unwrap_or(50);
    let rows = persistence::list_recent_runs(
        &st.pool,
        q.symbol.as_deref(),
        q.timeframe.as_deref(),
        limit,
    )
    .await
    .map_err(|e| {
        ApiError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            e.to_string(),
        )
    })?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize)]
pub struct RunDetail {
    pub config: serde_json::Value,
    pub report: serde_json::Value,
}

async fn get_run(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RunDetail>, ApiError> {
    let row = persistence::get_run_detail(&st.pool, id).await.map_err(|e| {
        ApiError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            e.to_string(),
        )
    })?;
    let Some((cfg, report)) = row else {
        return Err(ApiError::new(
            axum::http::StatusCode::NOT_FOUND,
            format!("run {id} not found"),
        ));
    };
    let cfg_json = serde_json::to_value(&cfg).map_err(|e| {
        ApiError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            e.to_string(),
        )
    })?;
    Ok(Json(RunDetail {
        config: cfg_json,
        report,
    }))
}

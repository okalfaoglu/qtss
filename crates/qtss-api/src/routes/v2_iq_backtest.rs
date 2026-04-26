//! `GET /v2/iq-backtest/*` — FAZ 26.6.
//!
//! Read API for the IQ-D / IQ-T backtest module
//! (`qtss-backtest::iq`). The runs table `iq_backtest_runs` is the
//! queryable index of completed backtest runs.
//!
//! Endpoints (canonical, `{venue}/{symbol}/{tf}` shape — same
//! convention as `/v2/chart` and `/v2/elliott`):
//!
//!   GET  /v2/iq-backtest/{venue}/{symbol}/{tf}/runs
//!        — runs scoped to a specific (exchange, symbol, timeframe).
//!          `?segment=spot|futures` query param (default `futures`)
//!          mirrors the chart route. `?limit=` caps row count.
//!   GET  /v2/iq-backtest/runs
//!        — global list across all symbols. Optional `?exchange=`,
//!          `?segment=`, `?symbol=`, `?timeframe=`, `?limit=`.
//!   GET  /v2/iq-backtest/runs/{id}
//!        — single run detail by UUID. UUIDs are global so no scope
//!          segment is needed.
//!   POST /v2/iq-backtest/dispatch
//!        — queue a new run from a JSON config payload. Returns 202
//!          immediately with the future run_id; persistence happens
//!          in a tokio task that writes to iq_backtest_runs when
//!          done. Poll the runs list to see when it lands.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use qtss_backtest::iq::persistence;
use qtss_backtest::iq::persistence::PersistedRun;
use qtss_backtest::iq::{
    CostModel, IqBacktestConfig, IqBacktestRunner, IqLifecycleManager,
    IqReplayDetector,
};

use crate::error::ApiError;
use crate::state::SharedState;

pub fn v2_iq_backtest_router() -> Router<SharedState> {
    Router::new()
        // Canonical scoped path — matches /v2/chart/{venue}/{symbol}/{tf}
        // and /v2/elliott/{venue}/{symbol}/{tf} convention.
        .route(
            "/v2/iq-backtest/{venue}/{symbol}/{tf}/runs",
            get(list_runs_scoped),
        )
        // Global list for cross-symbol views (e.g. ops dashboard).
        .route("/v2/iq-backtest/runs", get(list_runs))
        // UUID-keyed detail (UUIDs are global, no scope needed).
        .route("/v2/iq-backtest/runs/{id}", get(get_run))
        // Dispatch — queue a fresh run from a config payload.
        .route("/v2/iq-backtest/dispatch", post(dispatch_run))
}

#[derive(Debug, Deserialize)]
pub struct DispatchPayload {
    /// Full IqBacktestConfig as JSON. Same shape as the example
    /// configs in crates/qtss-backtest/examples/. Validated by serde
    /// at deserialise time; semantic validation (e.g. window sanity)
    /// surfaces from the runner.
    pub config: IqBacktestConfig,
}

#[derive(Debug, Serialize)]
pub struct DispatchResponse {
    /// Submitted-task acknowledgement. The actual run_id appears in
    /// the runs list once the worker completes — poll
    /// /v2/iq-backtest/runs?run_tag=… to find it.
    pub status: String,
    pub run_tag: String,
    pub note: String,
}

/// Spawn a fresh backtest in a tokio task. Returns 202 immediately.
///
/// The task does not stream progress — it persists the report when
/// done and the GUI polls the runs list. Future iteration could add
/// a job table for granular status tracking.
async fn dispatch_run(
    State(st): State<SharedState>,
    Json(payload): Json<DispatchPayload>,
) -> Result<(StatusCode, Json<DispatchResponse>), ApiError> {
    let cfg = payload.config;
    let run_tag = cfg.run_tag.clone();
    let pool = st.pool.clone();

    // Spawn detached task — the runner self-persists when done.
    tokio::spawn(async move {
        let detector = Arc::new(IqReplayDetector::new(cfg.clone()));
        let lifecycle = Arc::new(IqLifecycleManager::new(
            cfg.clone(),
            CostModel::default(),
        ));
        let runner = match IqBacktestRunner::new(cfg.clone()) {
            Ok(r) => r.with_detector(detector).with_lifecycle(lifecycle),
            Err(e) => {
                tracing::error!(
                    error = %e,
                    run_tag = %cfg.run_tag,
                    "iq-backtest dispatch: runner init failed"
                );
                return;
            }
        };
        match runner.run(&pool).await {
            Ok(report) => {
                if let Err(e) =
                    persistence::persist_report(&pool, &report, None).await
                {
                    tracing::warn!(
                        error = %e,
                        run_tag = %cfg.run_tag,
                        "iq-backtest dispatch: persist failed"
                    );
                }
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    run_tag = %cfg.run_tag,
                    "iq-backtest dispatch: runner.run() failed"
                );
            }
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(DispatchResponse {
            status: "queued".into(),
            run_tag,
            note: "poll /v2/iq-backtest/runs to see when the persisted run appears".into(),
        }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub exchange: Option<String>,
    pub segment: Option<String>,
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
        q.exchange.as_deref(),
        q.segment.as_deref(),
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

#[derive(Debug, Deserialize)]
pub struct ScopedQuery {
    /// Defaults to `futures` — same default as `/v2/chart`. Mirror
    /// it here so the same chart-toolbar combobox feeds both.
    pub segment: Option<String>,
    /// Default 50, capped at 200.
    pub limit: Option<u32>,
}

async fn list_runs_scoped(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<ScopedQuery>,
) -> Result<Json<Vec<PersistedRun>>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(50);
    let rows = persistence::list_recent_runs(
        &st.pool,
        Some(&venue),
        Some(&segment),
        Some(&symbol),
        Some(&tf),
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

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
//!   GET  /v2/iq-backtest/runs/{id}/trades
//!        — stream the per-trade JSONL log for a single run as a
//!          regular JSON array. Used by the GUI to power the trade
//!          timeline browser without forcing the operator to open
//!          a terminal and slice the JSONL file by hand.
//!          Optional `?limit=` (default 500, cap 5000),
//!          `?outcome=` to filter (stop_loss / take_profit_full / …),
//!          `?loss_reason=` to filter by attribution category.
//!   POST /v2/iq-backtest/compare
//!        — diff two runs side-by-side. Body: `{ "left": uuid,
//!          "right": uuid }`. Returns the same RunDetail shape for
//!          each side plus a sparse delta block highlighting the
//!          fields that differ (PnL, drawdown, winrate, weights).

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
        // Per-trade JSONL stream — powers the trade timeline GUI.
        .route("/v2/iq-backtest/runs/{id}/trades", get(get_run_trades))
        // Side-by-side diff for two runs.
        .route("/v2/iq-backtest/compare", post(compare_runs))
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
pub struct TradesQuery {
    /// Default 500, cap 5000.
    pub limit: Option<u32>,
    /// Filter by outcome string (stop_loss / take_profit_full / …).
    pub outcome: Option<String>,
    /// Filter by loss reason (StopLossNoTp / CostsOnly / …) when the
    /// JSONL row carries an attribution block.
    pub loss_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TradesResponse {
    pub run_id: Uuid,
    pub run_tag: String,
    pub trade_log_path: Option<String>,
    pub returned: usize,
    pub total_in_file: usize,
    pub trades: Vec<serde_json::Value>,
}

/// Stream the per-trade JSONL log for a single run, optionally
/// filtered by outcome / loss_reason. Reads the file from
/// `trade_log_path` and parses each line into a JSON object the GUI
/// can render directly. Files larger than the limit return only the
/// MOST RECENT N trades (close-time order); GUI uses pagination
/// later when the dataset grows.
async fn get_run_trades(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
    Query(q): Query<TradesQuery>,
) -> Result<Json<TradesResponse>, ApiError> {
    let limit = q.limit.unwrap_or(500).clamp(1, 5_000) as usize;
    // Pull run metadata to learn trade_log_path + run_tag.
    let row = sqlx::query(
        r#"SELECT run_tag, trade_log_path
             FROM iq_backtest_runs
            WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(&st.pool)
    .await
    .map_err(|e| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            e.to_string(),
        )
    })?;
    let Some(row) = row else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            format!("run {id} not found"),
        ));
    };
    use sqlx::Row;
    let run_tag: String = row.try_get("run_tag").map_err(|e| {
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;
    let trade_log_path: Option<String> =
        row.try_get("trade_log_path").ok();

    // No log path → empty array. (Operators can run without --log.)
    let Some(path) = trade_log_path.clone() else {
        return Ok(Json(TradesResponse {
            run_id: id,
            run_tag,
            trade_log_path: None,
            returned: 0,
            total_in_file: 0,
            trades: Vec::new(),
        }));
    };

    // Read the JSONL file. We keep it simple: load entire file
    // into memory, parse lines lazily, apply filters, then take
    // the LAST N. Trade logs at typical sizes (1500 trades for
    // a 15-month BTC 4h backtest) are well under 10 MB.
    let raw = match tokio::fs::read_to_string(&path).await {
        Ok(s) => s,
        Err(e) => {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                format!("trade log not readable at {path}: {e}"),
            ));
        }
    };
    let outcome_filter = q.outcome.as_deref();
    let loss_filter = q.loss_reason.as_deref();
    let mut all: Vec<serde_json::Value> = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue, // skip malformed lines defensively
        };
        // The JSONL row shape is { trade: {...}, attribution: {...} }
        // (TradeLogWriter::write_row). Filters reach inside.
        if let Some(o) = outcome_filter {
            let actual = v
                .get("trade")
                .and_then(|t| t.get("outcome"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if actual != o {
                continue;
            }
        }
        if let Some(lr) = loss_filter {
            let actual = v
                .get("attribution")
                .and_then(|a| a.get("loss_reason"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if actual != lr {
                continue;
            }
        }
        all.push(v);
    }
    let total = all.len();
    // Take the LAST N (most recent in close-time order, matching
    // the JSONL append order). Operator's typical workflow: scroll
    // the most recent losers first.
    let trades = if all.len() > limit {
        all.split_off(all.len() - limit)
    } else {
        all
    };
    Ok(Json(TradesResponse {
        run_id: id,
        run_tag,
        trade_log_path,
        returned: trades.len(),
        total_in_file: total,
        trades,
    }))
}

#[derive(Debug, Deserialize)]
pub struct ComparePayload {
    pub left: Uuid,
    pub right: Uuid,
}

#[derive(Debug, Serialize)]
pub struct CompareResponse {
    pub left: serde_json::Value,
    pub right: serde_json::Value,
    /// Map of field name → { left, right } for fields that differ
    /// at the headline level. Used by the GUI to highlight diffs.
    pub delta: serde_json::Value,
}

/// Side-by-side comparison of two runs. Loads each detail via the
/// existing `get_run_detail`, then computes a sparse delta block
/// of fields whose values differ (numeric / equity_curve length /
/// loss_reason maps) so the GUI can highlight the differences
/// instead of rendering two giant identical blobs.
async fn compare_runs(
    State(st): State<SharedState>,
    Json(payload): Json<ComparePayload>,
) -> Result<Json<CompareResponse>, ApiError> {
    let left = persistence::get_run_detail(&st.pool, payload.left)
        .await
        .map_err(|e| {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
    let right = persistence::get_run_detail(&st.pool, payload.right)
        .await
        .map_err(|e| {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
    let Some((left_cfg, left_report)) = left else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            format!("left run {} not found", payload.left),
        ));
    };
    let Some((right_cfg, right_report)) = right else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            format!("right run {} not found", payload.right),
        ));
    };

    // Headline fields whose absolute / relative gap is meaningful
    // for the operator. Anything not in this list still ships as
    // part of the full RunDetail body for both sides; delta just
    // surfaces the most-watched ones.
    let watch_fields = [
        "total_trades",
        "wins",
        "losses",
        "win_rate",
        "profit_factor",
        "expectancy_pct",
        "net_pnl",
        "gross_pnl",
        "max_drawdown_pct",
        "final_equity",
        "peak_equity",
        "sharpe_ratio",
        "bars_processed",
    ];
    let mut delta_map = serde_json::Map::new();
    for k in &watch_fields {
        let lv = left_report.get(*k).cloned().unwrap_or(serde_json::Value::Null);
        let rv = right_report.get(*k).cloned().unwrap_or(serde_json::Value::Null);
        if lv != rv {
            delta_map.insert(
                (*k).to_string(),
                serde_json::json!({ "left": lv, "right": rv }),
            );
        }
    }
    let left_cfg_v = serde_json::to_value(&left_cfg).map_err(|e| {
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;
    let right_cfg_v = serde_json::to_value(&right_cfg).map_err(|e| {
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;
    Ok(Json(CompareResponse {
        left: serde_json::json!({
            "config": left_cfg_v,
            "report": left_report,
        }),
        right: serde_json::json!({
            "config": right_cfg_v,
            "report": right_report,
        }),
        delta: serde_json::Value::Object(delta_map),
    }))
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

//! Faz 9.4.1 — Shadow Observation API.
//!
//! Read-only endpoints for the AI Shadow dashboard:
//!
//!   * `GET /v2/ml/predictions`             — paginated feed
//!   * `GET /v2/ml/predictions/summary`     — aggregate header stats
//!   * `GET /v2/ml/predictions/distribution` — score histogram
//!
//! No hardcoded defaults — limit / hours come from query params with
//! sensible caller-side defaults (rule #2).

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use qtss_storage::{
    fetch_latest_drift, fetch_latest_drift_model_version,
    fetch_prediction_feed, fetch_prediction_summary,
    fetch_score_distribution, resolve_system_string,
    DriftEntry, PredictionRow, PredictionSummary, ScoreBucket,
    fetch_feature_coverage, fetch_recent_snapshots, fetch_feature_stats,
    SourceCoverage, FeatureSnapshotRow, FeatureStat,
};

use crate::error::ApiError;
use crate::state::SharedState;

// ── Query param structs ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct FeedQuery {
    pub decision: Option<String>,
    pub symbol: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct WindowQuery {
    pub hours: Option<i64>,
}

// ── Response envelopes ───────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct FeedPayload {
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<PredictionRow>,
}

#[derive(Debug, Serialize)]
pub struct SummaryPayload {
    pub generated_at: DateTime<Utc>,
    pub summary: PredictionSummary,
}

#[derive(Debug, Serialize)]
pub struct DistributionPayload {
    pub generated_at: DateTime<Utc>,
    pub buckets: Vec<ScoreBucket>,
}

// ── Drift + Breaker response types (Faz 9.4.2 / 9.4.3) ─────────────

#[derive(Debug, Serialize)]
pub struct DriftPayload {
    pub generated_at: DateTime<Utc>,
    pub model_version: String,
    pub entries: Vec<DriftEntry>,
}

#[derive(Debug, Serialize)]
pub struct BreakerPayload {
    pub generated_at: DateTime<Utc>,
    pub state: String,
    pub gate_pct: f64,
}

// ── Router ───────────────────────────────────────────────────────────

pub fn v2_ml_predictions_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/ml/predictions", get(feed))
        .route("/v2/ml/predictions/summary", get(summary))
        .route("/v2/ml/predictions/distribution", get(distribution))
        .route("/v2/ml/predictions/drift", get(drift))
        .route("/v2/ml/predictions/breaker", get(breaker))
        .route("/v2/ml/features/coverage", get(features_coverage))
        .route("/v2/ml/features/snapshots", get(features_snapshots))
        .route("/v2/ml/features/stats", get(features_stats))
}

// ── Handlers ─────────────────────────────────────────────────────────

async fn feed(
    State(st): State<SharedState>,
    Query(q): Query<FeedQuery>,
) -> Result<Json<FeedPayload>, ApiError> {
    let limit = q.limit.unwrap_or(100);
    let entries = fetch_prediction_feed(
        &st.pool,
        q.decision.as_deref(),
        q.symbol.as_deref(),
        limit,
    )
    .await?;
    Ok(Json(FeedPayload {
        generated_at: Utc::now(),
        entries,
    }))
}

async fn summary(
    State(st): State<SharedState>,
    Query(q): Query<WindowQuery>,
) -> Result<Json<SummaryPayload>, ApiError> {
    let hours = q.hours.unwrap_or(24);
    let summary = fetch_prediction_summary(&st.pool, hours).await?;
    Ok(Json(SummaryPayload {
        generated_at: Utc::now(),
        summary,
    }))
}

async fn distribution(
    State(st): State<SharedState>,
    Query(q): Query<WindowQuery>,
) -> Result<Json<DistributionPayload>, ApiError> {
    let hours = q.hours.unwrap_or(24);
    let buckets = fetch_score_distribution(&st.pool, hours).await?;
    Ok(Json(DistributionPayload {
        generated_at: Utc::now(),
        buckets,
    }))
}

/// Faz 9.4.2 — latest per-feature PSI drift for the active model.
///
/// The model version is resolved from the most recent drift snapshot
/// (written by the Python sidecar) rather than requiring a family
/// parameter. Falls back to an empty response when no snapshots exist.
async fn drift(
    State(st): State<SharedState>,
) -> Result<Json<DriftPayload>, ApiError> {
    let version = fetch_latest_drift_model_version(&st.pool)
        .await?
        .unwrap_or_default();

    let entries = if version.is_empty() {
        vec![]
    } else {
        fetch_latest_drift(&st.pool, &version).await?
    };
    Ok(Json(DriftPayload {
        generated_at: Utc::now(),
        model_version: version,
        entries,
    }))
}

// ── Feature Inspector query params ──────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SnapshotQuery {
    pub source: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct FeatureStatsQuery {
    pub source: String,
    pub hours: Option<i64>,
}

// ── Feature Inspector response envelopes ────────────────────────────

#[derive(Debug, Serialize)]
pub struct CoveragePayload {
    pub generated_at: DateTime<Utc>,
    pub sources: Vec<SourceCoverage>,
}

#[derive(Debug, Serialize)]
pub struct SnapshotsPayload {
    pub generated_at: DateTime<Utc>,
    pub snapshots: Vec<FeatureSnapshotRow>,
}

#[derive(Debug, Serialize)]
pub struct FeatureStatsPayload {
    pub generated_at: DateTime<Utc>,
    pub stats: Vec<FeatureStat>,
}

/// Faz 9.4.3 / 9.4.4 — circuit breaker status + gate ramp percentage.
async fn breaker(
    State(st): State<SharedState>,
) -> Result<Json<BreakerPayload>, ApiError> {
    let state = resolve_system_string(
        &st.pool, "ai", "circuit_breaker.state", "", "closed",
    ).await;
    let gate_pct = qtss_storage::resolve_system_f64(
        &st.pool, "ai", "inference.gate_pct", "", 0.0,
    ).await;
    Ok(Json(BreakerPayload {
        generated_at: Utc::now(),
        state,
        gate_pct,
    }))
}

// ── Feature Inspector handlers ──────────────────────────────────────

async fn features_coverage(
    State(st): State<SharedState>,
    Query(q): Query<WindowQuery>,
) -> Result<Json<CoveragePayload>, ApiError> {
    let hours = q.hours.unwrap_or(24);
    let sources = fetch_feature_coverage(&st.pool, hours).await?;
    Ok(Json(CoveragePayload {
        generated_at: Utc::now(),
        sources,
    }))
}

async fn features_snapshots(
    State(st): State<SharedState>,
    Query(q): Query<SnapshotQuery>,
) -> Result<Json<SnapshotsPayload>, ApiError> {
    let limit = q.limit.unwrap_or(50);
    let snapshots = fetch_recent_snapshots(
        &st.pool,
        q.source.as_deref(),
        limit,
    )
    .await?;
    Ok(Json(SnapshotsPayload {
        generated_at: Utc::now(),
        snapshots,
    }))
}

async fn features_stats(
    State(st): State<SharedState>,
    Query(q): Query<FeatureStatsQuery>,
) -> Result<Json<FeatureStatsPayload>, ApiError> {
    let hours = q.hours.unwrap_or(24);
    let stats = fetch_feature_stats(&st.pool, &q.source, hours).await?;
    Ok(Json(FeatureStatsPayload {
        generated_at: Utc::now(),
        stats,
    }))
}

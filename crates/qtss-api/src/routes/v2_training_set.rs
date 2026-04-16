//! Faz 9.2.1 — Training set monitor.
//!
//! Single read-only endpoint the GUI polls to decide whether the Faz 9.3
//! LightGBM trainer has enough labeled data to kick off:
//!
//!   * `GET /v2/training-set/stats` — totals, per-label histogram,
//!     per-source feature coverage.
//!
//! Readiness thresholds (min closed setups, min coverage %) are resolved
//! from `config_schema` so operators can retune without a redeploy.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;

use qtss_storage::{
    fetch_training_set_stats, resolve_system_f64, resolve_system_u64, TrainingSetStats,
};

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Serialize)]
pub struct TrainingSetPayload {
    pub generated_at: DateTime<Utc>,
    pub stats: TrainingSetStats,
    pub readiness: Readiness,
}

#[derive(Debug, Serialize)]
pub struct Readiness {
    pub min_closed: i64,
    pub min_feature_coverage_pct: f64,
    pub closed_ok: bool,
    pub features_ok: bool,
    pub ready: bool,
}

pub fn v2_training_set_router() -> Router<SharedState> {
    Router::new().route("/v2/training-set/stats", get(stats))
}

async fn stats(State(st): State<SharedState>) -> Result<Json<TrainingSetPayload>, ApiError> {
    let stats = fetch_training_set_stats(&st.pool).await?;

    // Config-driven thresholds (rule #2: no hardcoded constants).
    let min_closed =
        resolve_system_u64(&st.pool, "setup", "training_set.min_closed", "", 500, 0, 1_000_000)
            .await as i64;
    let min_coverage_pct =
        resolve_system_f64(&st.pool, "setup", "training_set.min_feature_coverage_pct", "", 80.0)
            .await;

    let coverage_pct = if stats.total_setups > 0 {
        (stats.setups_with_features as f64 / stats.total_setups as f64) * 100.0
    } else {
        0.0
    };
    let closed_ok = stats.closed_setups >= min_closed;
    let features_ok = coverage_pct >= min_coverage_pct;

    Ok(Json(TrainingSetPayload {
        generated_at: Utc::now(),
        stats,
        readiness: Readiness {
            min_closed,
            min_feature_coverage_pct: min_coverage_pct,
            closed_ok,
            features_ok,
            ready: closed_ok && features_ok,
        },
    }))
}

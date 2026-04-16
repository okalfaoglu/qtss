//! Faz 9.3 — Model registry API.
//!
//! Read/activate endpoints over `qtss_models`:
//!   * `GET  /v2/models`                       — list
//!   * `GET  /v2/models/active?family=...`     — currently-serving row
//!   * `POST /v2/models/activate`              — flip active flag
//!
//! Training itself runs outside the API (Python CLI, `qtss-trainer
//! train`); the backend is read-only w.r.t. artifact content.

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use qtss_storage::{activate_model, active_model, list_models, ModelRow};

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub family: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ActivateBody {
    pub family: String,
    pub version: String,
}

#[derive(Debug, Serialize)]
pub struct ModelEntry {
    pub id: Uuid,
    pub model_family: String,
    pub model_version: String,
    pub feature_spec_version: i32,
    pub algorithm: String,
    pub task: String,
    pub n_train: i64,
    pub n_valid: i64,
    pub metrics: serde_json::Value,
    pub params: serde_json::Value,
    pub feature_count: usize,
    pub artifact_path: String,
    pub artifact_sha256: Option<String>,
    pub trained_at: DateTime<Utc>,
    pub trained_by: Option<String>,
    pub notes: Option<String>,
    pub active: bool,
}

#[derive(Debug, Serialize)]
pub struct ModelList {
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<ModelEntry>,
}

#[derive(Debug, Serialize)]
pub struct ActiveResponse {
    pub family: String,
    pub model: Option<ModelEntry>,
}

pub fn v2_models_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/models", get(list))
        .route("/v2/models/active", get(active))
        .route("/v2/models/activate", post(activate))
}

async fn list(
    State(st): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<ModelList>, ApiError> {
    let rows = list_models(&st.pool, q.family.as_deref()).await?;
    Ok(Json(ModelList {
        generated_at: Utc::now(),
        entries: rows.into_iter().map(into_entry).collect(),
    }))
}

async fn active(
    State(st): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<ActiveResponse>, ApiError> {
    let family = q
        .family
        .ok_or_else(|| ApiError::bad_request("family query param required"))?;
    let row = active_model(&st.pool, &family).await?;
    Ok(Json(ActiveResponse {
        family,
        model: row.map(into_entry),
    }))
}

async fn activate(
    State(st): State<SharedState>,
    Json(body): Json<ActivateBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    activate_model(&st.pool, &body.family, &body.version).await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "family": body.family,
        "version": body.version,
    })))
}

fn into_entry(row: ModelRow) -> ModelEntry {
    ModelEntry {
        id: row.id,
        model_family: row.model_family,
        model_version: row.model_version,
        feature_spec_version: row.feature_spec_version,
        algorithm: row.algorithm,
        task: row.task,
        n_train: row.n_train,
        n_valid: row.n_valid,
        metrics: row.metrics_json,
        params: row.params_json,
        feature_count: row.feature_names.len(),
        artifact_path: row.artifact_path,
        artifact_sha256: row.artifact_sha256,
        trained_at: row.trained_at,
        trained_by: row.trained_by,
        notes: row.notes,
        active: row.active,
    }
}

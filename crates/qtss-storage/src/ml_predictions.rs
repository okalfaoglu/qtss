//! Faz 9.3.5 — `qtss_ml_predictions` repo.
//!
//! One row per sidecar `/score` call. Lives independent of
//! `qtss_v2_setups` — see migration 0125's top-of-file comment for the
//! full rationale.
//!
//! CLAUDE.md:
//!   * #1 — flat INSERT, no if/else chains. Updaters are guard/early-
//!     return only.
//!   * #2 — no hard-coded values live here; callers supply everything
//!     from `system_config` (decision/threshold/sidecar_url/etc).

use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone)]
pub struct MlPredictionInsert {
    pub setup_id: Option<Uuid>,
    pub detection_id: Option<Uuid>,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub model_name: String,
    pub model_version: String,
    pub model_sha: Option<String>,
    pub feature_spec_version: String,
    pub feature_hash: Option<String>,
    pub score: f32,
    pub threshold: f32,
    pub gate_enabled: bool,
    /// One of `"pass"`, `"block"`, `"shadow"` — matches the CHECK
    /// constraint on the table.
    pub decision: String,
    pub shap_top10: Option<JsonValue>,
    pub latency_ms: i32,
    pub sidecar_url: String,
}

/// INSERT one ML prediction row. Returns the new row's UUID so callers
/// can back-fill `setup_id` via `attach_setup_id` once a setup is born.
pub async fn insert_ml_prediction(
    pool: &PgPool,
    row: &MlPredictionInsert,
) -> Result<Uuid, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO qtss_ml_predictions (
            setup_id, detection_id,
            exchange, symbol, timeframe,
            model_name, model_version, model_sha,
            feature_spec_version, feature_hash,
            score, threshold, gate_enabled, decision,
            shap_top10, latency_ms, sidecar_url
        ) VALUES (
            $1,  $2,
            $3,  $4,  $5,
            $6,  $7,  $8,
            $9,  $10,
            $11, $12, $13, $14,
            $15, $16, $17
        )
        RETURNING id
        "#,
    )
    .bind(row.setup_id)
    .bind(row.detection_id)
    .bind(&row.exchange)
    .bind(&row.symbol)
    .bind(&row.timeframe)
    .bind(&row.model_name)
    .bind(&row.model_version)
    .bind(row.model_sha.as_deref())
    .bind(&row.feature_spec_version)
    .bind(row.feature_hash.as_deref())
    .bind(row.score)
    .bind(row.threshold)
    .bind(row.gate_enabled)
    .bind(&row.decision)
    .bind(row.shap_top10.as_ref())
    .bind(row.latency_ms)
    .bind(&row.sidecar_url)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Back-fill `setup_id` onto a prediction row once the owning setup is
/// finally inserted. Soft-fail: the caller logs and moves on — the
/// prediction row is still valid without the link.
pub async fn attach_setup_id(
    pool: &PgPool,
    prediction_id: Uuid,
    setup_id: Uuid,
) -> Result<(), StorageError> {
    sqlx::query("UPDATE qtss_ml_predictions SET setup_id = $1 WHERE id = $2")
        .bind(setup_id)
        .bind(prediction_id)
        .execute(pool)
        .await?;
    Ok(())
}

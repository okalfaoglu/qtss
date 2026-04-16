//! Faz 9.4.1 — Shadow observation dashboard queries.
//!
//! Read-only functions for the AI Shadow page: prediction feed,
//! aggregate summary, and score-distribution histogram.
//!
//! CLAUDE.md:
//!   * #1 — no if/else chains; flat SQL with nullable filters.
//!   * #2 — no hard-coded constants; window / limit come from callers
//!     (which resolve them from config or query params).

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

// ── Row types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PredictionRow {
    pub id: Uuid,
    pub setup_id: Option<Uuid>,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub model_version: String,
    pub score: f32,
    pub threshold: f32,
    pub gate_enabled: bool,
    pub decision: String,
    pub shap_top10: Option<JsonValue>,
    pub latency_ms: i32,
    pub inference_ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PredictionSummary {
    pub total: i64,
    pub n_pass: i64,
    pub n_block: i64,
    pub n_shadow: i64,
    pub avg_score: Option<f64>,
    pub avg_latency_ms: Option<f64>,
    pub avg_pnl_pass: Option<f64>,
    pub avg_pnl_block: Option<f64>,
    pub block_wouldve_won: i64,
    pub block_with_outcome: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ScoreBucket {
    pub bucket: f64,
    pub n: i64,
    pub n_pass: i64,
    pub n_block: i64,
    pub n_shadow: i64,
}

// ── Queries ──────────────────────────────────────────────────────────

/// Paginated feed of recent predictions, optionally filtered by
/// decision and/or symbol.
pub async fn fetch_prediction_feed(
    pool: &PgPool,
    decision: Option<&str>,
    symbol: Option<&str>,
    limit: i64,
) -> Result<Vec<PredictionRow>, StorageError> {
    let rows = sqlx::query_as::<_, PredictionRow>(
        r#"
        SELECT id, setup_id, exchange, symbol, timeframe, model_version,
               score, threshold, gate_enabled, decision, shap_top10,
               latency_ms, inference_ts
          FROM qtss_ml_predictions
         WHERE ($1::text IS NULL OR decision = $1)
           AND ($2::text IS NULL OR symbol = $2)
         ORDER BY inference_ts DESC
         LIMIT $3
        "#,
    )
    .bind(decision)
    .bind(symbol)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Aggregate stats for the shadow dashboard header strip.
pub async fn fetch_prediction_summary(
    pool: &PgPool,
    hours_window: i64,
) -> Result<PredictionSummary, StorageError> {
    let row = sqlx::query_as::<_, PredictionSummary>(
        r#"
        SELECT
            COUNT(*)                                                       AS total,
            COUNT(*) FILTER (WHERE p.decision = 'pass')                    AS n_pass,
            COUNT(*) FILTER (WHERE p.decision = 'block')                   AS n_block,
            COUNT(*) FILTER (WHERE p.decision = 'shadow')                  AS n_shadow,
            AVG(p.score)::float8                                           AS avg_score,
            AVG(p.latency_ms)::float8                                      AS avg_latency_ms,
            AVG(s.pnl_pct) FILTER (WHERE p.decision = 'pass'  AND s.state = 'closed') AS avg_pnl_pass,
            AVG(s.pnl_pct) FILTER (WHERE p.decision = 'block' AND s.state = 'closed') AS avg_pnl_block,
            COUNT(*) FILTER (WHERE p.decision = 'block' AND s.pnl_pct > 0)            AS block_wouldve_won,
            COUNT(*) FILTER (WHERE p.decision = 'block' AND s.pnl_pct IS NOT NULL)     AS block_with_outcome
        FROM qtss_ml_predictions p
        LEFT JOIN qtss_v2_setups s ON s.id = p.setup_id
        WHERE p.inference_ts >= NOW() - ($1 || ' hours')::interval
        "#,
    )
    .bind(hours_window.to_string())
    .fetch_one(pool)
    .await?;
    Ok(row)
}

// ── Drift snapshot types (Faz 9.4.2) ────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct DriftEntry {
    pub feature_name: String,
    pub psi: f32,
    pub computed_at: DateTime<Utc>,
}

/// Fetch latest per-feature PSI for a given model version.
/// Returns one row per feature (the most recent snapshot).
pub async fn fetch_latest_drift(
    pool: &PgPool,
    model_version: &str,
) -> Result<Vec<DriftEntry>, StorageError> {
    let rows = sqlx::query_as::<_, DriftEntry>(
        r#"
        SELECT DISTINCT ON (feature_name)
               feature_name, psi, computed_at
          FROM qtss_ml_drift_snapshots
         WHERE model_version = $1
         ORDER BY feature_name, computed_at DESC
        "#,
    )
    .bind(model_version)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Resolve the model_version from the most recent drift snapshot.
pub async fn fetch_latest_drift_model_version(
    pool: &PgPool,
) -> Result<Option<String>, StorageError> {
    let v: Option<String> = sqlx::query_scalar(
        "SELECT model_version FROM qtss_ml_drift_snapshots ORDER BY computed_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(v)
}

/// Score histogram in 0.05-width buckets for the distribution chart.
pub async fn fetch_score_distribution(
    pool: &PgPool,
    hours_window: i64,
) -> Result<Vec<ScoreBucket>, StorageError> {
    let rows = sqlx::query_as::<_, ScoreBucket>(
        r#"
        SELECT
            (FLOOR(score / 0.05) * 0.05)::float8                   AS bucket,
            COUNT(*)                                                AS n,
            COUNT(*) FILTER (WHERE decision = 'pass')               AS n_pass,
            COUNT(*) FILTER (WHERE decision = 'block')              AS n_block,
            COUNT(*) FILTER (WHERE decision = 'shadow')             AS n_shadow
        FROM qtss_ml_predictions
        WHERE inference_ts >= NOW() - ($1 || ' hours')::interval
        GROUP BY 1
        ORDER BY 1
        "#,
    )
    .bind(hours_window.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

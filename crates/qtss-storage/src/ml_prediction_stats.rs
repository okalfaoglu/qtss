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

// ── Feature Inspector types ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct SourceCoverage {
    pub source: String,
    pub spec_version: i32,
    pub n_snapshots: i64,
    pub first_at: Option<DateTime<Utc>>,
    pub last_at: Option<DateTime<Utc>>,
    pub n_features: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FeatureSnapshotRow {
    pub id: Uuid,
    pub detection_id: Option<Uuid>,
    pub source: String,
    pub feature_spec_version: i32,
    pub features_json: JsonValue,
    pub computed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FeatureStat {
    pub feature: String,
    pub n: i64,
    pub mean: Option<f64>,
    pub min_val: Option<f64>,
    pub max_val: Option<f64>,
    pub stddev: Option<f64>,
}

// ── Feature Inspector queries ───────────────────────────────────────

/// Coverage per source within a time window.
pub async fn fetch_feature_coverage(
    pool: &PgPool,
    hours_window: i64,
) -> Result<Vec<SourceCoverage>, StorageError> {
    let rows = sqlx::query_as::<_, SourceCoverage>(
        r#"
        SELECT fs.source,
               fs.feature_spec_version AS spec_version,
               COUNT(*)::bigint AS n_snapshots,
               MIN(fs.computed_at) AS first_at,
               MAX(fs.computed_at) AS last_at,
               (SELECT COUNT(DISTINCT k)::bigint
                  FROM qtss_features_snapshot fs2,
                       LATERAL jsonb_object_keys(fs2.features_json) k
                 WHERE fs2.source = fs.source
                   AND fs2.computed_at >= NOW() - ($1 || ' hours')::interval
               ) AS n_features
          FROM qtss_features_snapshot fs
         WHERE fs.computed_at >= NOW() - ($1 || ' hours')::interval
         GROUP BY fs.source, fs.feature_spec_version
         ORDER BY fs.source
        "#,
    )
    .bind(hours_window.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Recent feature snapshots, optionally filtered by source.
pub async fn fetch_recent_snapshots(
    pool: &PgPool,
    source: Option<&str>,
    limit: i64,
) -> Result<Vec<FeatureSnapshotRow>, StorageError> {
    let rows = sqlx::query_as::<_, FeatureSnapshotRow>(
        r#"
        SELECT id, detection_id, source, feature_spec_version,
               features_json, computed_at
          FROM qtss_features_snapshot
         WHERE ($1::text IS NULL OR source = $1)
         ORDER BY computed_at DESC
         LIMIT $2
        "#,
    )
    .bind(source)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Per-feature min/max/mean/stddev for a given source within a window.
pub async fn fetch_feature_stats(
    pool: &PgPool,
    source: &str,
    hours_window: i64,
) -> Result<Vec<FeatureStat>, StorageError> {
    let rows = sqlx::query_as::<_, FeatureStat>(
        r#"
        WITH kv AS (
            SELECT k AS feature, v::text::float8 AS val
              FROM qtss_features_snapshot,
                   LATERAL jsonb_each(features_json) AS j(k, v)
             WHERE source = $1
               AND computed_at >= NOW() - ($2 || ' hours')::interval
               AND jsonb_typeof(v) = 'number'
        )
        SELECT feature,
               COUNT(*)::bigint AS n,
               AVG(val) AS mean,
               MIN(val) AS min_val,
               MAX(val) AS max_val,
               STDDEV(val) AS stddev
          FROM kv
         GROUP BY feature
         ORDER BY feature
        "#,
    )
    .bind(source)
    .bind(hours_window.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows)
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

//! Faz 9.2.1 — Training set monitor storage.
//!
//! Aggregates coverage + readiness stats off `v_qtss_training_set`
//! so the GUI can tell operators *when* the Faz 9.3 trainer has enough
//! labeled data to spin up.
//!
//! Readiness thresholds (min closed setups, min feature coverage, min
//! per-label counts) are config-driven and live in `config_schema`
//! under the `setup.training_set` prefix.

use serde::Serialize;
use sqlx::{FromRow, PgPool};

use crate::error::StorageError;

/// Single row of a per-label histogram (win / loss / null / ...).
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct LabelBucket {
    pub label: String,
    pub n: i64,
}

/// Coverage of a single `ConfluenceSource` slug across the training set.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct FeatureCoverage {
    pub source: String,
    pub n: i64,
}

/// Close-reason bucket (tp_hit / sl_hit / timeout / manual / ...).
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct CloseReasonBucket {
    pub reason: String,
    pub category: Option<String>,
    pub n: i64,
}

/// Aggregate PnL summary for the closed+labeled slice.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PnlSummary {
    pub n_closed: i64,
    pub n_win: i64,
    pub n_loss: i64,
    pub n_other: i64,
    pub avg_rr: Option<f64>,
    pub avg_pnl_pct: Option<f64>,
    pub best_rr: Option<f64>,
    pub worst_rr: Option<f64>,
}

/// One-shot stats bundle consumed by `GET /v2/training-set/stats`.
#[derive(Debug, Clone, Serialize)]
pub struct TrainingSetStats {
    pub total_setups: i64,
    pub closed_setups: i64,
    pub labeled_setups: i64,
    pub setups_with_features: i64,
    pub label_distribution: Vec<LabelBucket>,
    pub feature_coverage: Vec<FeatureCoverage>,
    pub close_reasons: Vec<CloseReasonBucket>,
    pub pnl: PnlSummary,
}

/// Produce the full stats payload for the training-set monitor.
///
/// Single-DB-round-trip-per-metric; five small aggregates over
/// `v_qtss_training_set`, no materialization.
pub async fn fetch_training_set_stats(pool: &PgPool) -> Result<TrainingSetStats, StorageError> {
    let totals: (i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*)::BIGINT                                             AS total,
            COUNT(*) FILTER (WHERE closed_at IS NOT NULL)::BIGINT        AS closed,
            COUNT(*) FILTER (WHERE label IS NOT NULL)::BIGINT            AS labeled,
            COUNT(*) FILTER (WHERE features_by_source IS NOT NULL)::BIGINT AS with_features
        FROM v_qtss_training_set
        "#,
    )
    .fetch_one(pool)
    .await?;

    let label_distribution: Vec<LabelBucket> = sqlx::query_as::<_, LabelBucket>(
        r#"
        SELECT COALESCE(label, 'unlabeled') AS label, COUNT(*)::BIGINT AS n
        FROM v_qtss_training_set
        GROUP BY 1
        ORDER BY n DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let feature_coverage: Vec<FeatureCoverage> = sqlx::query_as::<_, FeatureCoverage>(
        r#"
        SELECT src AS source, COUNT(*)::BIGINT AS n
        FROM v_qtss_training_set,
             LATERAL UNNEST(feature_sources) AS src
        GROUP BY 1
        ORDER BY n DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    // Faz 9.3.4 — close-reason breakdown so the monitor answers
    // "kaç tanesi kâr, kaç tanesi stop, kaç tanesi timeout" at a glance.
    // `category` groups variants (e.g. `tp1_hit`/`tp2_hit` → `take_profit`).
    let close_reasons: Vec<CloseReasonBucket> = sqlx::query_as::<_, CloseReasonBucket>(
        r#"
        SELECT
            COALESCE(close_reason, 'unknown')            AS reason,
            category,
            COUNT(*)::BIGINT                             AS n
        FROM v_qtss_training_set
        WHERE closed_at IS NOT NULL
        GROUP BY 1, 2
        ORDER BY n DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    // PnL / RR aggregate over labeled + closed rows only.
    let pnl: PnlSummary = sqlx::query_as::<_, PnlSummary>(
        r#"
        SELECT
            COUNT(*)::BIGINT                                           AS n_closed,
            COUNT(*) FILTER (WHERE label = 'win')::BIGINT              AS n_win,
            COUNT(*) FILTER (WHERE label = 'loss')::BIGINT             AS n_loss,
            COUNT(*) FILTER (WHERE label NOT IN ('win','loss')
                             OR label IS NULL)::BIGINT                 AS n_other,
            AVG(realized_rr)::FLOAT8                                   AS avg_rr,
            AVG(outcome_pnl_pct)::FLOAT8                               AS avg_pnl_pct,
            MAX(realized_rr)::FLOAT8                                   AS best_rr,
            MIN(realized_rr)::FLOAT8                                   AS worst_rr
        FROM v_qtss_training_set
        WHERE closed_at IS NOT NULL
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(TrainingSetStats {
        total_setups: totals.0,
        closed_setups: totals.1,
        labeled_setups: totals.2,
        setups_with_features: totals.3,
        label_distribution,
        feature_coverage,
        close_reasons,
        pnl,
    })
}

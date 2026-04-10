//! `qtss_v2_detection_outcomes` — resolved detection outcomes for validator self-learning (migration 0040).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DetectionOutcomeRow {
    pub id: Uuid,
    pub detection_id: Uuid,
    pub setup_id: Option<Uuid>,
    pub outcome: String,
    pub close_reason: Option<String>,
    pub pnl_pct: Option<f32>,
    pub entry_price: Option<f32>,
    pub exit_price: Option<f32>,
    pub duration_secs: Option<i64>,
    pub resolved_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// Aggregated hit-rate per (family, subkind, timeframe) computed from real outcomes.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct OutcomeHitRate {
    pub family: String,
    pub subkind: String,
    pub timeframe: String,
    pub total: i64,
    pub wins: i64,
    pub losses: i64,
    pub scratches: i64,
    pub win_rate: f64,
}

pub struct DetectionOutcomeRepository {
    pool: PgPool,
}

impl DetectionOutcomeRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Record a detection outcome. Idempotent (ON CONFLICT detection_id DO NOTHING).
    pub async fn record(
        &self,
        detection_id: Uuid,
        setup_id: Option<Uuid>,
        outcome: &str,
        close_reason: Option<&str>,
        pnl_pct: Option<f32>,
        entry_price: Option<f32>,
        exit_price: Option<f32>,
        duration_secs: Option<i64>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"INSERT INTO qtss_v2_detection_outcomes
                   (detection_id, setup_id, outcome, close_reason, pnl_pct,
                    entry_price, exit_price, duration_secs)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
               ON CONFLICT (detection_id) DO NOTHING"#,
        )
        .bind(detection_id)
        .bind(setup_id)
        .bind(outcome)
        .bind(close_reason)
        .bind(pnl_pct)
        .bind(entry_price)
        .bind(exit_price)
        .bind(duration_secs)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Aggregated hit rates from real outcomes — replaces the cheap proxy
    /// in `historical_outcome_counts()`. Groups by detection family/subkind/timeframe.
    pub async fn hit_rates(&self) -> Result<Vec<OutcomeHitRate>, StorageError> {
        let rows = sqlx::query_as::<_, OutcomeHitRate>(
            r#"SELECT d.family,
                      d.subkind,
                      d.timeframe,
                      COUNT(*)                                     AS total,
                      COUNT(*) FILTER (WHERE o.outcome = 'win')    AS wins,
                      COUNT(*) FILTER (WHERE o.outcome = 'loss')   AS losses,
                      COUNT(*) FILTER (WHERE o.outcome = 'scratch') AS scratches,
                      CASE WHEN COUNT(*) > 0
                           THEN COUNT(*) FILTER (WHERE o.outcome = 'win')::float / COUNT(*)::float
                           ELSE 0.0 END                            AS win_rate
                 FROM qtss_v2_detection_outcomes o
                 JOIN qtss_v2_detections d ON d.id = o.detection_id
                GROUP BY d.family, d.subkind, d.timeframe"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Recent outcomes for a specific symbol (for UI).
    pub async fn list_for_symbol(
        &self,
        exchange: &str,
        symbol: &str,
        limit: i64,
    ) -> Result<Vec<DetectionOutcomeRow>, StorageError> {
        let lim = limit.clamp(1, 200);
        let rows = sqlx::query_as::<_, DetectionOutcomeRow>(
            r#"SELECT o.id, o.detection_id, o.setup_id, o.outcome, o.close_reason,
                      o.pnl_pct, o.entry_price, o.exit_price, o.duration_secs,
                      o.resolved_at, o.created_at
                 FROM qtss_v2_detection_outcomes o
                 JOIN qtss_v2_detections d ON d.id = o.detection_id
                WHERE d.exchange = $1 AND d.symbol = $2
                ORDER BY o.resolved_at DESC LIMIT $3"#,
        )
        .bind(exchange)
        .bind(symbol)
        .bind(lim)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

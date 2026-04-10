//! `reconcile_reports` — persisted v2 reconciliation snapshots (migration 0038).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ReconcileReportRow {
    pub id: i64,
    pub user_id: Uuid,
    pub venue: String,
    pub overall: String,
    pub position_drifts: JsonValue,
    pub order_drifts: JsonValue,
    pub position_count: i32,
    pub order_count: i32,
    pub created_at: DateTime<Utc>,
}

pub struct ReconcileReportRepository {
    pool: PgPool,
}

impl ReconcileReportRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Persist a reconciliation report snapshot.
    pub async fn insert(
        &self,
        user_id: Uuid,
        venue: &str,
        overall: &str,
        position_drifts: JsonValue,
        order_drifts: JsonValue,
        position_count: i32,
        order_count: i32,
    ) -> Result<ReconcileReportRow, StorageError> {
        let row = sqlx::query_as::<_, ReconcileReportRow>(
            r#"INSERT INTO reconcile_reports
                   (user_id, venue, overall, position_drifts, order_drifts, position_count, order_count)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               RETURNING id, user_id, venue, overall, position_drifts, order_drifts,
                         position_count, order_count, created_at"#,
        )
        .bind(user_id)
        .bind(venue.trim())
        .bind(overall)
        .bind(position_drifts)
        .bind(order_drifts)
        .bind(position_count)
        .bind(order_count)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Recent reports for a user+venue (newest first).
    pub async fn list(
        &self,
        user_id: Uuid,
        venue: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ReconcileReportRow>, StorageError> {
        let lim = limit.clamp(1, 100);
        let rows = match venue {
            Some(v) => {
                sqlx::query_as::<_, ReconcileReportRow>(
                    r#"SELECT id, user_id, venue, overall, position_drifts, order_drifts,
                              position_count, order_count, created_at
                       FROM reconcile_reports
                       WHERE user_id = $1 AND venue = $2
                       ORDER BY created_at DESC LIMIT $3"#,
                )
                .bind(user_id)
                .bind(v.trim())
                .bind(lim)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as::<_, ReconcileReportRow>(
                    r#"SELECT id, user_id, venue, overall, position_drifts, order_drifts,
                              position_count, order_count, created_at
                       FROM reconcile_reports
                       WHERE user_id = $1
                       ORDER BY created_at DESC LIMIT $2"#,
                )
                .bind(user_id)
                .bind(lim)
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows)
    }

    /// Single report by id.
    pub async fn get_by_id(&self, id: i64) -> Result<Option<ReconcileReportRow>, StorageError> {
        let row = sqlx::query_as::<_, ReconcileReportRow>(
            r#"SELECT id, user_id, venue, overall, position_drifts, order_drifts,
                      position_count, order_count, created_at
               FROM reconcile_reports WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Latest report per venue for a user.
    pub async fn latest_per_venue(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<ReconcileReportRow>, StorageError> {
        let rows = sqlx::query_as::<_, ReconcileReportRow>(
            r#"SELECT DISTINCT ON (venue)
                      id, user_id, venue, overall, position_drifts, order_drifts,
                      position_count, order_count, created_at
               FROM reconcile_reports
               WHERE user_id = $1
               ORDER BY venue, created_at DESC"#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

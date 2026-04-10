//! `account_drawdown_snapshots` — periodic drawdown persistence (migration 0039).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DrawdownSnapshotRow {
    pub id: i64,
    pub user_id: Uuid,
    pub exchange: String,
    pub peak_equity: Decimal,
    pub current_equity: Decimal,
    pub drawdown_pct: Decimal,
    pub snapped_at: DateTime<Utc>,
}

pub struct AccountDrawdownRepository {
    pool: PgPool,
}

impl AccountDrawdownRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a snapshot.
    pub async fn insert(
        &self,
        user_id: Uuid,
        exchange: &str,
        peak_equity: Decimal,
        current_equity: Decimal,
        drawdown_pct: Decimal,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"INSERT INTO account_drawdown_snapshots
                   (user_id, exchange, peak_equity, current_equity, drawdown_pct)
               VALUES ($1, $2, $3, $4, $5)"#,
        )
        .bind(user_id)
        .bind(exchange)
        .bind(peak_equity)
        .bind(current_equity)
        .bind(drawdown_pct)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Recent snapshots for a user+exchange.
    pub async fn history(
        &self,
        user_id: Uuid,
        exchange: &str,
        limit: i64,
    ) -> Result<Vec<DrawdownSnapshotRow>, StorageError> {
        let lim = limit.clamp(1, 1000);
        let rows = sqlx::query_as::<_, DrawdownSnapshotRow>(
            r#"SELECT id, user_id, exchange, peak_equity, current_equity, drawdown_pct, snapped_at
               FROM account_drawdown_snapshots
               WHERE user_id = $1 AND exchange = $2
               ORDER BY snapped_at DESC LIMIT $3"#,
        )
        .bind(user_id)
        .bind(exchange)
        .bind(lim)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Latest snapshot per exchange for a user (for bootstrap).
    pub async fn latest(
        &self,
        user_id: Uuid,
        exchange: &str,
    ) -> Result<Option<DrawdownSnapshotRow>, StorageError> {
        let row = sqlx::query_as::<_, DrawdownSnapshotRow>(
            r#"SELECT id, user_id, exchange, peak_equity, current_equity, drawdown_pct, snapped_at
               FROM account_drawdown_snapshots
               WHERE user_id = $1 AND exchange = $2
               ORDER BY snapped_at DESC LIMIT 1"#,
        )
        .bind(user_id)
        .bind(exchange)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }
}

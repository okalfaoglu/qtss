//! Canlı / dry ledger için özet P&L rollup (dashboard ve raporlama).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PnlBucket {
    Instant,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct PnlRollupRow {
    pub org_id: Uuid,
    pub exchange: String,
    pub symbol: Option<String>,
    pub ledger: String,
    pub bucket: String,
    pub period_start: DateTime<Utc>,
    pub realized_pnl: Decimal,
    pub fees: Decimal,
    pub volume: Decimal,
    pub trade_count: i64,
}

pub struct PnlRollupRepository {
    pool: PgPool,
}

impl PnlRollupRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Dashboard için: belirli ledger (`live`/`dry`) ve bucket’a göre özet.
    pub async fn list_rollups(
        &self,
        ledger: &str,
        bucket: &str,
        limit: i64,
    ) -> Result<Vec<PnlRollupRow>, StorageError> {
        let rows = sqlx::query_as::<_, PnlRollupRow>(
            r#"SELECT org_id, exchange, symbol, ledger, bucket, period_start,
                      realized_pnl, fees, volume, trade_count
               FROM pnl_rollups
               WHERE ledger = $1 AND bucket = $2
               ORDER BY period_start DESC
               LIMIT $3"#,
        )
        .bind(ledger)
        .bind(bucket)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

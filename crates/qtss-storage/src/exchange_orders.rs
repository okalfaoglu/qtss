//! `exchange_orders` — gönderilen borsa emirleri.

use chrono::{DateTime, Utc};
use qtss_domain::orders::OrderIntent;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ExchangeOrderRow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub client_order_id: Uuid,
    pub status: String,
    pub intent: serde_json::Value,
    pub venue_order_id: Option<i64>,
    pub venue_response: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct ExchangeOrderRepository {
    pool: PgPool,
}

impl ExchangeOrderRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Başarılı `place` sonrası (borsa yanıtı varsa `venue_*` doldurulur).
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_submitted(
        &self,
        org_id: Uuid,
        user_id: Uuid,
        exchange: &str,
        segment: &str,
        symbol: &str,
        client_order_id: Uuid,
        intent: &OrderIntent,
        venue_order_id: Option<i64>,
        venue_response: Option<serde_json::Value>,
    ) -> Result<ExchangeOrderRow, StorageError> {
        let intent_v = serde_json::to_value(intent)
            .map_err(|e| StorageError::Other(format!("intent json: {e}")))?;
        let row = sqlx::query_as::<_, ExchangeOrderRow>(
            r#"INSERT INTO exchange_orders (
                   org_id, user_id, exchange, segment, symbol,
                   client_order_id, status, intent, venue_order_id, venue_response
               ) VALUES ($1, $2, $3, $4, $5, $6, 'submitted', $7, $8, $9)
               RETURNING id, org_id, user_id, exchange, segment, symbol,
                         client_order_id, status, intent, venue_order_id,
                         venue_response, created_at, updated_at"#,
        )
        .bind(org_id)
        .bind(user_id)
        .bind(exchange)
        .bind(segment)
        .bind(symbol)
        .bind(client_order_id)
        .bind(intent_v)
        .bind(venue_order_id)
        .bind(venue_response)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn mark_canceled(
        &self,
        user_id: Uuid,
        client_order_id: Uuid,
    ) -> Result<u64, StorageError> {
        let res = sqlx::query(
            r#"UPDATE exchange_orders
               SET status = 'canceled', updated_at = now()
               WHERE user_id = $1 AND client_order_id = $2"#,
        )
        .bind(user_id)
        .bind(client_order_id)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Mutabakat: `submitted` + `venue_order_id` borsanın açık listesinde yok → `reconciled_not_open`.
    pub async fn mark_submitted_reconciled_not_open_by_venue_ids(
        &self,
        user_id: Uuid,
        exchange: &str,
        segment: &str,
        venue_order_ids: &[i64],
    ) -> Result<u64, StorageError> {
        if venue_order_ids.is_empty() {
            return Ok(0);
        }
        let res = sqlx::query(
            r#"UPDATE exchange_orders
               SET status = 'reconciled_not_open', updated_at = now()
               WHERE user_id = $1 AND exchange = $2 AND segment = $3
                 AND status = 'submitted'
                 AND venue_order_id = ANY($4)"#,
        )
        .bind(user_id)
        .bind(exchange)
        .bind(segment)
        .bind(venue_order_ids)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    pub async fn list_for_user(
        &self,
        user_id: Uuid,
        limit: i64,
    ) -> Result<Vec<ExchangeOrderRow>, StorageError> {
        let rows = sqlx::query_as::<_, ExchangeOrderRow>(
            r#"SELECT id, org_id, user_id, exchange, segment, symbol,
                      client_order_id, status, intent, venue_order_id,
                      venue_response, created_at, updated_at
               FROM exchange_orders
               WHERE user_id = $1
               ORDER BY created_at DESC
               LIMIT $2"#,
        )
        .bind(user_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// PLAN Phase D — canlı dolum bildirimi: `place` sonrası `venue_response` içinde gerçekleşen miktar.
    pub async fn list_filled_orders_created_after(
        &self,
        after: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ExchangeOrderRow>, StorageError> {
        let lim = limit.clamp(1, 200);
        let rows = sqlx::query_as::<_, ExchangeOrderRow>(
            r#"SELECT id, org_id, user_id, exchange, segment, symbol,
                      client_order_id, status, intent, venue_order_id,
                      venue_response, created_at, updated_at
               FROM exchange_orders
               WHERE created_at > $1
               AND venue_response IS NOT NULL
               AND (
                   venue_response->>'status' IN ('FILLED', 'PARTIALLY_FILLED')
                   OR (
                       COALESCE(NULLIF(TRIM(venue_response->>'executedQty'), ''), '0')::numeric > 0
                   )
               )
               ORDER BY created_at ASC
               LIMIT $2"#,
        )
        .bind(after)
        .bind(lim)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Son dolumlar — pozisyon / SL-TP izleme (tüm kullanıcılar, yeniden eskiye).
    pub async fn list_recent_filled_orders_global(
        &self,
        limit: i64,
    ) -> Result<Vec<ExchangeOrderRow>, StorageError> {
        let lim = limit.clamp(1, 2000);
        let rows = sqlx::query_as::<_, ExchangeOrderRow>(
            r#"SELECT id, org_id, user_id, exchange, segment, symbol,
                      client_order_id, status, intent, venue_order_id,
                      venue_response, created_at, updated_at
               FROM exchange_orders
               WHERE venue_response IS NOT NULL
               AND (
                   venue_response->>'status' IN ('FILLED', 'PARTIALLY_FILLED')
                   OR (
                       COALESCE(NULLIF(TRIM(venue_response->>'executedQty'), ''), '0')::numeric > 0
                   )
               )
               ORDER BY created_at DESC
               LIMIT $1"#,
        )
        .bind(lim)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// `list_recent_filled_orders_global` ile aynı filtre; `created_at >= since` (copy-trade kuyruk taraması).
    pub async fn list_recent_filled_orders_global_since(
        &self,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ExchangeOrderRow>, StorageError> {
        let lim = limit.clamp(1, 2000);
        let rows = sqlx::query_as::<_, ExchangeOrderRow>(
            r#"SELECT id, org_id, user_id, exchange, segment, symbol,
                      client_order_id, status, intent, venue_order_id,
                      venue_response, created_at, updated_at
               FROM exchange_orders
               WHERE venue_response IS NOT NULL
               AND created_at >= $1
               AND (
                   venue_response->>'status' IN ('FILLED', 'PARTIALLY_FILLED')
                   OR (
                       COALESCE(NULLIF(TRIM(venue_response->>'executedQty'), ''), '0')::numeric > 0
                   )
               )
               ORDER BY created_at DESC
               LIMIT $2"#,
        )
        .bind(since)
        .bind(lim)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

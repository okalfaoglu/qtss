//! `exchange_fills` — canlı dolumlar (WS / reconcile).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ExchangeFillRow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub venue_order_id: i64,
    pub venue_trade_id: Option<i64>,
    pub fill_price: Option<Decimal>,
    pub fill_quantity: Option<Decimal>,
    pub fee: Option<Decimal>,
    pub fee_asset: Option<String>,
    pub event_time: DateTime<Utc>,
    pub raw_event: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

pub struct ExchangeFillRepository {
    pool: PgPool,
}

impl ExchangeFillRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_if_absent(
        &self,
        org_id: Uuid,
        user_id: Uuid,
        exchange: &str,
        segment: &str,
        symbol: &str,
        venue_order_id: i64,
        venue_trade_id: Option<i64>,
        fill_price: Option<Decimal>,
        fill_quantity: Option<Decimal>,
        fee: Option<Decimal>,
        fee_asset: Option<&str>,
        event_time: Option<DateTime<Utc>>,
        raw_event: Option<serde_json::Value>,
    ) -> Result<Option<ExchangeFillRow>, StorageError> {
        let row = sqlx::query_as::<_, ExchangeFillRow>(
            r#"INSERT INTO exchange_fills (
                   org_id, user_id, exchange, segment, symbol,
                   venue_order_id, venue_trade_id,
                   fill_price, fill_quantity, fee, fee_asset,
                   event_time, raw_event
               ) VALUES (
                   $1, $2, $3, $4, $5,
                   $6, $7,
                   $8, $9, $10, $11,
                   COALESCE($12, now()), $13
               )
               ON CONFLICT (exchange, segment, user_id, venue_order_id, venue_trade_id)
               DO NOTHING
               RETURNING id, org_id, user_id, exchange, segment, symbol,
                         venue_order_id, venue_trade_id,
                         fill_price, fill_quantity, fee, fee_asset,
                         event_time, raw_event, created_at"#,
        )
        .bind(org_id)
        .bind(user_id)
        .bind(exchange)
        .bind(segment)
        .bind(symbol)
        .bind(venue_order_id)
        .bind(venue_trade_id)
        .bind(fill_price)
        .bind(fill_quantity)
        .bind(fee)
        .bind(fee_asset)
        .bind(event_time)
        .bind(raw_event)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }

    pub async fn list_recent_for_user(
        &self,
        user_id: Uuid,
        limit: i64,
    ) -> Result<Vec<ExchangeFillRow>, StorageError> {
        let lim = limit.clamp(1, 500);
        let rows = sqlx::query_as::<_, ExchangeFillRow>(
            r#"SELECT id, org_id, user_id, exchange, segment, symbol,
                      venue_order_id, venue_trade_id,
                      fill_price, fill_quantity, fee, fee_asset,
                      event_time, raw_event, created_at
               FROM exchange_fills
               WHERE user_id = $1
               ORDER BY event_time DESC
               LIMIT $2"#,
        )
        .bind(user_id)
        .bind(lim)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}


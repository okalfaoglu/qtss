//! Paper / dry-run defter — `paper_balances`, `paper_fills`.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use qtss_domain::orders::OrderIntent;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PaperBalanceRow {
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub quote_balance: Decimal,
    pub base_positions: Json<HashMap<String, Decimal>>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PaperFillRow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub client_order_id: Uuid,
    pub side: String,
    pub quantity: Decimal,
    pub avg_price: Decimal,
    pub fee: Decimal,
    pub quote_balance_after: Decimal,
    pub base_positions_after: Json<HashMap<String, Decimal>>,
    pub intent: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

pub struct PaperLedgerRepository {
    pool: sqlx::PgPool,
}

impl PaperLedgerRepository {
    #[must_use]
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    pub async fn fetch_balance(
        &self,
        user_id: Uuid,
    ) -> Result<Option<PaperBalanceRow>, StorageError> {
        let row = sqlx::query_as::<_, PaperBalanceRow>(
            r#"SELECT user_id, org_id, quote_balance, base_positions, updated_at
               FROM paper_balances
               WHERE user_id = $1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn lock_balance_for_update(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        user_id: Uuid,
    ) -> Result<Option<PaperBalanceRow>, StorageError> {
        let row = sqlx::query_as::<_, PaperBalanceRow>(
            r#"SELECT user_id, org_id, quote_balance, base_positions, updated_at
               FROM paper_balances
               WHERE user_id = $1
               FOR UPDATE"#,
        )
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await?;
        Ok(row)
    }

    pub async fn insert_balance(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        org_id: Uuid,
        user_id: Uuid,
        initial_quote: Decimal,
    ) -> Result<PaperBalanceRow, StorageError> {
        let empty = Json(HashMap::<String, Decimal>::new());
        let row = sqlx::query_as::<_, PaperBalanceRow>(
            r#"INSERT INTO paper_balances (user_id, org_id, quote_balance, base_positions)
               VALUES ($1, $2, $3, $4)
               RETURNING user_id, org_id, quote_balance, base_positions, updated_at"#,
        )
        .bind(user_id)
        .bind(org_id)
        .bind(initial_quote)
        .bind(empty)
        .fetch_one(&mut **tx)
        .await?;
        Ok(row)
    }

    pub async fn update_balance(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        user_id: Uuid,
        quote_balance: Decimal,
        base_positions: &HashMap<String, Decimal>,
    ) -> Result<(), StorageError> {
        let j = Json(base_positions.clone());
        sqlx::query(
            r#"UPDATE paper_balances
               SET quote_balance = $1, base_positions = $2, updated_at = now()
               WHERE user_id = $3"#,
        )
        .bind(quote_balance)
        .bind(j)
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_fill(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        org_id: Uuid,
        user_id: Uuid,
        exchange: &str,
        segment: &str,
        symbol: &str,
        client_order_id: Uuid,
        side: &str,
        quantity: Decimal,
        avg_price: Decimal,
        fee: Decimal,
        quote_balance_after: Decimal,
        base_positions_after: &HashMap<String, Decimal>,
        intent: &OrderIntent,
    ) -> Result<PaperFillRow, StorageError> {
        let intent_v = serde_json::to_value(intent)
            .map_err(|e| StorageError::Other(format!("intent json: {e}")))?;
        let bases = Json(base_positions_after.clone());
        let row = sqlx::query_as::<_, PaperFillRow>(
            r#"INSERT INTO paper_fills (
                   org_id, user_id, exchange, segment, symbol,
                   client_order_id, side, quantity, avg_price, fee,
                   quote_balance_after, base_positions_after, intent
               ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
               RETURNING id, org_id, user_id, exchange, segment, symbol,
                         client_order_id, side, quantity, avg_price, fee,
                         quote_balance_after, base_positions_after, intent, created_at"#,
        )
        .bind(org_id)
        .bind(user_id)
        .bind(exchange)
        .bind(segment)
        .bind(symbol)
        .bind(client_order_id)
        .bind(side)
        .bind(quantity)
        .bind(avg_price)
        .bind(fee)
        .bind(quote_balance_after)
        .bind(bases)
        .bind(intent_v)
        .fetch_one(&mut **tx)
        .await?;
        Ok(row)
    }

    pub async fn list_fills_for_user(
        &self,
        user_id: Uuid,
        limit: i64,
    ) -> Result<Vec<PaperFillRow>, StorageError> {
        let rows = sqlx::query_as::<_, PaperFillRow>(
            r#"SELECT id, org_id, user_id, exchange, segment, symbol,
                      client_order_id, side, quantity, avg_price, fee,
                      quote_balance_after, base_positions_after, intent, created_at
               FROM paper_fills
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
}

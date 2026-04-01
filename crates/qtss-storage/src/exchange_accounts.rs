//! Borsa API anahtarları (`exchange_accounts`).

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone)]
pub struct ExchangeCredentials {
    pub api_key: String,
    pub api_secret: String,
    /// OKX and similar venues; `NULL` in DB when unused.
    pub passphrase: Option<String>,
}

pub struct ExchangeAccountRepository {
    pool: PgPool,
}

impl ExchangeAccountRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// `exchange`: `binance`, `bybit`, … (`exchange_accounts.exchange`, case-insensitive).
    /// `segment`: `spot`, `futures` vb.
    pub async fn credentials_for_user(
        &self,
        user_id: Uuid,
        exchange: &str,
        segment: &str,
    ) -> Result<Option<ExchangeCredentials>, StorageError> {
        let row: Option<(String, String, Option<String>)> = sqlx::query_as(
            r#"SELECT api_key, api_secret, passphrase FROM exchange_accounts
               WHERE user_id = $1
                 AND lower(exchange) = lower($2)
                 AND lower(segment) = lower($3)
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(exchange)
        .bind(segment)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(api_key, api_secret, passphrase)| ExchangeCredentials {
                api_key,
                api_secret,
                passphrase,
            },
        ))
    }

    /// `segment`: `spot`, `futures` vb. (`exchange_accounts.segment` ile birebir, case-insensitive).
    pub async fn binance_for_user(
        &self,
        user_id: Uuid,
        segment: &str,
    ) -> Result<Option<ExchangeCredentials>, StorageError> {
        self.credentials_for_user(user_id, "binance", segment).await
    }

    /// `segment`: `spot`, `futures` vb. — Binance hesabı olan tüm `user_id` (worker periyodik reconcile).
    pub async fn list_user_ids_binance_segment(
        &self,
        segment: &str,
    ) -> Result<Vec<Uuid>, StorageError> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            r#"SELECT DISTINCT user_id FROM exchange_accounts
               WHERE lower(exchange) = 'binance'
                 AND lower(segment) = lower($1)"#,
        )
        .bind(segment)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(u,)| u).collect())
    }
}

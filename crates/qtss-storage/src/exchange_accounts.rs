//! Borsa API anahtarları (`exchange_accounts`).

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone)]
pub struct ExchangeCredentials {
    pub api_key: String,
    pub api_secret: String,
}

pub struct ExchangeAccountRepository {
    pool: PgPool,
}

impl ExchangeAccountRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// `segment`: `spot`, `futures` vb. (`exchange_accounts.segment` ile birebir, case-insensitive).
    pub async fn binance_for_user(
        &self,
        user_id: Uuid,
        segment: &str,
    ) -> Result<Option<ExchangeCredentials>, StorageError> {
        let row: Option<(String, String)> = sqlx::query_as(
            r#"SELECT api_key, api_secret FROM exchange_accounts
               WHERE user_id = $1
                 AND lower(exchange) = 'binance'
                 AND lower(segment) = lower($2)
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(segment)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(api_key, api_secret)| ExchangeCredentials {
            api_key,
            api_secret,
        }))
    }
}

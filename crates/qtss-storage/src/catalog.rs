//! Borsa / piyasa / sembol kataloğu (DB). Connector senkronu `upsert_*` ile doldurur.

use chrono::{DateTime, Utc};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ExchangeRow {
    pub id: Uuid,
    pub code: String,
    pub display_name: String,
    pub is_active: bool,
    pub metadata: JsonValue,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MarketRow {
    pub id: Uuid,
    pub exchange_id: Uuid,
    pub segment: String,
    pub contract_kind: String,
    pub display_name: Option<String>,
    pub is_active: bool,
    pub metadata: JsonValue,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InstrumentRow {
    pub id: Uuid,
    pub market_id: Uuid,
    pub native_symbol: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub status: String,
    pub is_trading: bool,
    pub price_filter: Option<JsonValue>,
    pub lot_filter: Option<JsonValue>,
    pub metadata: JsonValue,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct CatalogRepository {
    pool: PgPool,
}

impl CatalogRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list_exchanges(&self) -> Result<Vec<ExchangeRow>, StorageError> {
        sqlx::query_as::<_, ExchangeRow>(
            r#"SELECT id, code, display_name, is_active, metadata, created_at, updated_at
               FROM exchanges ORDER BY code"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn upsert_exchange(
        &self,
        code: &str,
        display_name: &str,
        is_active: bool,
        metadata: JsonValue,
    ) -> Result<ExchangeRow, StorageError> {
        let row = sqlx::query_as::<_, ExchangeRow>(
            r#"INSERT INTO exchanges (code, display_name, is_active, metadata)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (code) DO UPDATE SET
                 display_name = EXCLUDED.display_name,
                 is_active = EXCLUDED.is_active,
                 metadata = EXCLUDED.metadata,
                 updated_at = now()
               RETURNING id, code, display_name, is_active, metadata, created_at, updated_at"#,
        )
        .bind(code)
        .bind(display_name)
        .bind(is_active)
        .bind(metadata)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn list_markets_by_exchange_code(
        &self,
        exchange_code: &str,
    ) -> Result<Vec<MarketRow>, StorageError> {
        sqlx::query_as::<_, MarketRow>(
            r#"SELECT m.id, m.exchange_id, m.segment, m.contract_kind, m.display_name,
                      m.is_active, m.metadata, m.created_at, m.updated_at
               FROM markets m
               INNER JOIN exchanges e ON e.id = m.exchange_id
               WHERE e.code = $1
               ORDER BY m.segment, m.contract_kind"#,
        )
        .bind(exchange_code)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn get_market(
        &self,
        exchange_code: &str,
        segment: &str,
        contract_kind: &str,
    ) -> Result<Option<MarketRow>, StorageError> {
        let row = sqlx::query_as::<_, MarketRow>(
            r#"SELECT m.id, m.exchange_id, m.segment, m.contract_kind, m.display_name,
                      m.is_active, m.metadata, m.created_at, m.updated_at
               FROM markets m
               INNER JOIN exchanges e ON e.id = m.exchange_id
               WHERE e.code = $1 AND m.segment = $2 AND m.contract_kind = $3"#,
        )
        .bind(exchange_code)
        .bind(segment)
        .bind(contract_kind)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn upsert_market(
        &self,
        exchange_code: &str,
        segment: &str,
        contract_kind: &str,
        display_name: Option<&str>,
        is_active: bool,
        metadata: JsonValue,
    ) -> Result<MarketRow, StorageError> {
        let row = sqlx::query_as::<_, MarketRow>(
            r#"INSERT INTO markets (exchange_id, segment, contract_kind, display_name, is_active, metadata)
               SELECT e.id, $2, $3, $4, $5, $6 FROM exchanges e WHERE e.code = $1
               ON CONFLICT (exchange_id, segment, contract_kind) DO UPDATE SET
                 display_name = EXCLUDED.display_name,
                 is_active = EXCLUDED.is_active,
                 metadata = EXCLUDED.metadata,
                 updated_at = now()
               RETURNING id, exchange_id, segment, contract_kind, display_name,
                         is_active, metadata, created_at, updated_at"#,
        )
        .bind(exchange_code)
        .bind(segment)
        .bind(contract_kind)
        .bind(display_name)
        .bind(is_active)
        .bind(metadata)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::Other("borsa kodu bulunamadı".into()))?;
        Ok(row)
    }

    pub async fn list_instruments_for_market(
        &self,
        market_id: Uuid,
        limit: i64,
    ) -> Result<Vec<InstrumentRow>, StorageError> {
        sqlx::query_as::<_, InstrumentRow>(
            r#"SELECT id, market_id, native_symbol, base_asset, quote_asset, status,
                      is_trading, price_filter, lot_filter, metadata, created_at, updated_at
               FROM instruments WHERE market_id = $1
               ORDER BY native_symbol
               LIMIT $2"#,
        )
        .bind(market_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// `exchange_code` + segment + `contract_kind` ile piyasayı çözümleyip sembolleri döner.
    pub async fn list_instruments_by_venue(
        &self,
        exchange_code: &str,
        segment: &str,
        contract_kind: &str,
        limit: i64,
    ) -> Result<Vec<InstrumentRow>, StorageError> {
        sqlx::query_as::<_, InstrumentRow>(
            r#"SELECT i.id, i.market_id, i.native_symbol, i.base_asset, i.quote_asset, i.status,
                      i.is_trading, i.price_filter, i.lot_filter, i.metadata, i.created_at, i.updated_at
               FROM instruments i
               INNER JOIN markets m ON m.id = i.market_id
               INNER JOIN exchanges e ON e.id = m.exchange_id
               WHERE e.code = $1 AND m.segment = $2 AND m.contract_kind = $3
               ORDER BY i.native_symbol
               LIMIT $4"#,
        )
        .bind(exchange_code)
        .bind(segment)
        .bind(contract_kind)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_instrument(
        &self,
        market_id: Uuid,
        native_symbol: &str,
        base_asset: &str,
        quote_asset: &str,
        status: &str,
        is_trading: bool,
        price_filter: Option<JsonValue>,
        lot_filter: Option<JsonValue>,
        metadata: JsonValue,
    ) -> Result<InstrumentRow, StorageError> {
        let row = sqlx::query_as::<_, InstrumentRow>(
            r#"INSERT INTO instruments (
                 market_id, native_symbol, base_asset, quote_asset, status,
                 is_trading, price_filter, lot_filter, metadata
               )
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
               ON CONFLICT (market_id, native_symbol) DO UPDATE SET
                 base_asset = EXCLUDED.base_asset,
                 quote_asset = EXCLUDED.quote_asset,
                 status = EXCLUDED.status,
                 is_trading = EXCLUDED.is_trading,
                 price_filter = EXCLUDED.price_filter,
                 lot_filter = EXCLUDED.lot_filter,
                 metadata = EXCLUDED.metadata,
                 updated_at = now()
               RETURNING id, market_id, native_symbol, base_asset, quote_asset, status,
                         is_trading, price_filter, lot_filter, metadata, created_at, updated_at"#,
        )
        .bind(market_id)
        .bind(native_symbol)
        .bind(base_asset)
        .bind(quote_asset)
        .bind(status)
        .bind(is_trading)
        .bind(price_filter)
        .bind(lot_filter)
        .bind(metadata)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }
}

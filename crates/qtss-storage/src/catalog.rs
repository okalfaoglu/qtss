//! Borsa / piyasa / sembol kataloğu (DB). Connector senkronu `upsert_*` ile doldurur.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ExchangeRow {
    pub id: Uuid,
    pub code: String,
    pub display_name: String,
    pub is_active: bool,
    pub metadata: JsonValue,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
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

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct BarIntervalRow {
    pub id: Uuid,
    pub code: String,
    pub label: Option<String>,
    pub duration_seconds: Option<i32>,
    pub sort_order: i32,
    pub is_active: bool,
    pub metadata: JsonValue,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
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

    pub async fn list_bar_intervals(&self) -> Result<Vec<BarIntervalRow>, StorageError> {
        sqlx::query_as::<_, BarIntervalRow>(
            r#"SELECT id, code, label, duration_seconds, sort_order, is_active, metadata, created_at, updated_at
               FROM bar_intervals ORDER BY sort_order ASC, code ASC"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn upsert_bar_interval(
        &self,
        code: &str,
        label: Option<&str>,
        duration_seconds: Option<i32>,
        sort_order: i32,
        is_active: bool,
        metadata: JsonValue,
    ) -> Result<BarIntervalRow, StorageError> {
        let row = sqlx::query_as::<_, BarIntervalRow>(
            r#"INSERT INTO bar_intervals (code, label, duration_seconds, sort_order, is_active, metadata)
               VALUES ($1, $2, $3, $4, $5, $6)
               ON CONFLICT (code) DO UPDATE SET
                 label = EXCLUDED.label,
                 duration_seconds = EXCLUDED.duration_seconds,
                 sort_order = EXCLUDED.sort_order,
                 is_active = EXCLUDED.is_active,
                 metadata = EXCLUDED.metadata,
                 updated_at = now()
               RETURNING id, code, label, duration_seconds, sort_order, is_active, metadata, created_at, updated_at"#,
        )
        .bind(code)
        .bind(label)
        .bind(duration_seconds)
        .bind(sort_order)
        .bind(is_active)
        .bind(metadata)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn delete_bar_interval(&self, id: Uuid) -> Result<u64, StorageError> {
        let r = sqlx::query("DELETE FROM bar_intervals WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected())
    }

    pub async fn update_exchange(
        &self,
        id: Uuid,
        display_name: Option<&str>,
        is_active: Option<bool>,
        metadata: Option<JsonValue>,
    ) -> Result<Option<ExchangeRow>, StorageError> {
        let row = sqlx::query_as::<_, ExchangeRow>(
            r#"UPDATE exchanges SET
                 display_name = COALESCE($2, display_name),
                 is_active = COALESCE($3, is_active),
                 metadata = COALESCE($4, metadata),
                 updated_at = now()
               WHERE id = $1
               RETURNING id, code, display_name, is_active, metadata, created_at, updated_at"#,
        )
        .bind(id)
        .bind(display_name)
        .bind(is_active)
        .bind(metadata)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn delete_exchange(&self, id: Uuid) -> Result<u64, StorageError> {
        let r = sqlx::query("DELETE FROM exchanges WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected())
    }

    pub async fn list_markets_all(&self, limit: i64) -> Result<Vec<MarketRow>, StorageError> {
        let lim = limit.clamp(1, 2000);
        sqlx::query_as::<_, MarketRow>(
            r#"SELECT id, exchange_id, segment, contract_kind, display_name, is_active, metadata, created_at, updated_at
               FROM markets ORDER BY exchange_id, segment, contract_kind LIMIT $1"#,
        )
        .bind(lim)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn update_market(
        &self,
        id: Uuid,
        display_name: Option<&str>,
        is_active: Option<bool>,
        metadata: Option<JsonValue>,
    ) -> Result<Option<MarketRow>, StorageError> {
        let row = sqlx::query_as::<_, MarketRow>(
            r#"UPDATE markets SET
                 display_name = COALESCE($2, display_name),
                 is_active = COALESCE($3, is_active),
                 metadata = COALESCE($4, metadata),
                 updated_at = now()
               WHERE id = $1
               RETURNING id, exchange_id, segment, contract_kind, display_name, is_active, metadata, created_at, updated_at"#,
        )
        .bind(id)
        .bind(display_name)
        .bind(is_active)
        .bind(metadata)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn delete_market(&self, id: Uuid) -> Result<u64, StorageError> {
        let r = sqlx::query("DELETE FROM markets WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected())
    }

    pub async fn update_instrument(
        &self,
        id: Uuid,
        base_asset: Option<&str>,
        quote_asset: Option<&str>,
        status: Option<&str>,
        is_trading: Option<bool>,
        metadata: Option<JsonValue>,
    ) -> Result<Option<InstrumentRow>, StorageError> {
        let row = sqlx::query_as::<_, InstrumentRow>(
            r#"UPDATE instruments SET
                 base_asset = COALESCE($2, base_asset),
                 quote_asset = COALESCE($3, quote_asset),
                 status = COALESCE($4, status),
                 is_trading = COALESCE($5, is_trading),
                 metadata = COALESCE($6, metadata),
                 updated_at = now()
               WHERE id = $1
               RETURNING id, market_id, native_symbol, base_asset, quote_asset, status,
                         is_trading, price_filter, lot_filter, metadata, created_at, updated_at"#,
        )
        .bind(id)
        .bind(base_asset)
        .bind(quote_asset)
        .bind(status)
        .bind(is_trading)
        .bind(metadata)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn delete_instrument(&self, id: Uuid) -> Result<u64, StorageError> {
        let r = sqlx::query("DELETE FROM instruments WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected())
    }

    pub async fn get_exchange_by_id(&self, id: Uuid) -> Result<Option<ExchangeRow>, StorageError> {
        let row = sqlx::query_as::<_, ExchangeRow>(
            r#"SELECT id, code, display_name, is_active, metadata, created_at, updated_at
               FROM exchanges WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn get_market_by_id(&self, id: Uuid) -> Result<Option<MarketRow>, StorageError> {
        let row = sqlx::query_as::<_, MarketRow>(
            r#"SELECT id, exchange_id, segment, contract_kind, display_name, is_active, metadata, created_at, updated_at
               FROM markets WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn get_instrument_by_id(
        &self,
        id: Uuid,
    ) -> Result<Option<InstrumentRow>, StorageError> {
        let row = sqlx::query_as::<_, InstrumentRow>(
            r#"SELECT id, market_id, native_symbol, base_asset, quote_asset, status,
                      is_trading, price_filter, lot_filter, metadata, created_at, updated_at
               FROM instruments WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }
}

/// `engine_symbols` / `market_bars` metin alanlarından katalog FK çözümü (yoksa None).
#[derive(Debug, Clone, Default)]
pub struct SeriesCatalogIds {
    pub exchange_id: Option<Uuid>,
    pub market_id: Option<Uuid>,
    pub instrument_id: Option<Uuid>,
    pub bar_interval_id: Option<Uuid>,
}

fn catalog_segment_parts(segment: &str) -> (&'static str, &'static str) {
    let s = segment.trim().to_lowercase();
    match s.as_str() {
        "futures" | "usdt_futures" | "fapi" => ("futures", "usdt_m"),
        _ => ("spot", ""),
    }
}

pub async fn resolve_series_catalog_ids(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    interval: &str,
) -> Result<SeriesCatalogIds, StorageError> {
    let ex_code = exchange.trim().to_lowercase();
    if ex_code.is_empty() {
        return Ok(SeriesCatalogIds::default());
    }
    let (m_seg, m_ck) = catalog_segment_parts(segment);
    let sym = symbol.trim().to_uppercase();
    let iv_code = interval.trim();
    if sym.is_empty() || iv_code.is_empty() {
        return Ok(SeriesCatalogIds::default());
    }

    let exchange_id: Option<Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM exchanges WHERE LOWER(TRIM(code)) = LOWER(TRIM($1)) LIMIT 1"#,
    )
    .bind(&ex_code)
    .fetch_optional(pool)
    .await?;

    let Some(eid) = exchange_id else {
        return Ok(SeriesCatalogIds::default());
    };

    let market_id: Option<Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM markets
           WHERE exchange_id = $1 AND segment = $2 AND contract_kind = $3
           LIMIT 1"#,
    )
    .bind(eid)
    .bind(m_seg)
    .bind(m_ck)
    .fetch_optional(pool)
    .await?;

    let Some(mid) = market_id else {
        return Ok(SeriesCatalogIds {
            exchange_id: Some(eid),
            ..Default::default()
        });
    };

    let instrument_id: Option<Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM instruments
           WHERE market_id = $1 AND UPPER(TRIM(native_symbol)) = UPPER(TRIM($2))
           LIMIT 1"#,
    )
    .bind(mid)
    .bind(&sym)
    .fetch_optional(pool)
    .await?;

    let bar_interval_id: Option<Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM bar_intervals
           WHERE LOWER(TRIM(code)) = LOWER(TRIM($1))
           LIMIT 1"#,
    )
    .bind(iv_code)
    .fetch_optional(pool)
    .await?;

    Ok(SeriesCatalogIds {
        exchange_id: Some(eid),
        market_id: Some(mid),
        instrument_id,
        bar_interval_id,
    })
}

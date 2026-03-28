//! `engine_symbols` + `analysis_snapshots` — arka plan analiz motorları (Trading Range, ileride ACP/Elliott).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct EngineSymbolRow {
    pub id: Uuid,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
    pub enabled: bool,
    pub sort_order: i32,
    pub label: Option<String>,
    /// `both` | `long_only` | `short_only` | `auto_segment`
    pub signal_direction_mode: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct EngineSymbolInsert {
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
    pub label: Option<String>,
    pub signal_direction_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AnalysisSnapshotJoinedRow {
    pub engine_symbol_id: Uuid,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
    pub engine_kind: String,
    pub payload: JsonValue,
    pub last_bar_open_time: Option<DateTime<Utc>>,
    pub bar_count: Option<i32>,
    pub computed_at: DateTime<Utc>,
    pub error: Option<String>,
}

pub async fn list_enabled_engine_symbols(pool: &PgPool) -> Result<Vec<EngineSymbolRow>, StorageError> {
    let rows = sqlx::query_as::<_, EngineSymbolRow>(
        r#"SELECT id, exchange, segment, symbol, interval, enabled, sort_order, label, signal_direction_mode, created_at, updated_at
           FROM engine_symbols
           WHERE enabled = true
           ORDER BY sort_order ASC, symbol ASC, interval ASC"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_engine_symbols_all(pool: &PgPool) -> Result<Vec<EngineSymbolRow>, StorageError> {
    let rows = sqlx::query_as::<_, EngineSymbolRow>(
        r#"SELECT id, exchange, segment, symbol, interval, enabled, sort_order, label, signal_direction_mode, created_at, updated_at
           FROM engine_symbols
           ORDER BY sort_order ASC, symbol ASC, interval ASC"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn insert_engine_symbol(pool: &PgPool, row: &EngineSymbolInsert) -> Result<EngineSymbolRow, StorageError> {
    let rec = sqlx::query_as::<_, EngineSymbolRow>(
        r#"INSERT INTO engine_symbols (exchange, segment, symbol, interval, label, signal_direction_mode)
           VALUES ($1, $2, $3, $4, $5, COALESCE($6, 'auto_segment'))
           ON CONFLICT (exchange, segment, symbol, interval) DO UPDATE SET
             updated_at = now(),
             label = COALESCE(EXCLUDED.label, engine_symbols.label),
             signal_direction_mode = COALESCE(EXCLUDED.signal_direction_mode, engine_symbols.signal_direction_mode)
           RETURNING id, exchange, segment, symbol, interval, enabled, sort_order, label, signal_direction_mode, created_at, updated_at"#,
    )
    .bind(&row.exchange)
    .bind(&row.segment)
    .bind(&row.symbol.to_uppercase())
    .bind(&row.interval)
    .bind(&row.label)
    .bind(&row.signal_direction_mode)
    .fetch_one(pool)
    .await?;
    Ok(rec)
}

pub async fn upsert_analysis_snapshot(
    pool: &PgPool,
    engine_symbol_id: Uuid,
    engine_kind: &str,
    payload: &JsonValue,
    last_bar_open_time: Option<DateTime<Utc>>,
    bar_count: Option<i32>,
    error: Option<&str>,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"INSERT INTO analysis_snapshots (
               engine_symbol_id, engine_kind, payload, last_bar_open_time, bar_count, computed_at, error
           ) VALUES ($1, $2, $3, $4, $5, now(), $6)
           ON CONFLICT (engine_symbol_id, engine_kind) DO UPDATE SET
             payload = EXCLUDED.payload,
             last_bar_open_time = EXCLUDED.last_bar_open_time,
             bar_count = EXCLUDED.bar_count,
             computed_at = now(),
             error = EXCLUDED.error"#,
    )
    .bind(engine_symbol_id)
    .bind(engine_kind)
    .bind(payload)
    .bind(last_bar_open_time)
    .bind(bar_count)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mevcut `analysis_snapshots` satırının `payload` alanı (yoksa `None`).
pub async fn fetch_analysis_snapshot_payload(
    pool: &PgPool,
    engine_symbol_id: Uuid,
    engine_kind: &str,
) -> Result<Option<JsonValue>, StorageError> {
    let row = sqlx::query_scalar::<_, JsonValue>(
        r#"SELECT payload FROM analysis_snapshots WHERE engine_symbol_id = $1 AND engine_kind = $2"#,
    )
    .bind(engine_symbol_id)
    .bind(engine_kind)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn update_engine_symbol_enabled(
    pool: &PgPool,
    id: Uuid,
    enabled: bool,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE engine_symbols SET enabled = $2, updated_at = now() WHERE id = $1"#,
    )
    .bind(id)
    .bind(enabled)
    .execute(pool)
    .await?;
    Ok(())
}

/// `enabled` ve/veya `signal_direction_mode` — `None` olan alanlar değiştirilmez.
pub async fn update_engine_symbol_patch(
    pool: &PgPool,
    id: Uuid,
    enabled: Option<bool>,
    signal_direction_mode: Option<&str>,
) -> Result<u64, StorageError> {
    let res = sqlx::query(
        r#"UPDATE engine_symbols SET
             enabled = COALESCE($2, enabled),
             signal_direction_mode = COALESCE($3, signal_direction_mode),
             updated_at = now()
           WHERE id = $1"#,
    )
    .bind(id)
    .bind(enabled)
    .bind(signal_direction_mode)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

pub async fn list_analysis_snapshots_with_symbols(pool: &PgPool) -> Result<Vec<AnalysisSnapshotJoinedRow>, StorageError> {
    let rows = sqlx::query_as::<_, AnalysisSnapshotJoinedRow>(
        r#"SELECT
             s.engine_symbol_id,
             e.exchange,
             e.segment,
             e.symbol,
             e.interval,
             s.engine_kind,
             s.payload,
             s.last_bar_open_time,
             s.bar_count,
             s.computed_at,
             s.error
           FROM analysis_snapshots s
           INNER JOIN engine_symbols e ON e.id = s.engine_symbol_id
           ORDER BY e.sort_order, e.symbol, e.interval, s.engine_kind"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// --- range_signal_events (F1) ---

#[derive(Debug, Clone)]
pub struct RangeSignalEventInsert {
    pub engine_symbol_id: Uuid,
    pub event_kind: String,
    pub bar_open_time: DateTime<Utc>,
    pub reference_price: Option<f64>,
    pub source: String,
    pub payload: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RangeSignalEventJoinedRow {
    pub id: Uuid,
    pub engine_symbol_id: Uuid,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
    pub event_kind: String,
    pub bar_open_time: DateTime<Utc>,
    pub reference_price: Option<f64>,
    pub source: String,
    pub payload: JsonValue,
    pub created_at: DateTime<Utc>,
}

/// Aynı (hedef, tür, bar) tekrar yazılmaz (`ON CONFLICT DO NOTHING`).
pub async fn insert_range_signal_event(
    pool: &PgPool,
    row: &RangeSignalEventInsert,
) -> Result<Option<Uuid>, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO range_signal_events (
               engine_symbol_id, event_kind, bar_open_time, reference_price, source, payload
           ) VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (engine_symbol_id, event_kind, bar_open_time) DO NOTHING
           RETURNING id"#,
    )
    .bind(row.engine_symbol_id)
    .bind(&row.event_kind)
    .bind(row.bar_open_time)
    .bind(row.reference_price)
    .bind(&row.source)
    .bind(&row.payload)
    .fetch_optional(pool)
    .await?;
    Ok(id)
}

pub async fn list_range_signal_events_joined(
    pool: &PgPool,
    engine_symbol_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<RangeSignalEventJoinedRow>, StorageError> {
    let lim = limit.clamp(1, 500);
    let rows = sqlx::query_as::<_, RangeSignalEventJoinedRow>(
        r#"SELECT
             r.id,
             r.engine_symbol_id,
             e.exchange,
             e.segment,
             e.symbol,
             e.interval,
             r.event_kind,
             r.bar_open_time,
             r.reference_price,
             r.source,
             r.payload,
             r.created_at
           FROM range_signal_events r
           INNER JOIN engine_symbols e ON e.id = r.engine_symbol_id
           WHERE ($1::uuid IS NULL OR r.engine_symbol_id = $1)
           ORDER BY r.bar_open_time DESC, r.created_at DESC
           LIMIT $2"#,
    )
    .bind(engine_symbol_id)
    .bind(lim)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

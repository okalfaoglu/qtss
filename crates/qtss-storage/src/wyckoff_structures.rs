//! `wyckoff_structures` CRUD — persistent Wyckoff structure tracking (Faz 10).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WyckoffStructureRow {
    pub id: Uuid,
    pub symbol: String,
    pub interval: String,
    pub exchange: String,
    pub segment: String,
    pub schematic: String,
    pub current_phase: String,
    pub range_top: Option<f64>,
    pub range_bottom: Option<f64>,
    pub creek_level: Option<f64>,
    pub ice_level: Option<f64>,
    pub slope_deg: Option<f64>,
    pub confidence: Option<f64>,
    pub events_json: Json,
    pub volume_profile: Option<Json>,
    pub is_active: bool,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub failed_at: Option<DateTime<Utc>>,
    pub failure_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct WyckoffStructureInsert<'a> {
    pub symbol: &'a str,
    pub interval: &'a str,
    pub exchange: &'a str,
    pub segment: &'a str,
    pub schematic: &'a str,
    pub current_phase: &'a str,
    pub range_top: f64,
    pub range_bottom: f64,
    pub creek_level: Option<f64>,
    pub ice_level: Option<f64>,
    pub events_json: Json,
    pub confidence: f64,
}

/// Insert a new active structure.
pub async fn insert_wyckoff_structure(
    pool: &PgPool,
    ins: &WyckoffStructureInsert<'_>,
) -> Result<Uuid, StorageError> {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO wyckoff_structures
               (id, symbol, interval, exchange, segment, schematic, current_phase,
                range_top, range_bottom, creek_level, ice_level, events_json, confidence)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)"#,
    )
    .bind(id)
    .bind(ins.symbol)
    .bind(ins.interval)
    .bind(ins.exchange)
    .bind(ins.segment)
    .bind(ins.schematic)
    .bind(ins.current_phase)
    .bind(ins.range_top)
    .bind(ins.range_bottom)
    .bind(ins.creek_level)
    .bind(ins.ice_level)
    .bind(&ins.events_json)
    .bind(ins.confidence)
    .execute(pool)
    .await?;
    Ok(id)
}

/// Update an existing structure (phase progression, new events, levels).
pub async fn update_wyckoff_structure(
    pool: &PgPool,
    id: Uuid,
    phase: &str,
    schematic: &str,
    range_top: f64,
    range_bottom: f64,
    creek: Option<f64>,
    ice: Option<f64>,
    events_json: &Json,
    confidence: f64,
) -> Result<u64, StorageError> {
    let res = sqlx::query(
        r#"UPDATE wyckoff_structures
              SET current_phase = $2,
                  schematic     = $3,
                  range_top     = $4,
                  range_bottom  = $5,
                  creek_level   = $6,
                  ice_level     = $7,
                  events_json   = $8,
                  confidence    = $9,
                  updated_at    = NOW()
            WHERE id = $1"#,
    )
    .bind(id)
    .bind(phase)
    .bind(schematic)
    .bind(range_top)
    .bind(range_bottom)
    .bind(creek)
    .bind(ice)
    .bind(events_json)
    .bind(confidence)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Mark a structure as completed (Phase E reached).
pub async fn complete_wyckoff_structure(pool: &PgPool, id: Uuid) -> Result<u64, StorageError> {
    let res = sqlx::query(
        r#"UPDATE wyckoff_structures
              SET is_active = false, completed_at = NOW(), current_phase = 'E', updated_at = NOW()
            WHERE id = $1"#,
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Mark a structure as failed.
pub async fn fail_wyckoff_structure(
    pool: &PgPool,
    id: Uuid,
    reason: &str,
) -> Result<u64, StorageError> {
    let res = sqlx::query(
        r#"UPDATE wyckoff_structures
              SET is_active = false, failed_at = NOW(), failure_reason = $2, updated_at = NOW()
            WHERE id = $1"#,
    )
    .bind(id)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// List all active structures, optionally filtered.
pub async fn list_active_wyckoff_structures(
    pool: &PgPool,
    symbol: Option<&str>,
    interval: Option<&str>,
) -> Result<Vec<WyckoffStructureRow>, StorageError> {
    let rows = sqlx::query_as::<_, WyckoffStructureRow>(
        r#"SELECT id, symbol, interval, exchange, segment, schematic, current_phase,
                  range_top, range_bottom, creek_level, ice_level, slope_deg,
                  confidence, events_json, volume_profile, is_active,
                  started_at, completed_at, failed_at, failure_reason,
                  created_at, updated_at
             FROM wyckoff_structures
            WHERE is_active = true
              AND ($1::text IS NULL OR symbol = $1)
              AND ($2::text IS NULL OR interval = $2)
            ORDER BY started_at DESC
            LIMIT 100"#,
    )
    .bind(symbol)
    .bind(interval)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Get a single structure by id.
pub async fn get_wyckoff_structure(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<WyckoffStructureRow>, StorageError> {
    let row = sqlx::query_as::<_, WyckoffStructureRow>(
        r#"SELECT id, symbol, interval, exchange, segment, schematic, current_phase,
                  range_top, range_bottom, creek_level, ice_level, slope_deg,
                  confidence, events_json, volume_profile, is_active,
                  started_at, completed_at, failed_at, failure_reason,
                  created_at, updated_at
             FROM wyckoff_structures
            WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// History for a symbol (active + completed + failed), newest first.
pub async fn list_wyckoff_history(
    pool: &PgPool,
    symbol: &str,
    limit: i64,
) -> Result<Vec<WyckoffStructureRow>, StorageError> {
    let rows = sqlx::query_as::<_, WyckoffStructureRow>(
        r#"SELECT id, symbol, interval, exchange, segment, schematic, current_phase,
                  range_top, range_bottom, creek_level, ice_level, slope_deg,
                  confidence, events_json, volume_profile, is_active,
                  started_at, completed_at, failed_at, failure_reason,
                  created_at, updated_at
             FROM wyckoff_structures
            WHERE symbol = $1
            ORDER BY started_at DESC
            LIMIT $2"#,
    )
    .bind(symbol)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Find the active structure for a given (symbol, interval) — at most one.
pub async fn find_active_wyckoff_structure(
    pool: &PgPool,
    symbol: &str,
    interval: &str,
) -> Result<Option<WyckoffStructureRow>, StorageError> {
    let row = sqlx::query_as::<_, WyckoffStructureRow>(
        r#"SELECT id, symbol, interval, exchange, segment, schematic, current_phase,
                  range_top, range_bottom, creek_level, ice_level, slope_deg,
                  confidence, events_json, volume_profile, is_active,
                  started_at, completed_at, failed_at, failure_reason,
                  created_at, updated_at
             FROM wyckoff_structures
            WHERE symbol = $1 AND interval = $2 AND is_active = true
            ORDER BY started_at DESC
            LIMIT 1"#,
    )
    .bind(symbol)
    .bind(interval)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

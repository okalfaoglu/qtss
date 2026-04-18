//! Faz 9.8.11 — `selected_candidates` repo.
//!
//! Bridge table between the setup engine and the execution bridge.
//! The selector worker reads armed `qtss_v2_setups` rows, applies the
//! filter registry (risk caps, cooldowns, liquidation distance, …)
//! and writes one row here per *approved* candidate. The execution
//! bridge worker then claims pending rows with
//! `FOR UPDATE SKIP LOCKED`, dispatches the order through
//! `ExecutionManager.place_on(mode)`, and marks the row `placed`
//! or `errored`.
//!
//! CLAUDE.md #3 — qtss-storage stays domain-free: DTO in/DTO out.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone)]
pub struct InsertSelectedCandidate {
    pub setup_id: Uuid,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub direction: &'static str, // 'long' | 'short'
    pub entry_price: Decimal,
    pub sl_price: Decimal,
    pub tp_ladder: JsonValue,
    pub risk_pct: Decimal,
    pub mode: &'static str, // 'dry' | 'live' | 'backtest'
    pub selector_score: Option<Decimal>,
    pub selector_meta: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SelectedCandidateRow {
    pub id: i64,
    pub setup_id: Uuid,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub direction: String,
    pub entry_price: Decimal,
    pub sl_price: Decimal,
    pub tp_ladder: JsonValue,
    pub risk_pct: Decimal,
    pub mode: String,
    pub status: String,
    pub reject_reason: Option<String>,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub selector_score: Option<Decimal>,
    pub selector_meta: JsonValue,
    pub created_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub placed_at: Option<DateTime<Utc>>,
}

/// Insert a new candidate. Returns `Ok(Some(id))` when inserted,
/// `Ok(None)` if a row for the same `(setup_id, mode)` already existed
/// (unique constraint collision — idempotent for the selector loop).
pub async fn insert(
    pool: &PgPool,
    c: &InsertSelectedCandidate,
) -> Result<Option<i64>, StorageError> {
    let row: Option<(i64,)> = sqlx::query_as(
        r#"
        INSERT INTO selected_candidates (
            setup_id, exchange, symbol, timeframe, direction,
            entry_price, sl_price, tp_ladder, risk_pct, mode,
            selector_score, selector_meta
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
        ON CONFLICT (setup_id, mode) DO NOTHING
        RETURNING id
        "#,
    )
    .bind(c.setup_id)
    .bind(&c.exchange)
    .bind(&c.symbol)
    .bind(&c.timeframe)
    .bind(c.direction)
    .bind(c.entry_price)
    .bind(c.sl_price)
    .bind(&c.tp_ladder)
    .bind(c.risk_pct)
    .bind(c.mode)
    .bind(c.selector_score)
    .bind(&c.selector_meta)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.0))
}

/// Atomic claim: pop at most `limit` pending rows, mark them
/// `claimed`, and hand back the full row data for dispatch. Uses
/// `FOR UPDATE SKIP LOCKED` so multiple execution workers can safely
/// run side by side.
pub async fn claim_pending(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<SelectedCandidateRow>, StorageError> {
    let rows = sqlx::query_as::<_, SelectedCandidateRow>(
        r#"
        WITH picked AS (
            SELECT id FROM selected_candidates
             WHERE status = 'pending'
             ORDER BY created_at ASC
             FOR UPDATE SKIP LOCKED
             LIMIT $1
        )
        UPDATE selected_candidates c
           SET status = 'claimed',
               claimed_at = now(),
               attempts = c.attempts + 1
          FROM picked
         WHERE c.id = picked.id
         RETURNING c.id, c.setup_id, c.exchange, c.symbol, c.timeframe,
                   c.direction, c.entry_price, c.sl_price, c.tp_ladder,
                   c.risk_pct, c.mode, c.status, c.reject_reason,
                   c.attempts, c.last_error, c.selector_score,
                   c.selector_meta, c.created_at, c.claimed_at, c.placed_at
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn mark_placed(pool: &PgPool, id: i64) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE selected_candidates
              SET status = 'placed', placed_at = now(), last_error = NULL
            WHERE id = $1"#,
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_errored(pool: &PgPool, id: i64, err: &str) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE selected_candidates
              SET status = 'errored', last_error = $2
            WHERE id = $1"#,
    )
    .bind(id)
    .bind(err)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_rejected(pool: &PgPool, id: i64, reason: &str) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE selected_candidates
              SET status = 'rejected', reject_reason = $2
            WHERE id = $1"#,
    )
    .bind(id)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(())
}

/// Setup IDs that already have a candidate row in the given mode.
/// Used by the selector loop to stay idempotent without relying on
/// the unique-index error path.
pub async fn existing_setup_ids(
    pool: &PgPool,
    setup_ids: &[Uuid],
    mode: &str,
) -> Result<Vec<Uuid>, StorageError> {
    if setup_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        r#"SELECT setup_id FROM selected_candidates
            WHERE setup_id = ANY($1) AND mode = $2"#,
    )
    .bind(setup_ids)
    .bind(mode)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn list_recent(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<SelectedCandidateRow>, StorageError> {
    let rows = sqlx::query_as::<_, SelectedCandidateRow>(
        r#"SELECT id, setup_id, exchange, symbol, timeframe, direction,
                  entry_price, sl_price, tp_ladder, risk_pct, mode,
                  status, reject_reason, attempts, last_error,
                  selector_score, selector_meta,
                  created_at, claimed_at, placed_at
             FROM selected_candidates
            ORDER BY created_at DESC
            LIMIT $1"#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

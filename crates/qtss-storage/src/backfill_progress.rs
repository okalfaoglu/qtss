//! `backfill_progress` — tracks per-series backfill state so the worker
//! can resume after crashes and verify completeness before analysis.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct BackfillProgressRow {
    pub id: Uuid,
    pub engine_symbol_id: Uuid,
    pub state: String,
    pub oldest_fetched: Option<DateTime<Utc>>,
    pub newest_fetched: Option<DateTime<Utc>>,
    pub bar_count: i64,
    pub expected_bars: Option<i64>,
    pub gap_count: i32,
    pub max_gap_seconds: Option<i32>,
    pub backfill_started_at: Option<DateTime<Utc>>,
    pub backfill_finished_at: Option<DateTime<Utc>>,
    pub verified_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub pages_fetched: i32,
    pub bars_upserted: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Get or create progress row for an engine_symbol.
pub async fn get_or_create_backfill_progress(
    pool: &PgPool,
    engine_symbol_id: Uuid,
) -> Result<BackfillProgressRow, StorageError> {
    let row = sqlx::query_as::<_, BackfillProgressRow>(
        r#"INSERT INTO backfill_progress (engine_symbol_id, state)
           VALUES ($1, 'pending')
           ON CONFLICT (engine_symbol_id) DO NOTHING
           RETURNING *"#,
    )
    .bind(engine_symbol_id)
    .fetch_optional(pool)
    .await?;

    if let Some(r) = row {
        return Ok(r);
    }

    // Already exists — fetch it
    sqlx::query_as::<_, BackfillProgressRow>(
        "SELECT * FROM backfill_progress WHERE engine_symbol_id = $1",
    )
    .bind(engine_symbol_id)
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

/// Mark backfill as started (state → backfilling).
pub async fn mark_backfill_started(
    pool: &PgPool,
    engine_symbol_id: Uuid,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE backfill_progress
           SET state = 'backfilling',
               backfill_started_at = COALESCE(backfill_started_at, now()),
               last_error = NULL,
               updated_at = now()
           WHERE engine_symbol_id = $1"#,
    )
    .bind(engine_symbol_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Update progress cursor after each page fetch.
pub async fn update_backfill_cursor(
    pool: &PgPool,
    engine_symbol_id: Uuid,
    oldest_fetched: DateTime<Utc>,
    newest_fetched: Option<DateTime<Utc>>,
    pages_fetched: i32,
    bars_upserted: i64,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE backfill_progress
           SET oldest_fetched = $2,
               newest_fetched = COALESCE($3, newest_fetched),
               pages_fetched = $4,
               bars_upserted = $5,
               updated_at = now()
           WHERE engine_symbol_id = $1"#,
    )
    .bind(engine_symbol_id)
    .bind(oldest_fetched)
    .bind(newest_fetched)
    .bind(pages_fetched)
    .bind(bars_upserted)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mark backfill finished, move to verifying.
pub async fn mark_backfill_finished(
    pool: &PgPool,
    engine_symbol_id: Uuid,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE backfill_progress
           SET state = 'verifying',
               backfill_finished_at = now(),
               updated_at = now()
           WHERE engine_symbol_id = $1"#,
    )
    .bind(engine_symbol_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Update verification results and set state to complete or back to backfilling.
pub async fn update_verification(
    pool: &PgPool,
    engine_symbol_id: Uuid,
    bar_count: i64,
    expected_bars: i64,
    gap_count: i32,
    max_gap_seconds: Option<i32>,
    is_complete: bool,
) -> Result<(), StorageError> {
    let new_state = if is_complete { "complete" } else { "backfilling" };
    sqlx::query(
        r#"UPDATE backfill_progress
           SET state = $2,
               bar_count = $3,
               expected_bars = $4,
               gap_count = $5,
               max_gap_seconds = $6,
               verified_at = CASE WHEN $7 THEN now() ELSE verified_at END,
               updated_at = now()
           WHERE engine_symbol_id = $1"#,
    )
    .bind(engine_symbol_id)
    .bind(new_state)
    .bind(bar_count)
    .bind(expected_bars)
    .bind(gap_count)
    .bind(max_gap_seconds)
    .bind(is_complete)
    .execute(pool)
    .await?;
    Ok(())
}

/// Promote to live (complete + real-time active).
pub async fn mark_live(
    pool: &PgPool,
    engine_symbol_id: Uuid,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE backfill_progress
           SET state = 'live', updated_at = now()
           WHERE engine_symbol_id = $1 AND state = 'complete'"#,
    )
    .bind(engine_symbol_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Returns `true` when the series has finished backfill (state = complete | live).
/// Downstream loops (detection, pivot, analysis) must call this before processing
/// a symbol/interval so they never run on incomplete data.
pub async fn is_backfill_ready(pool: &PgPool, engine_symbol_id: Uuid) -> bool {
    let state: Option<String> = sqlx::query_scalar(
        "SELECT state FROM backfill_progress WHERE engine_symbol_id = $1",
    )
    .bind(engine_symbol_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    matches!(state.as_deref(), Some("complete" | "live"))
}

/// Record an error without changing state.
pub async fn record_backfill_error(
    pool: &PgPool,
    engine_symbol_id: Uuid,
    error: &str,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE backfill_progress
           SET last_error = $2, updated_at = now()
           WHERE engine_symbol_id = $1"#,
    )
    .bind(engine_symbol_id)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

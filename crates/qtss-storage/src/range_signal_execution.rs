//! `range_signal_events` → optional worker paper execution (idempotency + status).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

/// Row from `range_signal_events` joined with `engine_symbols`, excluding already-executed events.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RangeSignalEventPendingExecutionRow {
    pub id: Uuid,
    pub engine_symbol_id: Uuid,
    pub event_kind: String,
    pub bar_open_time: DateTime<Utc>,
    pub reference_price: Option<f64>,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
}

/// List recent range events that have no `range_signal_paper_executions` row yet.
pub async fn list_range_signal_events_pending_paper_execution(
    pool: &PgPool,
    max_age_hours: i64,
    limit: i64,
) -> Result<Vec<RangeSignalEventPendingExecutionRow>, StorageError> {
    let lim = limit.clamp(1, 200);
    let hours: i64 = max_age_hours.clamp(1, 168);
    let rows = sqlx::query_as::<_, RangeSignalEventPendingExecutionRow>(
        r#"SELECT
             r.id,
             r.engine_symbol_id,
             r.event_kind,
             r.bar_open_time,
             r.reference_price,
             e.exchange,
             e.segment,
             e.symbol,
             e.interval
           FROM range_signal_events r
           INNER JOIN engine_symbols e ON e.id = r.engine_symbol_id
           WHERE NOT EXISTS (
             SELECT 1 FROM range_signal_paper_executions x WHERE x.range_signal_event_id = r.id
           )
           AND r.event_kind IN ('long_entry', 'short_entry', 'long_exit', 'short_exit')
           AND r.created_at > (now() - ($1::bigint * interval '1 hour'))
           ORDER BY r.created_at ASC
           LIMIT $2"#,
    )
    .bind(hours)
    .bind(lim)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Claim one event for processing (`processing`). Returns `true` if this worker won the row.
pub async fn try_claim_range_signal_event_for_paper_execution(
    pool: &PgPool,
    range_signal_event_id: Uuid,
) -> Result<bool, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO range_signal_paper_executions (
               range_signal_event_id, status
           ) VALUES ($1, 'processing')
           ON CONFLICT (range_signal_event_id) DO NOTHING
           RETURNING range_signal_event_id"#,
    )
    .bind(range_signal_event_id)
    .fetch_optional(pool)
    .await?;
    Ok(id.is_some())
}

pub async fn update_range_signal_paper_execution_status(
    pool: &PgPool,
    range_signal_event_id: Uuid,
    status: &str,
    client_order_id: Option<Uuid>,
    error_message: Option<&str>,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE range_signal_paper_executions SET
             status = $2,
             client_order_id = COALESCE($3, client_order_id),
             error_message = $4,
             updated_at = now()
           WHERE range_signal_event_id = $1"#,
    )
    .bind(range_signal_event_id)
    .bind(status)
    .bind(client_order_id)
    .bind(error_message)
    .execute(pool)
    .await?;
    Ok(())
}

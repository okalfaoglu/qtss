//! Faz 9.7.7 — Per-user daily digest queries.
//!
//! Two concerns:
//!   * Scan: which users are due for a digest *right now* (their local
//!     clock crossed the configured digest hour and the last-sent
//!     stamp is older than their local midnight).
//!   * Aggregate: roll up the last N hours of lifecycle events into a
//!     single digest payload.
//!
//! Per-user setup filtering (subscription / symbol list) is deliberately
//! kept out of scope — the digest is currently a market-wide summary.
//! Filters will plug in via `telegram_filters` JSONB in a later patch.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DigestUserRow {
    pub user_id: Uuid,
    pub tz_offset_minutes: i32,
    pub telegram_chat_id: Option<String>,
    pub last_digest_sent_utc: Option<DateTime<Utc>>,
}

/// Users with `notify_daily_digest=true`, `telegram_enabled=true`,
/// a non-null chat id, and (`last_digest_sent_utc` is null OR older
/// than `now_utc` minus `min_gap_hours`). Fine-grained "local hour
/// reached" gating is done in memory because it depends on the
/// config-driven `digest.local_hour` knob.
pub async fn list_digest_candidates(
    pool: &PgPool,
    min_gap_hours: i32,
) -> Result<Vec<DigestUserRow>, StorageError> {
    let rows = sqlx::query_as::<_, DigestUserRow>(
        r#"
        SELECT u.id AS user_id,
               u.tz_offset_minutes,
               p.telegram_chat_id,
               p.last_digest_sent_utc
          FROM users u
          JOIN notify_delivery_prefs p ON p.user_id = u.id
         WHERE p.notify_daily_digest = true
           AND p.telegram_enabled = true
           AND p.telegram_chat_id IS NOT NULL
           AND ( p.last_digest_sent_utc IS NULL
                 OR p.last_digest_sent_utc < NOW() - ($1 || ' hours')::interval )
        "#,
    )
    .bind(min_gap_hours.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn stamp_digest_sent(
    pool: &PgPool,
    user_id: Uuid,
    at: DateTime<Utc>,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE notify_delivery_prefs
              SET last_digest_sent_utc = $2, updated_at = NOW()
            WHERE user_id = $1"#,
    )
    .bind(user_id)
    .bind(at)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DigestAggregate {
    pub window_start_utc: DateTime<Utc>,
    pub window_end_utc: DateTime<Utc>,
    pub opened: i64,
    pub closed: i64,
    pub tp_final: i64,
    pub sl_hit: i64,
    pub invalidated: i64,
    pub cancelled: i64,
    /// Sum of `pnl_pct` over closed events (simple sum, not compounded).
    pub total_pnl_pct: f64,
    /// Average health score of still-open setups in the window.
    pub avg_open_health: Option<f64>,
}

/// Build the digest aggregate for the given UTC window. The query is
/// deliberately a handful of cheap aggregates — designed to run in a
/// single round-trip.
pub async fn aggregate_digest(
    pool: &PgPool,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<DigestAggregate, StorageError> {
    let row: (i64, i64, i64, i64, i64, i64, f64) = sqlx::query_as(
        r#"
        SELECT
          COUNT(*) FILTER (WHERE s.created_at >= $1 AND s.created_at < $2)                AS opened,
          COUNT(*) FILTER (WHERE s.closed_at  >= $1 AND s.closed_at  < $2)                AS closed,
          COUNT(*) FILTER (WHERE s.close_reason = 'tp_final'     AND s.closed_at >= $1
                              AND s.closed_at < $2)                                        AS tp_final,
          COUNT(*) FILTER (WHERE s.close_reason = 'sl_hit'       AND s.closed_at >= $1
                              AND s.closed_at < $2)                                        AS sl_hit,
          COUNT(*) FILTER (WHERE s.close_reason = 'invalidated'  AND s.closed_at >= $1
                              AND s.closed_at < $2)                                        AS invalidated,
          COUNT(*) FILTER (WHERE s.close_reason = 'cancelled'    AND s.closed_at >= $1
                              AND s.closed_at < $2)                                        AS cancelled,
          COALESCE(SUM(s.realized_pnl_pct) FILTER (WHERE s.closed_at >= $1
                                                       AND s.closed_at < $2), 0.0)::float8 AS total_pnl_pct
          FROM qtss_v2_setups s
        "#,
    )
    .bind(from)
    .bind(to)
    .fetch_one(pool)
    .await?;

    let avg_open_health: Option<f64> = sqlx::query_scalar(
        r#"SELECT AVG(health_score)::float8
             FROM qtss_position_health_snapshots h
             JOIN qtss_v2_setups s ON s.id = h.setup_id
            WHERE s.closed_at IS NULL
              AND h.captured_at >= $1"#,
    )
    .bind(from)
    .fetch_one(pool)
    .await
    .unwrap_or(None);

    Ok(DigestAggregate {
        window_start_utc: from,
        window_end_utc: to,
        opened: row.0,
        closed: row.1,
        tp_final: row.2,
        sl_hit: row.3,
        invalidated: row.4,
        cancelled: row.5,
        total_pnl_pct: row.6,
        avg_open_health,
    })
}

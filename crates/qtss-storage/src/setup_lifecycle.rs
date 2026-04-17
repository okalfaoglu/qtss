//! Faz 9.7.3 — Repos for `qtss_setup_lifecycle_events` (audit trail)
//! and `qtss_position_health_snapshots` (band-transition snapshots),
//! plus setup closure updates called when the watcher emits a
//! terminal event.
//!
//! Schema in migration 0135.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

// ---------------------------------------------------------------------------
// Lifecycle events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LifecycleEventInsert {
    pub setup_id: Uuid,
    pub event_kind: String,
    pub price: Decimal,
    pub pnl_pct: Option<f64>,
    pub pnl_r: Option<f64>,
    pub health_score: Option<f64>,
    pub duration_ms: Option<i64>,
    pub ai_action: Option<String>,
    pub ai_reasoning: Option<String>,
    pub ai_confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LifecycleEventRow {
    pub id: Uuid,
    pub setup_id: Uuid,
    pub event_kind: String,
    pub price: Decimal,
    pub pnl_pct: Option<f64>,
    pub pnl_r: Option<f64>,
    pub health_score: Option<f64>,
    pub duration_ms: Option<i64>,
    pub ai_action: Option<String>,
    pub ai_reasoning: Option<String>,
    pub ai_confidence: Option<f64>,
    pub notify_outbox_id: Option<Uuid>,
    pub x_outbox_id: Option<Uuid>,
    pub emitted_at: DateTime<Utc>,
}

/// Thin DB projection used by the Faz 9.7.x SetupWatcher — only the
/// columns needed to run lifecycle/health/ratchet. Keeping this
/// separate from `V2SetupRow` avoids churning dozens of callers.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WatcherSetupRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub venue_class: String,
    pub exchange: String,
    pub symbol: String,
    pub direction: String,
    pub entry_price: Option<f32>,
    pub entry_sl: Option<f32>,
    pub koruma: Option<f32>,
    pub current_sl: Option<Decimal>,
    pub raw_meta: serde_json::Value,
    pub entry_touched_at: Option<DateTime<Utc>>,
    pub tp_hits_bitmap: i32,
    pub ratchet_reference_price: Option<Decimal>,
    pub ratchet_cumulative_pct: Option<Decimal>,
    pub ratchet_last_update_at: Option<DateTime<Utc>>,
}

pub async fn list_watcher_rows(
    pool: &PgPool,
) -> Result<Vec<WatcherSetupRow>, StorageError> {
    let rows = sqlx::query_as::<_, WatcherSetupRow>(
        r#"SELECT id, created_at, venue_class, exchange, symbol, direction,
                  entry_price, entry_sl, koruma, current_sl, raw_meta,
                  entry_touched_at, tp_hits_bitmap,
                  ratchet_reference_price, ratchet_cumulative_pct, ratchet_last_update_at
             FROM qtss_v2_setups
            WHERE state IN ('armed','active') AND closed_at IS NULL"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn insert_lifecycle_event(
    pool: &PgPool,
    ev: &LifecycleEventInsert,
) -> Result<Uuid, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO qtss_setup_lifecycle_events (
            setup_id, event_kind, price, pnl_pct, pnl_r, health_score,
            duration_ms, ai_action, ai_reasoning, ai_confidence
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        RETURNING id
        "#,
    )
    .bind(ev.setup_id)
    .bind(&ev.event_kind)
    .bind(ev.price)
    .bind(ev.pnl_pct)
    .bind(ev.pnl_r)
    .bind(ev.health_score)
    .bind(ev.duration_ms)
    .bind(&ev.ai_action)
    .bind(&ev.ai_reasoning)
    .bind(ev.ai_confidence)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn list_lifecycle_events_for_setup(
    pool: &PgPool,
    setup_id: Uuid,
    limit: i64,
) -> Result<Vec<LifecycleEventRow>, StorageError> {
    let rows = sqlx::query_as::<_, LifecycleEventRow>(
        r#"SELECT id, setup_id, event_kind, price, pnl_pct, pnl_r,
                  health_score, duration_ms, ai_action, ai_reasoning,
                  ai_confidence, notify_outbox_id, x_outbox_id, emitted_at
             FROM qtss_setup_lifecycle_events
            WHERE setup_id = $1
            ORDER BY emitted_at DESC
            LIMIT $2"#,
    )
    .bind(setup_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Health snapshots
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HealthSnapshotInsert {
    pub setup_id: Uuid,
    pub health_score: f64,
    pub prev_health_score: Option<f64>,
    pub band: String,
    pub prev_band: Option<String>,
    pub momentum_score: Option<f64>,
    pub structural_score: Option<f64>,
    pub orderbook_score: Option<f64>,
    pub regime_match_score: Option<f64>,
    pub correlation_score: Option<f64>,
    pub ai_rescore: Option<f64>,
    pub price: Decimal,
}

pub async fn insert_health_snapshot(
    pool: &PgPool,
    snap: &HealthSnapshotInsert,
) -> Result<Uuid, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO qtss_position_health_snapshots (
            setup_id, health_score, prev_health_score, band, prev_band,
            momentum_score, structural_score, orderbook_score,
            regime_match_score, correlation_score, ai_rescore, price
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
        RETURNING id
        "#,
    )
    .bind(snap.setup_id)
    .bind(snap.health_score)
    .bind(snap.prev_health_score)
    .bind(&snap.band)
    .bind(&snap.prev_band)
    .bind(snap.momentum_score)
    .bind(snap.structural_score)
    .bind(snap.orderbook_score)
    .bind(snap.regime_match_score)
    .bind(snap.correlation_score)
    .bind(snap.ai_rescore)
    .bind(snap.price)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// Setup closure + lifecycle state transitions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SetupCloseUpdate {
    pub setup_id: Uuid,
    pub close_reason: String, // tp_final | sl_hit | invalidated | cancelled
    pub close_price: Decimal,
    pub realized_pnl_pct: Option<f64>,
    pub realized_r: Option<f64>,
}

pub async fn close_setup(
    pool: &PgPool,
    update: &SetupCloseUpdate,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        UPDATE qtss_v2_setups SET
            state            = 'closed',
            close_reason     = $2,
            close_price      = $3,
            realized_pnl_pct = $4,
            realized_r       = $5,
            closed_at        = NOW(),
            updated_at       = NOW()
         WHERE id = $1 AND closed_at IS NULL
        "#,
    )
    .bind(update.setup_id)
    .bind(&update.close_reason)
    .bind(update.close_price)
    .bind(update.realized_pnl_pct)
    .bind(update.realized_r)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_entry_touched(
    pool: &PgPool,
    setup_id: Uuid,
    at: DateTime<Utc>,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE qtss_v2_setups
              SET entry_touched_at = COALESCE(entry_touched_at, $2),
                  updated_at = NOW()
            WHERE id = $1"#,
    )
    .bind(setup_id)
    .bind(at)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct RatchetUpdate {
    pub setup_id: Uuid,
    pub current_sl: Option<Decimal>,        // None when only reference advanced
    pub ratchet_reference_price: Decimal,
    pub ratchet_cumulative_pct: f64,
    pub ratchet_last_update_at: DateTime<Utc>,
}

/// Persist a Poz Koruma ratchet step. If `current_sl` is `Some`, the
/// setup's live SL is updated (only raised for LONG / lowered for
/// SHORT — caller guarantees this). Reference + cumulative + last-
/// update always move.
pub async fn apply_ratchet_update(
    pool: &PgPool,
    u: &RatchetUpdate,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        UPDATE qtss_v2_setups SET
            current_sl                 = COALESCE($2, current_sl),
            koruma                     = COALESCE($2::numeric::real, koruma),
            ratchet_reference_price    = $3,
            ratchet_cumulative_pct     = $4,
            ratchet_last_update_at     = $5,
            updated_at                 = NOW()
         WHERE id = $1
        "#,
    )
    .bind(u.setup_id)
    .bind(u.current_sl)
    .bind(u.ratchet_reference_price)
    .bind(u.ratchet_cumulative_pct)
    .bind(u.ratchet_last_update_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_tp_hit_bit(
    pool: &PgPool,
    setup_id: Uuid,
    tp_index: u8,
) -> Result<(), StorageError> {
    // tp_index is 1-based (1=TP1, 2=TP2, 3=TP3).
    let bit: i32 = 1 << (tp_index.saturating_sub(1) as i32);
    sqlx::query(
        r#"UPDATE qtss_v2_setups
              SET tp_hits_bitmap = tp_hits_bitmap | $2,
                  updated_at = NOW()
            WHERE id = $1"#,
    )
    .bind(setup_id)
    .bind(bit)
    .execute(pool)
    .await?;
    Ok(())
}

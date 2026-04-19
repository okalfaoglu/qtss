//! Faz 8.0 — `qtss_setups` repo.
//!
//! One row per setup lifecycle instance. The Setup Engine inserts
//! with state='armed', updates `koruma`/state as the setup runs, and
//! stamps `close_reason`/`close_price`/`closed_at` on transition to
//! `closed`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct V2SetupRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub venue_class: String,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub profile: String,
    pub alt_type: Option<String>,
    pub state: String,
    pub direction: String,
    pub confluence_id: Option<Uuid>,
    pub entry_price: Option<f32>,
    pub entry_sl: Option<f32>,
    pub koruma: Option<f32>,
    pub target_ref: Option<f32>,
    pub risk_pct: Option<f32>,
    pub close_reason: Option<String>,
    pub close_price: Option<f32>,
    pub closed_at: Option<DateTime<Utc>>,
    pub raw_meta: JsonValue,
    /// FK to the originating detection (migration 0040). Nullable for
    /// setups created before the column existed.
    pub detection_id: Option<Uuid>,
    /// D/T/Q: realised P&L % (migration 0055).
    pub pnl_pct: Option<f32>,
    /// D/T/Q: risk mode at setup creation (migration 0055).
    pub risk_mode: Option<String>,
    /// Faz 9.3.3 — P(win) from the LightGBM inference sidecar at open.
    pub ai_score: Option<f32>,
    /// Faz 9.7.5 — true once the setup watcher has flipped this setup
    /// into trailing-stop mode (either approaching the last TP or
    /// running beyond final TP). SL then ratchets via `apply_trail_advance`.
    pub trail_mode: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct V2SetupInsert {
    pub venue_class: String,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub profile: String,
    pub alt_type: Option<String>,
    pub state: String,
    pub direction: String,
    pub confluence_id: Option<Uuid>,
    pub entry_price: Option<f32>,
    pub entry_sl: Option<f32>,
    pub koruma: Option<f32>,
    pub target_ref: Option<f32>,
    pub risk_pct: Option<f32>,
    pub raw_meta: JsonValue,
    /// Faz 9.3.3 — LightGBM P(win) resolved at setup-open time via the
    /// inference sidecar. `None` when the sidecar is disabled / unreachable
    /// / errored; shadow-only until `ai.inference.gate_enabled` flips true.
    pub ai_score: Option<f32>,
    /// Faz 9.8.AI-1 — primary detection that tipped the confluence gate.
    /// Required for the training-set view to join `qtss_features_snapshot`
    /// rows (which are keyed by `detection_id`). `None` only for legacy rows
    /// created before this field existed.
    pub detection_id: Option<Uuid>,
    /// Faz 9B backfill fix — "live" | "dry" | "backtest". Propagated from
    /// the primary detection's own `mode` so historical_progressive_scan
    /// replays produce mode='backtest' setups instead of collapsing into
    /// the column default. Required for the backfill orchestrator's
    /// plateau detection to observe actual setup growth.
    pub mode: String,
}

pub async fn insert_v2_setup(
    pool: &PgPool,
    row: &V2SetupInsert,
) -> Result<Uuid, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO qtss_setups (
            venue_class, exchange, symbol, timeframe, profile, alt_type,
            state, direction, confluence_id,
            entry_price, entry_sl, koruma, target_ref, risk_pct, raw_meta,
            ai_score, detection_id, mode
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)
        -- P14 + Faz 9B: one open setup per (exchange, symbol, timeframe,
        -- profile, mode) — mode added so live + backtest can coexist
        -- while the backfill orchestrator replays history. See
        -- migration 0171.
        ON CONFLICT (exchange, symbol, timeframe, profile, mode)
            WHERE state IN ('armed', 'active')
        DO NOTHING
        RETURNING id
        "#,
    )
    .bind(&row.venue_class)
    .bind(&row.exchange)
    .bind(&row.symbol)
    .bind(&row.timeframe)
    .bind(&row.profile)
    .bind(row.alt_type.as_deref())
    .bind(&row.state)
    .bind(&row.direction)
    .bind(row.confluence_id)
    .bind(row.entry_price)
    .bind(row.entry_sl)
    .bind(row.koruma)
    .bind(row.target_ref)
    .bind(row.risk_pct)
    .bind(&row.raw_meta)
    .bind(row.ai_score)
    .bind(row.detection_id)
    .bind(&row.mode)
    .fetch_optional(pool)
    .await?;
    match id {
        Some(id) => Ok(id),
        None => Err(StorageError::DuplicateSetup),
    }
}

/// Move a setup forward. `close_reason`/`close_price` are only set
/// on transition to `closed`; leave them `None` for ratchet-only
/// updates.
pub async fn update_v2_setup_state(
    pool: &PgPool,
    id: Uuid,
    new_state: &str,
    koruma: Option<f32>,
    close_reason: Option<&str>,
    close_price: Option<f32>,
) -> Result<(), StorageError> {
    // Faz 9.3.4 — any `closed*` variant (`closed`, `closed_win`,
    // `closed_loss`, `closed_timeout`, ...) terminates the lifecycle,
    // so all of them must stamp `closed_at`. Prior bug: only exact
    // "closed" matched, which left `closed_win`/`closed_loss` with
    // NULL closed_at → `v_qtss_training_set_closed` (WHERE closed_at
    // IS NOT NULL) was empty and the trainer saw zero rows.
    let closed_at: Option<DateTime<Utc>> = if new_state.starts_with("closed") {
        Some(Utc::now())
    } else {
        None
    };
    sqlx::query(
        r#"
        UPDATE qtss_setups
           SET state        = $2,
               koruma       = COALESCE($3, koruma),
               close_reason = COALESCE($4, close_reason),
               close_price  = COALESCE($5, close_price),
               closed_at    = COALESCE($6, closed_at),
               updated_at   = now()
         WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(new_state)
    .bind(koruma)
    .bind(close_reason)
    .bind(close_price)
    .bind(closed_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn fetch_v2_setup(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<V2SetupRow>, StorageError> {
    let row = sqlx::query_as::<_, V2SetupRow>(
        r#"SELECT id, created_at, updated_at, venue_class, exchange, symbol,
                  timeframe, profile, alt_type, state, direction, confluence_id,
                  entry_price, entry_sl, koruma, target_ref, risk_pct,
                  close_reason, close_price, closed_at, raw_meta, detection_id,
                  pnl_pct, risk_mode, ai_score, trail_mode
             FROM qtss_setups
            WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_open_v2_setups(
    pool: &PgPool,
    venue_class: Option<&str>,
) -> Result<Vec<V2SetupRow>, StorageError> {
    let rows = sqlx::query_as::<_, V2SetupRow>(
        r#"SELECT id, created_at, updated_at, venue_class, exchange, symbol,
                  timeframe, profile, alt_type, state, direction, confluence_id,
                  entry_price, entry_sl, koruma, target_ref, risk_pct,
                  close_reason, close_price, closed_at, raw_meta, detection_id,
                  pnl_pct, risk_mode, ai_score, trail_mode
             FROM qtss_setups
            WHERE state IN ('armed','active')
              AND ($1::text IS NULL OR venue_class = $1)
            ORDER BY created_at DESC"#,
    )
    .bind(venue_class)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(Debug, Clone, Default)]
pub struct SetupFilter {
    pub limit: i64,
    pub venue_class: Option<String>,
    /// If empty, defaults to open-only (`armed`,`active`). Pass explicit
    /// states (e.g. `["closed"]`) to override.
    pub states: Vec<String>,
    pub profile: Option<String>,
    /// SQL `LIKE` pattern matched against `alt_type` (e.g. `wyckoff_%`).
    pub alt_type_like: Option<String>,
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    pub mode: Option<String>,
}

pub async fn list_v2_setups_filtered(
    pool: &PgPool,
    filter: &SetupFilter,
) -> Result<Vec<V2SetupRow>, StorageError> {
    let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(
        r#"SELECT id, created_at, updated_at, venue_class, exchange, symbol,
                  timeframe, profile, alt_type, state, direction, confluence_id,
                  entry_price, entry_sl, koruma, target_ref, risk_pct,
                  close_reason, close_price, closed_at, raw_meta, detection_id,
                  pnl_pct, risk_mode, ai_score, trail_mode
             FROM qtss_setups
            WHERE 1=1"#,
    );

    let effective_states: Vec<String> = if filter.states.is_empty() {
        vec!["armed".to_string(), "active".to_string()]
    } else {
        filter.states.clone()
    };
    qb.push(" AND state = ANY(");
    qb.push_bind(effective_states);
    qb.push(")");

    if let Some(v) = filter.venue_class.as_ref() {
        qb.push(" AND venue_class = ");
        qb.push_bind(v.clone());
    }
    if let Some(p) = filter.profile.as_ref() {
        qb.push(" AND profile = ");
        qb.push_bind(p.clone());
    }
    if let Some(a) = filter.alt_type_like.as_ref() {
        qb.push(" AND alt_type LIKE ");
        qb.push_bind(a.clone());
    }
    if let Some(s) = filter.symbol.as_ref() {
        qb.push(" AND symbol = ");
        qb.push_bind(s.clone());
    }
    if let Some(tf) = filter.timeframe.as_ref() {
        qb.push(" AND timeframe = ");
        qb.push_bind(tf.clone());
    }
    if let Some(m) = filter.mode.as_ref() {
        qb.push(" AND mode = ");
        qb.push_bind(m.clone());
    }

    qb.push(" ORDER BY created_at DESC LIMIT ");
    qb.push_bind(filter.limit);

    let rows = qb.build_query_as::<V2SetupRow>().fetch_all(pool).await?;
    Ok(rows)
}

pub async fn list_recent_v2_setups(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<V2SetupRow>, StorageError> {
    let rows = sqlx::query_as::<_, V2SetupRow>(
        r#"SELECT id, created_at, updated_at, venue_class, exchange, symbol,
                  timeframe, profile, alt_type, state, direction, confluence_id,
                  entry_price, entry_sl, koruma, target_ref, risk_pct,
                  close_reason, close_price, closed_at, raw_meta, detection_id,
                  pnl_pct, risk_mode, ai_score, trail_mode
             FROM qtss_setups
            ORDER BY created_at DESC
            LIMIT $1"#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

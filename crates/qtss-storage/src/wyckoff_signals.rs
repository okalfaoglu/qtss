//! Wyckoff signal persistence — idempotent upsert into `qtss_v2_setups`.
//!
//! Callers pass a `WyckoffSetupPayload` (produced by
//! `qtss_wyckoff::persistence::signal_to_payload`) to insert/update a row
//! keyed by `idempotency_key` (migration 0064).
//!
//! Behaviour:
//! * First call with a fresh key → INSERT new `armed` row.
//! * Subsequent calls with the same key → UPDATE plan/ladder/meta in place
//!   (as long as the setup is still `armed` or `active`). Closed setups are
//!   immutable — rescans do not resurrect them.
//! * Returns the row's `id` in both cases so the worker can emit events.
//!
//! CLAUDE.md: #2 — no hardcoded strings beyond SQL; all thresholds live in
//! the caller's config. The persistence path itself has no policy.

use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

/// Plain data carried across the crate boundary. Mirrors
/// `qtss_wyckoff::persistence::WyckoffSetupPayload` — kept as a local
/// struct to keep `qtss-storage` independent of `qtss-wyckoff`.
#[derive(Debug, Clone)]
pub struct WyckoffSetupUpsert {
    pub idempotency_key: String,
    pub venue_class: String,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub mode: String,
    pub profile: String,
    pub alt_type: String,
    pub direction: String,
    pub entry_price: f32,
    pub entry_sl: f32,
    pub target_ref: f32,
    pub tp_ladder_json: JsonValue,
    pub wyckoff_classic_json: JsonValue,
    pub raw_meta_json: JsonValue,
}

/// Insert or update a Wyckoff setup row keyed by `idempotency_key`.
///
/// On conflict:
///   * If existing row is `armed` or `active` → UPDATE plan/meta fields.
///     `state` is NOT regressed — a setup that has moved to `active` stays
///     active and only gains updated TP ladder / classic / raw_meta.
///   * If existing row is `closed` → no-op; returns the existing id.
pub async fn upsert_wyckoff_setup(
    pool: &PgPool,
    row: &WyckoffSetupUpsert,
) -> Result<Uuid, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO qtss_v2_setups (
            venue_class, exchange, symbol, timeframe, profile, alt_type,
            state, direction, mode,
            entry_price, entry_sl, target_ref,
            raw_meta, tp_ladder, wyckoff_classic, idempotency_key
        ) VALUES (
            $1, $2, $3, $4, $5, $6,
            'armed', $7, $8,
            $9, $10, $11,
            $12, $13, $14, $15
        )
        ON CONFLICT (idempotency_key) WHERE idempotency_key IS NOT NULL
        DO UPDATE SET
            entry_price     = EXCLUDED.entry_price,
            entry_sl        = EXCLUDED.entry_sl,
            target_ref      = EXCLUDED.target_ref,
            raw_meta        = EXCLUDED.raw_meta,
            tp_ladder       = EXCLUDED.tp_ladder,
            wyckoff_classic = EXCLUDED.wyckoff_classic,
            updated_at      = now()
          WHERE qtss_v2_setups.state IN ('armed', 'active')
        RETURNING id
        "#,
    )
    .bind(&row.venue_class)
    .bind(&row.exchange)
    .bind(&row.symbol)
    .bind(&row.timeframe)
    .bind(&row.profile)
    .bind(&row.alt_type)
    .bind(&row.direction)
    .bind(&row.mode)
    .bind(row.entry_price)
    .bind(row.entry_sl)
    .bind(row.target_ref)
    .bind(&row.raw_meta_json)
    .bind(&row.tp_ladder_json)
    .bind(&row.wyckoff_classic_json)
    .bind(&row.idempotency_key)
    .fetch_optional(pool)
    .await?;

    match id {
        Some(id) => Ok(id),
        // Closed-row case: ON CONFLICT guard skipped the update. Fetch
        // existing id so the caller still gets a stable reference.
        None => fetch_id_by_key(pool, &row.idempotency_key).await?
            .ok_or(StorageError::DuplicateSetup),
    }
}

async fn fetch_id_by_key(
    pool: &PgPool,
    key: &str,
) -> Result<Option<Uuid>, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"SELECT id FROM qtss_v2_setups WHERE idempotency_key = $1 LIMIT 1"#,
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(id)
}

/// List open Wyckoff setups (armed|active), optionally filtered by mode.
pub async fn list_open_wyckoff_setups(
    pool: &PgPool,
    mode: Option<&str>,
) -> Result<Vec<crate::v2_setups::V2SetupRow>, StorageError> {
    let rows = sqlx::query_as::<_, crate::v2_setups::V2SetupRow>(
        r#"SELECT id, created_at, updated_at, venue_class, exchange, symbol,
                  timeframe, profile, alt_type, state, direction, confluence_id,
                  entry_price, entry_sl, koruma, target_ref, risk_pct,
                  close_reason, close_price, closed_at, raw_meta, detection_id,
                  pnl_pct, risk_mode
             FROM qtss_v2_setups
            WHERE state IN ('armed','active')
              AND alt_type LIKE 'wyckoff_%'
              AND ($1::text IS NULL OR mode = $1)
            ORDER BY created_at DESC"#,
    )
    .bind(mode)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

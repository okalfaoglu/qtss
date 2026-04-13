//! CRUD for wave_projections table.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

// ─── Row type ───────────────────────────────────────────────────────

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct WaveProjectionRow {
    pub id: Uuid,
    pub source_wave_id: Uuid,
    pub alt_group: Uuid,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub degree: String,
    pub projected_kind: String,
    pub projected_label: String,
    pub direction: String,
    pub fib_basis: Option<String>,
    pub projected_legs: serde_json::Value,
    pub probability: f32,
    pub rank: i32,
    pub state: String,
    pub elimination_reason: Option<String>,
    pub bars_validated: i32,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub confirmed_detection_id: Option<Uuid>,
    pub time_start_est: Option<DateTime<Utc>>,
    pub time_end_est: Option<DateTime<Utc>>,
    pub price_target_min: Option<Decimal>,
    pub price_target_max: Option<Decimal>,
    pub invalidation_price: Option<Decimal>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ─── Projected leg JSON shape ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedLeg {
    pub label: String,              // "A", "B", "C", "4", "5"
    pub price_start: f64,
    pub price_end: f64,
    pub time_start_est: Option<String>,  // ISO8601
    pub time_end_est: Option<String>,
    pub fib_level: Option<String>,       // "0.382 retrace"
    pub direction: String,               // "bullish" / "bearish"
}

// ─── Insert type ────────────────────────────────────────────────────

pub struct WaveProjectionInsert {
    pub source_wave_id: Uuid,
    pub alt_group: Uuid,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub degree: String,
    pub projected_kind: String,
    pub projected_label: String,
    pub direction: String,
    pub fib_basis: Option<String>,
    pub projected_legs: serde_json::Value,
    pub probability: f32,
    pub rank: i32,
    pub time_start_est: Option<DateTime<Utc>>,
    pub time_end_est: Option<DateTime<Utc>>,
    pub price_target_min: Option<Decimal>,
    pub price_target_max: Option<Decimal>,
    pub invalidation_price: Option<Decimal>,
}

// ─── Queries ────────────────────────────────────────────────────────

pub async fn insert_projection(
    pool: &PgPool,
    row: &WaveProjectionInsert,
) -> Result<Uuid, sqlx::Error> {
    let rec: (Uuid,) = sqlx::query_as(
        r#"INSERT INTO wave_projections
           (source_wave_id, alt_group, exchange, symbol, timeframe, degree,
            projected_kind, projected_label, direction, fib_basis,
            projected_legs, probability, rank,
            time_start_est, time_end_est,
            price_target_min, price_target_max, invalidation_price)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)
           RETURNING id"#,
    )
    .bind(row.source_wave_id)
    .bind(row.alt_group)
    .bind(&row.exchange)
    .bind(&row.symbol)
    .bind(&row.timeframe)
    .bind(&row.degree)
    .bind(&row.projected_kind)
    .bind(&row.projected_label)
    .bind(&row.direction)
    .bind(&row.fib_basis)
    .bind(&row.projected_legs)
    .bind(row.probability)
    .bind(row.rank)
    .bind(row.time_start_est)
    .bind(row.time_end_est)
    .bind(row.price_target_min)
    .bind(row.price_target_max)
    .bind(row.invalidation_price)
    .fetch_one(pool)
    .await?;
    Ok(rec.0)
}

/// List active/leading projections for a source wave.
pub async fn list_by_source(
    pool: &PgPool,
    source_wave_id: Uuid,
) -> Result<Vec<WaveProjectionRow>, sqlx::Error> {
    sqlx::query_as::<_, WaveProjectionRow>(
        r#"SELECT * FROM wave_projections
           WHERE source_wave_id = $1
           ORDER BY rank ASC, probability DESC"#,
    )
    .bind(source_wave_id)
    .fetch_all(pool)
    .await
}

/// List all projections in an alt_group.
pub async fn list_by_alt_group(
    pool: &PgPool,
    alt_group: Uuid,
) -> Result<Vec<WaveProjectionRow>, sqlx::Error> {
    sqlx::query_as::<_, WaveProjectionRow>(
        r#"SELECT * FROM wave_projections
           WHERE alt_group = $1
           ORDER BY rank ASC, probability DESC"#,
    )
    .bind(alt_group)
    .fetch_all(pool)
    .await
}

/// List active projections for a symbol+timeframe (for validation loop).
pub async fn list_active_projections(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
) -> Result<Vec<WaveProjectionRow>, sqlx::Error> {
    sqlx::query_as::<_, WaveProjectionRow>(
        r#"SELECT * FROM wave_projections
           WHERE exchange = $1 AND symbol = $2 AND timeframe = $3
             AND state IN ('active', 'leading')
           ORDER BY created_at DESC"#,
    )
    .bind(exchange)
    .bind(symbol)
    .bind(timeframe)
    .fetch_all(pool)
    .await
}

/// Eliminate a projection.
pub async fn eliminate_projection(
    pool: &PgPool,
    id: Uuid,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE wave_projections
           SET state = 'eliminated', elimination_reason = $2, updated_at = now()
           WHERE id = $1"#,
    )
    .bind(id)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(())
}

/// Confirm a projection (matched by real detection).
pub async fn confirm_projection(
    pool: &PgPool,
    id: Uuid,
    detection_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE wave_projections
           SET state = 'confirmed', confirmed_detection_id = $2, updated_at = now()
           WHERE id = $1"#,
    )
    .bind(id)
    .bind(detection_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Update validation progress.
pub async fn update_validation(
    pool: &PgPool,
    id: Uuid,
    bars_validated: i32,
    new_probability: f32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE wave_projections
           SET bars_validated = $2, probability = $3,
               last_validated_at = now(), updated_at = now()
           WHERE id = $1"#,
    )
    .bind(id)
    .bind(bars_validated)
    .bind(new_probability)
    .execute(pool)
    .await?;
    Ok(())
}

/// Recalculate ranks within an alt_group (highest probability = rank 1).
pub async fn recalculate_ranks(
    pool: &PgPool,
    alt_group: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"WITH ranked AS (
             SELECT id,
                    ROW_NUMBER() OVER (ORDER BY probability DESC) AS new_rank
             FROM wave_projections
             WHERE alt_group = $1 AND state IN ('active', 'leading')
           )
           UPDATE wave_projections wp
           SET rank = r.new_rank,
               state = CASE WHEN r.new_rank = 1 THEN 'leading' ELSE 'active' END,
               updated_at = now()
           FROM ranked r
           WHERE wp.id = r.id"#,
    )
    .bind(alt_group)
    .execute(pool)
    .await?;
    Ok(())
}

/// Delete all projections for a source wave (when source invalidated).
pub async fn delete_by_source(
    pool: &PgPool,
    source_wave_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        "DELETE FROM wave_projections WHERE source_wave_id = $1",
    )
    .bind(source_wave_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Count active projections for a source wave.
pub async fn count_active_by_source(
    pool: &PgPool,
    source_wave_id: Uuid,
) -> Result<i64, sqlx::Error> {
    let rec: (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*) FROM wave_projections
           WHERE source_wave_id = $1 AND state IN ('active', 'leading')"#,
    )
    .bind(source_wave_id)
    .fetch_one(pool)
    .await?;
    Ok(rec.0)
}

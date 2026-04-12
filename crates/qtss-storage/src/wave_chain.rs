//! wave_chain CRUD — Elliott Deep cross-TF wave linking.
//!
//! Each row represents a single wave segment (e.g., wave 3 of a Minor
//! impulse on 1h). Rows form a tree via `parent_id`: a Cycle wave III
//! on 1w is the parent of Primary waves [1]-[5] on 1d.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Full row from the `wave_chain` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WaveChainRow {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub degree: String,
    pub kind: String,
    pub direction: String,
    pub wave_number: Option<String>,
    pub bar_start: i64,
    pub bar_end: i64,
    pub price_start: Decimal,
    pub price_end: Decimal,
    pub structural_score: f32,
    pub state: String,
    pub detection_id: Option<Uuid>,
    pub time_start: Option<DateTime<Utc>>,
    pub time_end: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert payload (no id/timestamps — DB generates).
pub struct WaveChainInsert {
    pub parent_id: Option<Uuid>,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub degree: String,
    pub kind: String,
    pub direction: String,
    pub wave_number: Option<String>,
    pub bar_start: i64,
    pub bar_end: i64,
    pub price_start: Decimal,
    pub price_end: Decimal,
    pub structural_score: f32,
    pub state: String,
    pub detection_id: Option<Uuid>,
    pub time_start: Option<DateTime<Utc>>,
    pub time_end: Option<DateTime<Utc>>,
}

/// Insert a wave segment, return the generated UUID.
pub async fn insert_wave_chain(pool: &PgPool, row: &WaveChainInsert) -> Result<Uuid, sqlx::Error> {
    let id: (Uuid,) = sqlx::query_as(
        r#"INSERT INTO wave_chain
           (parent_id, exchange, symbol, timeframe, degree, kind, direction,
            wave_number, bar_start, bar_end, price_start, price_end,
            structural_score, state, detection_id, time_start, time_end)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)
           RETURNING id"#,
    )
    .bind(row.parent_id)
    .bind(&row.exchange)
    .bind(&row.symbol)
    .bind(&row.timeframe)
    .bind(&row.degree)
    .bind(&row.kind)
    .bind(&row.direction)
    .bind(&row.wave_number)
    .bind(row.bar_start)
    .bind(row.bar_end)
    .bind(row.price_start)
    .bind(row.price_end)
    .bind(row.structural_score)
    .bind(&row.state)
    .bind(row.detection_id)
    .bind(row.time_start)
    .bind(row.time_end)
    .fetch_one(pool)
    .await?;
    Ok(id.0)
}

/// Find a parent wave on the parent TF whose time range contains the child.
/// Returns the best-scoring match.
pub async fn find_parent_wave(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    parent_tf: &str,
    parent_degree: &str,
    child_time_start: DateTime<Utc>,
    child_time_end: DateTime<Utc>,
) -> Result<Option<WaveChainRow>, sqlx::Error> {
    sqlx::query_as::<_, WaveChainRow>(
        r#"SELECT * FROM wave_chain
           WHERE exchange = $1 AND symbol = $2
             AND timeframe = $3 AND degree = $4
             AND state != 'invalidated'
             AND time_start <= $5 AND time_end >= $6
           ORDER BY structural_score DESC
           LIMIT 1"#,
    )
    .bind(exchange)
    .bind(symbol)
    .bind(parent_tf)
    .bind(parent_degree)
    .bind(child_time_start)
    .bind(child_time_end)
    .fetch_optional(pool)
    .await
}

/// Link orphan child waves to a parent (set parent_id on children whose
/// time range falls within the parent's range).
pub async fn adopt_children(
    pool: &PgPool,
    parent_id: Uuid,
    exchange: &str,
    symbol: &str,
    child_tf: &str,
    child_degree: &str,
    parent_time_start: DateTime<Utc>,
    parent_time_end: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        r#"UPDATE wave_chain
           SET parent_id = $1, updated_at = NOW()
           WHERE exchange = $2 AND symbol = $3
             AND timeframe = $4 AND degree = $5
             AND parent_id IS NULL
             AND state != 'invalidated'
             AND time_start >= $6 AND time_end <= $7"#,
    )
    .bind(parent_id)
    .bind(exchange)
    .bind(symbol)
    .bind(child_tf)
    .bind(child_degree)
    .bind(parent_time_start)
    .bind(parent_time_end)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// List children of a parent wave, ordered by wave_number.
pub async fn list_children(pool: &PgPool, parent_id: Uuid) -> Result<Vec<WaveChainRow>, sqlx::Error> {
    sqlx::query_as::<_, WaveChainRow>(
        r#"SELECT * FROM wave_chain
           WHERE parent_id = $1
           ORDER BY time_start ASC"#,
    )
    .bind(parent_id)
    .fetch_all(pool)
    .await
}

/// Walk the ancestor chain from a wave up to the root.
/// Returns `[self, parent, grandparent, ...]` (closest first).
pub async fn get_ancestor_chain(pool: &PgPool, wave_id: Uuid) -> Result<Vec<WaveChainRow>, sqlx::Error> {
    sqlx::query_as::<_, WaveChainRow>(
        r#"WITH RECURSIVE chain AS (
             SELECT * FROM wave_chain WHERE id = $1
             UNION ALL
             SELECT w.* FROM wave_chain w
               JOIN chain c ON w.id = c.parent_id
           )
           SELECT * FROM chain"#,
    )
    .bind(wave_id)
    .fetch_all(pool)
    .await
}

/// Find wave_chain row by detection_id.
pub async fn find_by_detection(pool: &PgPool, detection_id: Uuid) -> Result<Option<WaveChainRow>, sqlx::Error> {
    sqlx::query_as::<_, WaveChainRow>(
        "SELECT * FROM wave_chain WHERE detection_id = $1 LIMIT 1",
    )
    .bind(detection_id)
    .fetch_optional(pool)
    .await
}

/// Invalidate a wave and optionally cascade to children.
pub async fn invalidate_wave(pool: &PgPool, wave_id: Uuid, cascade: bool) -> Result<u64, sqlx::Error> {
    let mut affected = 0u64;
    let res = sqlx::query(
        "UPDATE wave_chain SET state = 'invalidated', updated_at = NOW() WHERE id = $1",
    )
    .bind(wave_id)
    .execute(pool)
    .await?;
    affected += res.rows_affected();

    if cascade {
        let res = sqlx::query(
            r#"WITH RECURSIVE descendants AS (
                 SELECT id FROM wave_chain WHERE parent_id = $1
                 UNION ALL
                 SELECT w.id FROM wave_chain w
                   JOIN descendants d ON w.parent_id = d.id
               )
               UPDATE wave_chain SET state = 'invalidated', updated_at = NOW()
               WHERE id IN (SELECT id FROM descendants)"#,
        )
        .bind(wave_id)
        .execute(pool)
        .await?;
        affected += res.rows_affected();
    }
    Ok(affected)
}

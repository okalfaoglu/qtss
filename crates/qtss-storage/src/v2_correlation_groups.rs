//! Faz 8.0 — `qtss_v2_correlation_groups` repo. Seeded lookup table
//! the allocator uses to enforce the per-group concurrency cap.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CorrelationGroupRow {
    pub id: Uuid,
    pub venue_class: String,
    pub group_key: String,
    pub symbol: String,
    pub weight: f32,
}

/// All group_keys a given symbol belongs to inside a venue class.
pub async fn list_groups_for_symbol(
    pool: &PgPool,
    venue_class: &str,
    symbol: &str,
) -> Result<Vec<String>, StorageError> {
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"SELECT group_key
             FROM qtss_v2_correlation_groups
            WHERE venue_class = $1 AND symbol = $2"#,
    )
    .bind(venue_class)
    .bind(symbol)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(g,)| g).collect())
}

/// Number of currently open (`armed`/`active`) setups whose symbol
/// is in the given correlation group. When `direction` is `Some`,
/// only setups in that direction are counted — used when
/// `setup.risk.correlation.same_direction_only = true`.
pub async fn count_open_setups_in_group(
    pool: &PgPool,
    venue_class: &str,
    group_key: &str,
    direction: Option<&str>,
) -> Result<i64, StorageError> {
    let n: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint
          FROM qtss_v2_setups s
          JOIN qtss_v2_correlation_groups g
            ON g.venue_class = s.venue_class
           AND g.symbol      = s.symbol
         WHERE s.state IN ('armed','active')
           AND s.venue_class = $1
           AND g.group_key   = $2
           AND ($3::text IS NULL OR s.direction = $3)
        "#,
    )
    .bind(venue_class)
    .bind(group_key)
    .bind(direction)
    .fetch_one(pool)
    .await?;
    Ok(n)
}

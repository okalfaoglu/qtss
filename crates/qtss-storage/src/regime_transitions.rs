//! Storage for regime_transitions table (Faz 11).

use chrono::{DateTime, Utc};
use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RegimeTransitionRow {
    pub id: Uuid,
    pub symbol: String,
    pub interval: String,
    pub from_regime: String,
    pub to_regime: String,
    pub transition_speed: Option<f64>,
    pub confidence: f64,
    pub confirming_indicators: Json<Vec<String>>,
    pub hmm_probability: Option<f64>,
    pub detected_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub was_correct: Option<bool>,
}

pub struct RegimeTransitionInsert {
    pub symbol: String,
    pub interval: String,
    pub from_regime: String,
    pub to_regime: String,
    pub transition_speed: Option<f64>,
    pub confidence: f64,
    pub confirming_indicators: Vec<String>,
    pub hmm_probability: Option<f64>,
}

pub async fn insert_regime_transition(
    pool: &PgPool,
    row: &RegimeTransitionInsert,
) -> Result<Uuid, sqlx::Error> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO regime_transitions
           (symbol, interval, from_regime, to_regime, transition_speed,
            confidence, confirming_indicators, hmm_probability)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
           RETURNING id"#,
    )
    .bind(&row.symbol)
    .bind(&row.interval)
    .bind(&row.from_regime)
    .bind(&row.to_regime)
    .bind(row.transition_speed)
    .bind(row.confidence)
    .bind(Json(&row.confirming_indicators))
    .bind(row.hmm_probability)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Active (unresolved) transitions.
pub async fn list_active_transitions(
    pool: &PgPool,
) -> Result<Vec<RegimeTransitionRow>, sqlx::Error> {
    sqlx::query_as::<_, RegimeTransitionRow>(
        r#"SELECT * FROM regime_transitions
           WHERE resolved_at IS NULL
           ORDER BY detected_at DESC"#,
    )
    .fetch_all(pool)
    .await
}

/// Recent transitions (resolved or not), limit N.
pub async fn list_recent_transitions(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<RegimeTransitionRow>, sqlx::Error> {
    sqlx::query_as::<_, RegimeTransitionRow>(
        r#"SELECT * FROM regime_transitions
           ORDER BY detected_at DESC LIMIT $1"#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Resolve a transition (mark it as confirmed).
pub async fn resolve_transition(
    pool: &PgPool,
    id: Uuid,
    was_correct: bool,
) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        r#"UPDATE regime_transitions
           SET resolved_at = now(), was_correct = $2
           WHERE id = $1"#,
    )
    .bind(id)
    .bind(was_correct)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

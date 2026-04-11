//! Storage for regime_snapshots table (Faz 11).

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RegimeSnapshotRow {
    pub id: Uuid,
    pub symbol: String,
    pub interval: String,
    pub regime: String,
    pub trend_strength: Option<String>,
    pub confidence: f64,
    pub adx: Option<f64>,
    pub plus_di: Option<f64>,
    pub minus_di: Option<f64>,
    pub bb_width: Option<f64>,
    pub atr_pct: Option<f64>,
    pub choppiness: Option<f64>,
    pub hmm_state: Option<String>,
    pub hmm_confidence: Option<f64>,
    pub computed_at: DateTime<Utc>,
}

pub struct RegimeSnapshotInsert {
    pub symbol: String,
    pub interval: String,
    pub regime: String,
    pub trend_strength: Option<String>,
    pub confidence: f64,
    pub adx: Option<f64>,
    pub plus_di: Option<f64>,
    pub minus_di: Option<f64>,
    pub bb_width: Option<f64>,
    pub atr_pct: Option<f64>,
    pub choppiness: Option<f64>,
    pub hmm_state: Option<String>,
    pub hmm_confidence: Option<f64>,
}

pub async fn insert_regime_snapshot(
    pool: &PgPool,
    row: &RegimeSnapshotInsert,
) -> Result<Uuid, sqlx::Error> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO regime_snapshots
           (symbol, interval, regime, trend_strength, confidence,
            adx, plus_di, minus_di, bb_width, atr_pct, choppiness,
            hmm_state, hmm_confidence)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
           RETURNING id"#,
    )
    .bind(&row.symbol)
    .bind(&row.interval)
    .bind(&row.regime)
    .bind(&row.trend_strength)
    .bind(row.confidence)
    .bind(row.adx)
    .bind(row.plus_di)
    .bind(row.minus_di)
    .bind(row.bb_width)
    .bind(row.atr_pct)
    .bind(row.choppiness)
    .bind(&row.hmm_state)
    .bind(row.hmm_confidence)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Latest snapshot per (symbol, interval).
pub async fn latest_regime_snapshot(
    pool: &PgPool,
    symbol: &str,
    interval: &str,
) -> Result<Option<RegimeSnapshotRow>, sqlx::Error> {
    sqlx::query_as::<_, RegimeSnapshotRow>(
        r#"SELECT * FROM regime_snapshots
           WHERE symbol = $1 AND interval = $2
           ORDER BY computed_at DESC LIMIT 1"#,
    )
    .bind(symbol)
    .bind(interval)
    .fetch_optional(pool)
    .await
}

/// All latest snapshots for a symbol (one per interval).
pub async fn latest_snapshots_for_symbol(
    pool: &PgPool,
    symbol: &str,
) -> Result<Vec<RegimeSnapshotRow>, sqlx::Error> {
    sqlx::query_as::<_, RegimeSnapshotRow>(
        r#"SELECT DISTINCT ON (interval) *
           FROM regime_snapshots
           WHERE symbol = $1
           ORDER BY interval, computed_at DESC"#,
    )
    .bind(symbol)
    .fetch_all(pool)
    .await
}

/// All latest snapshots across all symbols (one per symbol+interval).
pub async fn latest_snapshots_all(
    pool: &PgPool,
) -> Result<Vec<RegimeSnapshotRow>, sqlx::Error> {
    sqlx::query_as::<_, RegimeSnapshotRow>(
        r#"SELECT DISTINCT ON (symbol, interval) *
           FROM regime_snapshots
           ORDER BY symbol, interval, computed_at DESC"#,
    )
    .fetch_all(pool)
    .await
}

/// Timeline for chart overlay: last N snapshots for (symbol, interval).
pub async fn regime_timeline(
    pool: &PgPool,
    symbol: &str,
    interval: &str,
    limit: i64,
) -> Result<Vec<RegimeSnapshotRow>, sqlx::Error> {
    sqlx::query_as::<_, RegimeSnapshotRow>(
        r#"SELECT * FROM regime_snapshots
           WHERE symbol = $1 AND interval = $2
           ORDER BY computed_at DESC LIMIT $3"#,
    )
    .bind(symbol)
    .bind(interval)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Purge old snapshots beyond retention.
pub async fn purge_old_snapshots(
    pool: &PgPool,
    retention_days: i64,
) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        r#"DELETE FROM regime_snapshots
           WHERE computed_at < now() - make_interval(days => $1)"#,
    )
    .bind(retention_days as i32)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

//! Regime-based performance correlation (Faz 11).
//!
//! Queries completed setups grouped by the regime that was active at
//! setup creation time, computing win rate and average P&L per regime.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RegimePerformanceRow {
    pub regime: String,
    pub total: i64,
    pub wins: i64,
    pub win_rate: f64,
    pub avg_pnl_pct: f64,
}

/// Performance by regime over the last N days.
/// Reads from v2_setups + regime_snapshots correlation.
pub async fn regime_performance(
    pool: &PgPool,
    days: i64,
) -> Result<Vec<RegimePerformanceRow>, sqlx::Error> {
    // Join setups with the regime snapshot closest to setup creation time.
    // Since regime_snapshots stores periodic snapshots, we use LATERAL join
    // to find the closest snapshot for each setup's symbol at creation time.
    sqlx::query_as::<_, RegimePerformanceRow>(
        r#"
        WITH setup_regimes AS (
            SELECT
                s.id,
                s.state,
                s.pnl_pct,
                COALESCE(
                    (SELECT rs.regime
                     FROM regime_snapshots rs
                     WHERE rs.symbol = s.symbol
                       AND rs.interval = '1h'
                       AND rs.computed_at <= s.created_at
                     ORDER BY rs.computed_at DESC
                     LIMIT 1),
                    'uncertain'
                ) as regime
            FROM qtss_v2_setups s
            WHERE s.created_at > now() - make_interval(days => $1)
              AND s.state IN ('closed_win', 'closed_loss', 'closed_manual')
        )
        SELECT
            regime,
            COUNT(*) as total,
            COUNT(*) FILTER (WHERE state = 'closed_win') as wins,
            CASE WHEN COUNT(*) > 0
                 THEN ROUND(100.0 * COUNT(*) FILTER (WHERE state = 'closed_win') / COUNT(*), 1)
                 ELSE 0.0 END as win_rate,
            COALESCE(ROUND(AVG(pnl_pct)::numeric, 2)::float8, 0.0) as avg_pnl_pct
        FROM setup_regimes
        GROUP BY regime
        ORDER BY total DESC
        "#,
    )
    .bind(days as i32)
    .fetch_all(pool)
    .await
}

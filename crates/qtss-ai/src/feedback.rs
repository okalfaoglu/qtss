//! Feedback loop — persist outcomes and expose summaries for strategic context (FAZ 6.3).

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AiResult;
use crate::storage::{fetch_recent_outcome_stats, insert_ai_decision_outcome};

/// Records a closed-trade outcome linked to an `ai_decisions` row when known.
pub async fn record_decision_outcome(
    pool: &PgPool,
    decision_id: Uuid,
    pnl_pct: Option<f64>,
    pnl_usdt: Option<f64>,
    outcome: &str,
    holding_hours: Option<f64>,
    notes: Option<&str>,
) -> AiResult<Uuid> {
    insert_ai_decision_outcome(pool, decision_id, pnl_pct, pnl_usdt, outcome, holding_hours, notes).await
}

/// Last `n` decision outcomes aggregated for prompts.
pub async fn outcome_stats_for_prompt(pool: &PgPool, n: i64) -> AiResult<serde_json::Value> {
    fetch_recent_outcome_stats(pool, n).await
}

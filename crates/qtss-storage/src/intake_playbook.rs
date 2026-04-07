//! `intake_playbook_runs` / `intake_playbook_candidates` — smart-money playbook sweeps (worker + API).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct IntakePlaybookRunRow {
    pub id: Uuid,
    pub playbook_id: String,
    pub computed_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub market_mode: Option<String>,
    pub confidence_0_100: i32,
    pub key_reason: Option<String>,
    pub neutral_guidance: Option<String>,
    pub summary_json: JsonValue,
    pub inputs_json: JsonValue,
    pub meta_json: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct IntakePlaybookCandidateRow {
    pub id: Uuid,
    pub run_id: Uuid,
    pub rank: i32,
    pub symbol: String,
    pub chain: Option<String>,
    pub direction: String,
    pub intake_tier: String,
    pub confidence_0_100: i32,
    pub detail_json: JsonValue,
    pub merged_engine_symbol_id: Option<Uuid>,
    pub merged_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct IntakePlaybookRunInsert<'a> {
    pub playbook_id: &'a str,
    pub expires_at: Option<DateTime<Utc>>,
    pub market_mode: Option<&'a str>,
    pub confidence_0_100: i32,
    pub key_reason: Option<&'a str>,
    pub neutral_guidance: Option<&'a str>,
    pub summary_json: &'a JsonValue,
    pub inputs_json: &'a JsonValue,
    pub meta_json: &'a JsonValue,
}

#[derive(Debug, Clone)]
pub struct IntakePlaybookCandidateInsert<'a> {
    pub rank: i32,
    pub symbol: &'a str,
    pub chain: Option<&'a str>,
    pub direction: &'a str,
    pub intake_tier: &'a str,
    pub confidence_0_100: i32,
    pub detail_json: &'a JsonValue,
}

pub async fn insert_intake_playbook_run(
    pool: &PgPool,
    row: &IntakePlaybookRunInsert<'_>,
) -> Result<Uuid, StorageError> {
    let id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO intake_playbook_runs (
            playbook_id, expires_at, market_mode, confidence_0_100, key_reason, neutral_guidance,
            summary_json, inputs_json, meta_json
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        RETURNING id"#,
    )
    .bind(row.playbook_id)
    .bind(row.expires_at)
    .bind(row.market_mode)
    .bind(row.confidence_0_100)
    .bind(row.key_reason)
    .bind(row.neutral_guidance)
    .bind(row.summary_json)
    .bind(row.inputs_json)
    .bind(row.meta_json)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn insert_intake_playbook_candidates(
    pool: &PgPool,
    run_id: Uuid,
    candidates: &[IntakePlaybookCandidateInsert<'_>],
) -> Result<(), StorageError> {
    for c in candidates {
        sqlx::query(
            r#"INSERT INTO intake_playbook_candidates (
                run_id, rank, symbol, chain, direction, intake_tier, confidence_0_100, detail_json
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)"#,
        )
        .bind(run_id)
        .bind(c.rank)
        .bind(c.symbol)
        .bind(c.chain)
        .bind(c.direction)
        .bind(c.intake_tier)
        .bind(c.confidence_0_100)
        .bind(c.detail_json)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Latest run for a playbook id (e.g. `market_mode`, `elite_long`).
pub async fn fetch_intake_playbook_run_by_id(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<IntakePlaybookRunRow>, StorageError> {
    let row = sqlx::query_as::<_, IntakePlaybookRunRow>(
        r#"SELECT id, playbook_id, computed_at, expires_at, market_mode, confidence_0_100,
                  key_reason, neutral_guidance, summary_json, inputs_json, meta_json
           FROM intake_playbook_runs
           WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn fetch_intake_playbook_candidate_by_id(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<IntakePlaybookCandidateRow>, StorageError> {
    let row = sqlx::query_as::<_, IntakePlaybookCandidateRow>(
        r#"SELECT id, run_id, rank, symbol, chain, direction, intake_tier, confidence_0_100,
                  detail_json, merged_engine_symbol_id, merged_at
           FROM intake_playbook_candidates
           WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn fetch_latest_intake_playbook_run(
    pool: &PgPool,
    playbook_id: &str,
) -> Result<Option<IntakePlaybookRunRow>, StorageError> {
    let row = sqlx::query_as::<_, IntakePlaybookRunRow>(
        r#"SELECT id, playbook_id, computed_at, expires_at, market_mode, confidence_0_100,
                  key_reason, neutral_guidance, summary_json, inputs_json, meta_json
           FROM intake_playbook_runs
           WHERE playbook_id = $1
           ORDER BY computed_at DESC
           LIMIT 1"#,
    )
    .bind(playbook_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_intake_playbook_candidates_for_run(
    pool: &PgPool,
    run_id: Uuid,
) -> Result<Vec<IntakePlaybookCandidateRow>, StorageError> {
    let rows = sqlx::query_as::<_, IntakePlaybookCandidateRow>(
        r#"SELECT id, run_id, rank, symbol, chain, direction, intake_tier, confidence_0_100,
                  detail_json, merged_engine_symbol_id, merged_at
           FROM intake_playbook_candidates
           WHERE run_id = $1
           ORDER BY rank ASC"#,
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Recent runs across all playbooks (dashboard).
pub async fn list_recent_intake_playbook_runs(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<IntakePlaybookRunRow>, StorageError> {
    let lim = limit.clamp(1, 500);
    let rows = sqlx::query_as::<_, IntakePlaybookRunRow>(
        r#"SELECT id, playbook_id, computed_at, expires_at, market_mode, confidence_0_100,
                  key_reason, neutral_guidance, summary_json, inputs_json, meta_json
           FROM intake_playbook_runs
           ORDER BY computed_at DESC
           LIMIT $1"#,
    )
    .bind(lim)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Un-promoted candidates from recent runs: `merged_engine_symbol_id IS NULL`,
/// filtered by playbook IDs and minimum confidence, ordered by confidence DESC.
pub async fn list_promotable_intake_candidates(
    pool: &PgPool,
    playbook_ids: &[String],
    min_confidence: i32,
    limit: i64,
) -> Result<Vec<IntakePlaybookCandidateRow>, StorageError> {
    let lim = limit.clamp(1, 200);
    let rows = sqlx::query_as::<_, IntakePlaybookCandidateRow>(
        r#"SELECT c.id, c.run_id, c.rank, c.symbol, c.chain, c.direction, c.intake_tier,
                  c.confidence_0_100, c.detail_json, c.merged_engine_symbol_id, c.merged_at
           FROM intake_playbook_candidates c
           JOIN intake_playbook_runs r ON r.id = c.run_id
           WHERE c.merged_engine_symbol_id IS NULL
             AND c.confidence_0_100 >= $1
             AND r.playbook_id = ANY($2)
             AND r.computed_at > now() - interval '6 hours'
             AND (r.expires_at IS NULL OR r.expires_at > now())
           ORDER BY c.confidence_0_100 DESC, c.rank ASC
           LIMIT $3"#,
    )
    .bind(min_confidence)
    .bind(playbook_ids)
    .bind(lim)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn update_intake_candidate_merged_engine_symbol(
    pool: &PgPool,
    candidate_id: Uuid,
    engine_symbol_id: Uuid,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE intake_playbook_candidates
           SET merged_engine_symbol_id = $2, merged_at = now()
           WHERE id = $1"#,
    )
    .bind(candidate_id)
    .bind(engine_symbol_id)
    .execute(pool)
    .await?;
    Ok(())
}

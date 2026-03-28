//! `nansen_setup_runs` + `nansen_setup_rows` — worker skorlama çıktısı.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct NansenSetupRunRow {
    pub id: Uuid,
    pub computed_at: DateTime<Utc>,
    pub request_json: JsonValue,
    pub source: String,
    pub candidate_count: i32,
    pub meta_json: Option<JsonValue>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct NansenSetupRowDetail {
    pub id: Uuid,
    pub run_id: Uuid,
    pub rank: i32,
    pub chain: String,
    pub token_address: String,
    pub token_symbol: String,
    pub direction: String,
    pub score: i32,
    pub probability: f64,
    pub setup: String,
    pub key_signals: JsonValue,
    pub entry: f64,
    pub stop_loss: f64,
    pub tp1: f64,
    pub tp2: f64,
    pub tp3: f64,
    pub rr: f64,
    pub pct_to_tp2: f64,
    pub ohlc_enriched: bool,
    pub raw_metrics: JsonValue,
}

#[derive(Debug, Clone)]
pub struct NansenSetupRunInsert {
    pub request_json: JsonValue,
    pub source: String,
    pub candidate_count: i32,
    pub meta_json: Option<JsonValue>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NansenSetupRowInsert {
    pub rank: i32,
    pub chain: String,
    pub token_address: String,
    pub token_symbol: String,
    pub direction: String,
    pub score: i32,
    pub probability: f64,
    pub setup: String,
    pub key_signals: JsonValue,
    pub entry: f64,
    pub stop_loss: f64,
    pub tp1: f64,
    pub tp2: f64,
    pub tp3: f64,
    pub rr: f64,
    pub pct_to_tp2: f64,
    pub ohlc_enriched: bool,
    pub raw_metrics: JsonValue,
}

pub async fn insert_nansen_setup_run(pool: &PgPool, row: &NansenSetupRunInsert) -> Result<Uuid, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO nansen_setup_runs (
               request_json, source, candidate_count, meta_json, error
           ) VALUES ($1, $2, $3, $4, $5)
           RETURNING id"#,
    )
    .bind(&row.request_json)
    .bind(&row.source)
    .bind(row.candidate_count)
    .bind(&row.meta_json)
    .bind(&row.error)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn insert_nansen_setup_row(pool: &PgPool, run_id: Uuid, row: &NansenSetupRowInsert) -> Result<(), StorageError> {
    sqlx::query(
        r#"INSERT INTO nansen_setup_rows (
               run_id, rank, chain, token_address, token_symbol, direction,
               score, probability, setup, key_signals,
               entry, stop_loss, tp1, tp2, tp3, rr, pct_to_tp2,
               ohlc_enriched, raw_metrics
           ) VALUES (
               $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19
           )"#,
    )
    .bind(run_id)
    .bind(row.rank)
    .bind(&row.chain)
    .bind(&row.token_address)
    .bind(&row.token_symbol)
    .bind(&row.direction)
    .bind(row.score)
    .bind(row.probability)
    .bind(&row.setup)
    .bind(&row.key_signals)
    .bind(row.entry)
    .bind(row.stop_loss)
    .bind(row.tp1)
    .bind(row.tp2)
    .bind(row.tp3)
    .bind(row.rr)
    .bind(row.pct_to_tp2)
    .bind(row.ohlc_enriched)
    .bind(&row.raw_metrics)
    .execute(pool)
    .await?;
    Ok(())
}

/// Son başarılı koşu (`error` boş) + satırlar; yoksa `None`.
pub async fn fetch_latest_nansen_setup_with_rows(
    pool: &PgPool,
) -> Result<Option<(NansenSetupRunRow, Vec<NansenSetupRowDetail>)>, StorageError> {
    let run = sqlx::query_as::<_, NansenSetupRunRow>(
        r#"SELECT id, computed_at, request_json, source, candidate_count, meta_json, error
           FROM nansen_setup_runs
           WHERE error IS NULL
           ORDER BY computed_at DESC
           LIMIT 1"#,
    )
    .fetch_optional(pool)
    .await?;

    let Some(run) = run else {
        return Ok(None);
    };

    let rows = sqlx::query_as::<_, NansenSetupRowDetail>(
        r#"SELECT id, run_id, rank, chain, token_address, token_symbol, direction,
                  score, probability, setup, key_signals,
                  entry, stop_loss, tp1, tp2, tp3, rr, pct_to_tp2,
                  ohlc_enriched, raw_metrics
           FROM nansen_setup_rows
           WHERE run_id = $1
           ORDER BY rank ASC"#,
    )
    .bind(run.id)
    .fetch_all(pool)
    .await?;

    Ok(Some((run, rows)))
}

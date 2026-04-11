//! Storage layer for `nansen_raw_flows` + `nansen_enriched_signals`.
//!
//! Raw flows are append-only: every Nansen API row is persisted so AI
//! can read historical patterns. Enriched signals are the output of
//! the cross-chain / DEX-spike / whale analyzers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

// ── Raw Flows ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct NansenRawFlowInsert<'a> {
    pub source_type: &'a str,
    pub chain: Option<&'a str>,
    pub token_symbol: Option<&'a str>,
    pub token_address: Option<&'a str>,
    pub engine_symbol: Option<&'a str>,
    pub direction: Option<&'a str>,
    pub value_usd: Option<f64>,
    pub balance_pct_change: Option<f64>,
    pub raw_row: &'a Json,
    pub snapshot_at: DateTime<Utc>,
}

/// Batch insert raw flow rows. Returns number of rows inserted.
pub async fn insert_raw_flows(
    pool: &PgPool,
    rows: &[NansenRawFlowInsert<'_>],
) -> Result<u64, StorageError> {
    if rows.is_empty() {
        return Ok(0);
    }
    // Batch via unnest for performance
    let mut ids = Vec::with_capacity(rows.len());
    let mut source_types = Vec::with_capacity(rows.len());
    let mut chains: Vec<Option<String>> = Vec::with_capacity(rows.len());
    let mut token_symbols: Vec<Option<String>> = Vec::with_capacity(rows.len());
    let mut token_addresses: Vec<Option<String>> = Vec::with_capacity(rows.len());
    let mut engine_symbols: Vec<Option<String>> = Vec::with_capacity(rows.len());
    let mut directions: Vec<Option<String>> = Vec::with_capacity(rows.len());
    let mut values_usd: Vec<Option<f64>> = Vec::with_capacity(rows.len());
    let mut balance_pcts: Vec<Option<f64>> = Vec::with_capacity(rows.len());
    let mut raw_rows: Vec<Json> = Vec::with_capacity(rows.len());
    let mut snapshot_ats: Vec<DateTime<Utc>> = Vec::with_capacity(rows.len());

    for r in rows {
        ids.push(Uuid::new_v4());
        source_types.push(r.source_type.to_string());
        chains.push(r.chain.map(String::from));
        token_symbols.push(r.token_symbol.map(String::from));
        token_addresses.push(r.token_address.map(String::from));
        engine_symbols.push(r.engine_symbol.map(String::from));
        directions.push(r.direction.map(String::from));
        values_usd.push(r.value_usd);
        balance_pcts.push(r.balance_pct_change);
        raw_rows.push(r.raw_row.clone());
        snapshot_ats.push(r.snapshot_at);
    }

    let res = sqlx::query(
        r#"INSERT INTO nansen_raw_flows (
               id, source_type, chain, token_symbol, token_address,
               engine_symbol, direction, value_usd, balance_pct_change,
               raw_row, snapshot_at
           )
           SELECT * FROM UNNEST(
               $1::uuid[], $2::text[], $3::text[], $4::text[], $5::text[],
               $6::text[], $7::text[], $8::float8[], $9::float8[],
               $10::jsonb[], $11::timestamptz[]
           )"#,
    )
    .bind(&ids)
    .bind(&source_types)
    .bind(&chains)
    .bind(&token_symbols)
    .bind(&token_addresses)
    .bind(&engine_symbols)
    .bind(&directions)
    .bind(&values_usd)
    .bind(&balance_pcts)
    .bind(&raw_rows)
    .bind(&snapshot_ats)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Prune rows older than `retention_days`.
pub async fn prune_raw_flows(pool: &PgPool, retention_days: i64) -> Result<u64, StorageError> {
    if retention_days <= 0 {
        return Ok(0);
    }
    let res = sqlx::query(
        "DELETE FROM nansen_raw_flows WHERE created_at < now() - make_interval(days => $1)",
    )
    .bind(retention_days as f64)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

// ── Enriched Signals ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct NansenEnrichedSignalRow {
    pub id: Uuid,
    pub symbol: String,
    pub signal_type: String,
    pub score: f64,
    pub direction: String,
    pub confidence: f64,
    pub chain_breakdown: Option<Json>,
    pub details: Option<Json>,
    pub computed_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct EnrichedSignalInsert<'a> {
    pub symbol: &'a str,
    pub signal_type: &'a str,
    pub score: f64,
    pub direction: &'a str,
    pub confidence: f64,
    pub chain_breakdown: Option<Json>,
    pub details: Option<Json>,
}

pub async fn insert_enriched_signal(
    pool: &PgPool,
    s: &EnrichedSignalInsert<'_>,
) -> Result<NansenEnrichedSignalRow, StorageError> {
    let row = sqlx::query_as::<_, NansenEnrichedSignalRow>(
        r#"INSERT INTO nansen_enriched_signals (
               id, symbol, signal_type, score, direction, confidence,
               chain_breakdown, details
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
           RETURNING *"#,
    )
    .bind(Uuid::new_v4())
    .bind(s.symbol)
    .bind(s.signal_type)
    .bind(s.score)
    .bind(s.direction)
    .bind(s.confidence)
    .bind(&s.chain_breakdown)
    .bind(&s.details)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Latest enriched signal for a (symbol, signal_type) within staleness window.
pub async fn fetch_latest_enriched(
    pool: &PgPool,
    symbol: &str,
    signal_type: &str,
    max_age_secs: i64,
) -> Result<Option<NansenEnrichedSignalRow>, StorageError> {
    let row = sqlx::query_as::<_, NansenEnrichedSignalRow>(
        r#"SELECT * FROM nansen_enriched_signals
            WHERE symbol = $1
              AND signal_type = $2
              AND computed_at > now() - make_interval(secs => $3)
            ORDER BY computed_at DESC
            LIMIT 1"#,
    )
    .bind(symbol)
    .bind(signal_type)
    .bind(max_age_secs as f64)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Fetch last N enriched signals for a symbol (for AI context).
pub async fn list_enriched_for_symbol(
    pool: &PgPool,
    symbol: &str,
    limit: i64,
) -> Result<Vec<NansenEnrichedSignalRow>, StorageError> {
    let rows = sqlx::query_as::<_, NansenEnrichedSignalRow>(
        r#"SELECT * FROM nansen_enriched_signals
            WHERE symbol = $1
            ORDER BY computed_at DESC
            LIMIT $2"#,
    )
    .bind(symbol)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

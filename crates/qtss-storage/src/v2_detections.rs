//! `qtss_v2_detections` — Faz 7 Adım 1.
//!
//! One row per pattern emitted by the v2 detector orchestrator. The
//! validator (Adım 3) updates the same row with `confidence`,
//! `channel_scores`, `validated_at`. The chart endpoint and the
//! Detections panel both read from this single source of truth.
//!
//! State transitions are append-by-update: an in-memory orchestrator
//! never has to keep its own copy of "what is currently forming" — it
//! reads back from this table.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DetectionRow {
    pub id: Uuid,
    pub detected_at: DateTime<Utc>,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub family: String,
    pub subkind: String,
    pub state: String,
    pub structural_score: f32,
    pub confidence: Option<f32>,
    pub invalidation_price: Decimal,
    pub anchors: Json,
    pub regime: Json,
    pub channel_scores: Option<Json>,
    pub raw_meta: Json,
    pub validated_at: Option<DateTime<Utc>>,
    pub mode: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert payload. Borrows where possible so the orchestrator does not
/// have to clone domain types just to write a row.
#[derive(Debug, Clone)]
pub struct NewDetection<'a> {
    pub id: Uuid,
    pub detected_at: DateTime<Utc>,
    pub exchange: &'a str,
    pub symbol: &'a str,
    pub timeframe: &'a str,
    pub family: &'a str,
    pub subkind: &'a str,
    pub state: &'a str,
    pub structural_score: f32,
    pub invalidation_price: Decimal,
    pub anchors: Json,
    pub regime: Json,
    pub raw_meta: Json,
    pub mode: &'a str,
}

#[derive(Debug, Clone)]
pub struct DetectionFilter<'a> {
    pub exchange: Option<&'a str>,
    pub symbol: Option<&'a str>,
    pub timeframe: Option<&'a str>,
    pub family: Option<&'a str>,
    pub state: Option<&'a str>,
    pub mode: Option<&'a str>,
    pub limit: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HistoricalOutcomeRow {
    pub family: String,
    pub subkind: String,
    pub timeframe: String,
    pub validated_count: i64,
    pub invalidated_count: i64,
}

#[derive(Debug, sqlx::FromRow)]
pub struct FormingRow {
    pub id: Uuid,
    pub family: String,
    pub subkind: String,
    pub invalidation_price: Decimal,
    pub anchors: Json,
}

impl FormingRow {
    /// Extract the last anchor's timestamp from the JSON anchors array.
    pub fn last_anchor_time(&self) -> Option<DateTime<Utc>> {
        self.anchors
            .as_array()
            .and_then(|arr| arr.last())
            .and_then(|a| a.get("time"))
            .and_then(|t| t.as_str())
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
    }
}

pub struct V2DetectionRepository {
    pool: PgPool,
}

impl V2DetectionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, d: NewDetection<'_>) -> Result<DetectionRow, StorageError> {
        let row = sqlx::query_as::<_, DetectionRow>(
            r#"INSERT INTO qtss_v2_detections (
                   id, detected_at, exchange, symbol, timeframe,
                   family, subkind, state, structural_score,
                   invalidation_price, anchors, regime, raw_meta, mode
               ) VALUES (
                   $1, $2, $3, $4, $5,
                   $6, $7, $8, $9,
                   $10, $11, $12, $13, $14
               )
               RETURNING id, detected_at, exchange, symbol, timeframe,
                         family, subkind, state, structural_score, confidence,
                         invalidation_price, anchors, regime, channel_scores,
                         raw_meta, validated_at, mode, created_at, updated_at"#,
        )
        .bind(d.id)
        .bind(d.detected_at)
        .bind(d.exchange)
        .bind(d.symbol)
        .bind(d.timeframe)
        .bind(d.family)
        .bind(d.subkind)
        .bind(d.state)
        .bind(d.structural_score)
        .bind(d.invalidation_price)
        .bind(d.anchors)
        .bind(d.regime)
        .bind(d.raw_meta)
        .bind(d.mode)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Refresh a forming detection's projection data and structural score
    /// without inserting a new row. Called when dedup detects the same
    /// structure (same last_anchor_idx) but we still want fresh forecasts.
    pub async fn update_projection(
        &self,
        id: Uuid,
        structural_score: f32,
        raw_meta: Json,
    ) -> Result<u64, StorageError> {
        let res = sqlx::query(
            r#"UPDATE qtss_v2_detections
                   SET structural_score = $2,
                       raw_meta = $3,
                       updated_at = NOW()
                 WHERE id = $1"#,
        )
        .bind(id)
        .bind(structural_score)
        .bind(raw_meta)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Move a detection between forming/confirmed/invalidated/completed.
    /// Returns the number of rows updated (0 = unknown id).
    pub async fn update_state(&self, id: Uuid, state: &str) -> Result<u64, StorageError> {
        let res = sqlx::query("UPDATE qtss_v2_detections SET state = $2 WHERE id = $1")
            .bind(id)
            .bind(state)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
    }

    /// Validator output: confidence + per-channel breakdown.
    pub async fn mark_validated(
        &self,
        id: Uuid,
        confidence: f32,
        channel_scores: Json,
        validated_at: DateTime<Utc>,
    ) -> Result<u64, StorageError> {
        let res = sqlx::query(
            r#"UPDATE qtss_v2_detections
                   SET state = 'confirmed',
                       confidence = $2,
                       channel_scores = $3,
                       validated_at = $4
                 WHERE id = $1"#,
        )
        .bind(id)
        .bind(confidence)
        .bind(channel_scores)
        .bind(validated_at)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Chart overlay read path: latest N detections for one
    /// (exchange, symbol, timeframe).
    ///
    /// DISTINCT ON (family, subkind) collapses the Forming → Confirmed
    /// lifecycle of the same wave into a *single* row (the most recent
    /// state). Without it the chart renders both overlays and the user
    /// cannot tell which forecast is live — see post-Faz 8.0 backlog
    /// item #2. Invalidated rows are filtered out so stale forecasts
    /// don't linger on the chart.
    pub async fn list_for_chart(
        &self,
        exchange: &str,
        symbol: &str,
        timeframe: &str,
        limit: i64,
    ) -> Result<Vec<DetectionRow>, StorageError> {
        let rows = sqlx::query_as::<_, DetectionRow>(
            r#"SELECT * FROM (
                  SELECT DISTINCT ON (family, subkind)
                         id, detected_at, exchange, symbol, timeframe,
                         family, subkind, state, structural_score, confidence,
                         invalidation_price, anchors, regime, channel_scores,
                         raw_meta, validated_at, mode, created_at, updated_at
                    FROM qtss_v2_detections
                   WHERE exchange = $1
                     AND symbol   = $2
                     AND timeframe = $3
                     AND state <> 'invalidated'
                   ORDER BY family, subkind, detected_at DESC
               ) latest
               ORDER BY detected_at DESC
               LIMIT $4"#,
        )
        .bind(exchange)
        .bind(symbol)
        .bind(timeframe)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Aggregate validation outcomes per `(family, subkind, timeframe)`
    /// for the historical hit-rate channel. Validator-pass proxy:
    ///
    /// * a row with `confidence IS NOT NULL` is treated as a positive
    ///   sample (the validator agreed it was worth surfacing),
    /// * a row whose state was flipped to `invalidated` is a negative.
    ///
    /// This is intentionally a *cheap* proxy until a real
    /// target-vs-stop tracker lands; the channel's `min_samples` floor
    /// keeps the early/noisy regime out of the blend.
    pub async fn historical_outcome_counts(
        &self,
    ) -> Result<Vec<HistoricalOutcomeRow>, StorageError> {
        let rows = sqlx::query_as::<_, HistoricalOutcomeRow>(
            // Reads from the MATERIALIZED VIEW (migration 0107) refreshed
            // by the worker — scanning the 11M-row base table on every
            // validator tick was costing ≈1.2 s.
            r#"SELECT family,
                      subkind,
                      timeframe,
                      validated_count,
                      invalidated_count
                 FROM qtss_v2_detection_outcome_stats"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// HTF confluence read path: recent rows on a *different* timeframe
    /// for the same `(exchange, symbol)`. The validator filters these
    /// down to strictly-higher-TF rows in-memory using
    /// `Timeframe::seconds()` — keeping the SQL agnostic of the TF
    /// ordering avoids hardcoding it in two places (CLAUDE.md #1).
    /// Only confirmed/forming rows with a non-null confidence are
    /// returned so we don't blend invalidated noise back in.
    pub async fn list_recent_for_symbol_htf(
        &self,
        exchange: &str,
        symbol: &str,
        exclude_timeframe: &str,
        limit: i64,
    ) -> Result<Vec<DetectionRow>, StorageError> {
        let rows = sqlx::query_as::<_, DetectionRow>(
            r#"SELECT id, detected_at, exchange, symbol, timeframe,
                      family, subkind, state, structural_score, confidence,
                      invalidation_price, anchors, regime, channel_scores,
                      raw_meta, validated_at, mode, created_at, updated_at
                 FROM qtss_v2_detections
                WHERE exchange = $1
                  AND symbol   = $2
                  AND timeframe <> $3
                  AND state <> 'invalidated'
                  AND confidence IS NOT NULL
                ORDER BY detected_at DESC
                LIMIT $4"#,
        )
        .bind(exchange)
        .bind(symbol)
        .bind(exclude_timeframe)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Validator loop read path: forming detections that have not yet
    /// been scored. Newest first so a backlog drains in a useful order.
    pub async fn list_pending_validation(
        &self,
        limit: i64,
    ) -> Result<Vec<DetectionRow>, StorageError> {
        let rows = sqlx::query_as::<_, DetectionRow>(
            r#"SELECT id, detected_at, exchange, symbol, timeframe,
                      family, subkind, state, structural_score, confidence,
                      invalidation_price, anchors, regime, channel_scores,
                      raw_meta, validated_at, mode, created_at, updated_at
                 FROM qtss_v2_detections
                WHERE confidence IS NULL
                  AND state = 'forming'
                ORDER BY detected_at DESC
                LIMIT $1"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Stale forming sweeper — Faz 7 Adım 10. Flips any row that has
    /// been sitting in `forming` longer than `older_than_secs` to
    /// `invalidated` so it disappears from the live overlay. Returns
    /// the number of rows touched so the caller can log a single
    /// rolled-up line per pass.
    pub async fn invalidate_stale_forming(
        &self,
        older_than_secs: i64,
    ) -> Result<u64, StorageError> {
        let res = sqlx::query(
            r#"UPDATE qtss_v2_detections
                   SET state = 'invalidated'
                 WHERE state = 'forming'
                   AND detected_at < now() - make_interval(secs => $1)"#,
        )
        .bind(older_than_secs as f64)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Price-breach sweep — minimal projection of `(id, family,
    /// subkind, invalidation_price)` for every `forming` row of one
    /// (exchange, symbol, timeframe). The orchestrator iterates this in
    /// Rust to classify direction and compare against the latest close,
    /// then calls [`Self::update_state`] for each breached row. Kept as
    /// a thin SELECT (no JOINs) so the per-symbol pre-pass is cheap.
    pub async fn list_forming_for_symbol(
        &self,
        exchange: &str,
        symbol: &str,
        timeframe: &str,
    ) -> Result<Vec<FormingRow>, StorageError> {
        let rows = sqlx::query_as::<_, FormingRow>(
            r#"SELECT id, family, subkind, invalidation_price, anchors
                 FROM qtss_v2_detections
                WHERE exchange = $1
                  AND symbol   = $2
                  AND timeframe = $3
                  AND state    = 'forming'"#,
        )
        .bind(exchange)
        .bind(symbol)
        .bind(timeframe)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Live revision: when a fresh detection lands for the same
    /// (exchange, symbol, timeframe, family, subkind), retire all the
    /// older `forming` rows for that key by flipping them to
    /// `invalidated`. Without this the chart sees stacked overlays of
    /// the *same* wave at different lifecycle moments — see post-Faz
    /// 8.0 backlog item #2.
    pub async fn supersede_previous_forming(
        &self,
        exchange: &str,
        symbol: &str,
        timeframe: &str,
        family: &str,
        subkind: &str,
        keep_id: Uuid,
    ) -> Result<u64, StorageError> {
        let res = sqlx::query(
            r#"UPDATE qtss_v2_detections
                   SET state = 'invalidated'
                 WHERE exchange = $1
                   AND symbol   = $2
                   AND timeframe = $3
                   AND family   = $4
                   AND subkind  = $5
                   AND state IN ('forming', 'confirmed')
                   AND id       <> $6"#,
        )
        .bind(exchange)
        .bind(symbol)
        .bind(timeframe)
        .bind(family)
        .bind(subkind)
        .bind(keep_id)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Detections feed read path. All filters are optional so the
    /// Detections panel can compose them dynamically.
    pub async fn list_filtered(
        &self,
        f: DetectionFilter<'_>,
    ) -> Result<Vec<DetectionRow>, StorageError> {
        let rows = sqlx::query_as::<_, DetectionRow>(
            r#"SELECT id, detected_at, exchange, symbol, timeframe,
                      family, subkind, state, structural_score, confidence,
                      invalidation_price, anchors, regime, channel_scores,
                      raw_meta, validated_at, mode, created_at, updated_at
                 FROM qtss_v2_detections
                WHERE ($1::text IS NULL OR exchange  = $1)
                  AND ($2::text IS NULL OR symbol    = $2)
                  AND ($3::text IS NULL OR timeframe = $3)
                  AND ($4::text IS NULL OR family    = $4)
                  AND ($5::text IS NULL OR state     = $5)
                  AND ($6::text IS NULL OR mode      = $6)
                ORDER BY detected_at DESC
                LIMIT $7"#,
        )
        .bind(f.exchange)
        .bind(f.symbol)
        .bind(f.timeframe)
        .bind(f.family)
        .bind(f.state)
        .bind(f.mode)
        .bind(f.limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

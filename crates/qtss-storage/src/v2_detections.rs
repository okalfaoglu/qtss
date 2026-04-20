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
    /// Aşama 5 — explicit overlay geometry. NULL for legacy rows; the
    /// chart endpoint passes through unchanged and frontend decides
    /// whether to dispatch on it or fall back to anchor-derived render.
    pub render_geometry: Option<Json>,
    pub render_style: Option<String>,
    pub render_labels: Option<Json>,
    /// Faz 12 — detectors that consume `PivotTree` tag the level they
    /// ran on (`L0`..`L3`). NULL for detectors that don't depend on
    /// pivots (TBM, candle patterns, gaps).
    pub pivot_level: Option<String>,
    /// Faz 12.R — populated only by the backtest-chart query (LEFT JOIN
    /// `qtss_v2_detection_outcomes`). NULL for live rows and for
    /// backtest rows that haven't been evaluated yet.
    #[sqlx(default)]
    pub outcome: Option<String>,
    #[sqlx(default)]
    pub outcome_pnl_pct: Option<f32>,
    #[sqlx(default)]
    pub outcome_entry_price: Option<f32>,
    #[sqlx(default)]
    pub outcome_exit_price: Option<f32>,
    #[sqlx(default)]
    pub outcome_close_reason: Option<String>,
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
    /// Aşama 5 — optional explicit overlay geometry. Detectors that opt
    /// in pass a `{ "kind": ..., "payload": ... }` object here; legacy
    /// detectors leave it None and the chart falls back to anchors.
    pub render_geometry: Option<Json>,
    pub render_style: Option<&'a str>,
    pub render_labels: Option<Json>,
    /// Faz 12 — pivot level tag. See `DetectionRow::pivot_level`.
    pub pivot_level: Option<&'a str>,
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
                   invalidation_price, anchors, regime, raw_meta, mode,
                   render_geometry, render_style, render_labels,
                   pivot_level
               ) VALUES (
                   $1, $2, $3, $4, $5,
                   $6, $7, $8, $9,
                   $10, $11, $12, $13, $14,
                   $15, $16, $17,
                   $18
               )
               RETURNING id, detected_at, exchange, symbol, timeframe,
                         family, subkind, state, structural_score, confidence,
                         invalidation_price, anchors, regime, channel_scores,
                         raw_meta, validated_at, mode, created_at, updated_at,
                         render_geometry, render_style, render_labels,
                         pivot_level"#,
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
        .bind(d.render_geometry)
        .bind(d.render_style)
        .bind(d.render_labels)
        .bind(d.pivot_level)
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

    /// TBM single-record upsert support (Faz 9 follow-up): refresh the
    /// anchor block, invalidation price, structural score and raw_meta
    /// of an existing forming row in place. Used when a TBM setup's
    /// argmin/argmax moves to a fresh extremum — instead of invalidating
    /// the old row and inserting a new one (which produces visual
    /// duplicates in the Detections panel), we mutate the same row so
    /// each logical setup occupies exactly one DB row across its
    /// lifetime. See bug: tbm duplicate rows.
    pub async fn update_anchor_projection(
        &self,
        id: Uuid,
        structural_score: f32,
        invalidation_price: Decimal,
        anchors: Json,
        raw_meta: Json,
    ) -> Result<u64, StorageError> {
        let res = sqlx::query(
            r#"UPDATE qtss_v2_detections
                   SET structural_score   = $2,
                       invalidation_price = $3,
                       anchors            = $4,
                       raw_meta           = $5,
                       updated_at         = NOW()
                 WHERE id = $1"#,
        )
        .bind(id)
        .bind(structural_score)
        .bind(invalidation_price)
        .bind(anchors)
        .bind(raw_meta)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// TBM single-record support: return every non-invalidated row for a
    /// given (exchange, symbol, timeframe, family, subkind) key, newest
    /// first. The TBM loop uses this to locate the *one* open record for
    /// an upsert; if more than one comes back (legacy duplicates from
    /// before the upsert path landed) the caller keeps the freshest and
    /// invalidates the rest.
    pub async fn list_open_by_key(
        &self,
        exchange: &str,
        symbol: &str,
        timeframe: &str,
        family: &str,
        subkind: &str,
    ) -> Result<Vec<DetectionRow>, StorageError> {
        let rows = sqlx::query_as::<_, DetectionRow>(
            r#"SELECT id, detected_at, exchange, symbol, timeframe,
                      family, subkind, state, structural_score, confidence,
                      invalidation_price, anchors, regime, channel_scores,
                      raw_meta, validated_at, mode, created_at, updated_at,
                      render_geometry, render_style, render_labels,
                      pivot_level
                 FROM qtss_v2_detections
                WHERE exchange  = $1
                  AND symbol    = $2
                  AND timeframe = $3
                  AND family    = $4
                  AND subkind   = $5
                  AND state    <> 'invalidated'
                ORDER BY detected_at DESC"#,
        )
        .bind(exchange)
        .bind(symbol)
        .bind(timeframe)
        .bind(family)
        .bind(subkind)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
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
        // Live-only view keeps the legacy collapse: one row per
        // (family, subkind) so Forming→Confirmed lifecycle doesn't
        // double-render.
        let rows = sqlx::query_as::<_, DetectionRow>(
            r#"SELECT * FROM (
                  SELECT DISTINCT ON (family, subkind)
                         id, detected_at, exchange, symbol, timeframe,
                         family, subkind, state, structural_score, confidence,
                         invalidation_price, anchors, regime, channel_scores,
                         raw_meta, validated_at, mode, created_at, updated_at,
                      render_geometry, render_style, render_labels,
                      pivot_level
                    FROM qtss_v2_detections
                   WHERE exchange = $1
                     AND symbol   = $2
                     AND timeframe = $3
                     AND state <> 'invalidated'
                     AND mode <> 'backtest'
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

    /// Faz 12 — backtest harmonic overlays for the chart.
    ///
    /// Unlike `list_for_chart`, this does NOT collapse across
    /// `pivot_level`: the backtest sweep emits many detections per
    /// (family, subkind) across L0..L3 and we want the operator to
    /// see every one so the level-toggle comparison is meaningful.
    /// Rows are filtered by `pivot_level IN (...)` from the request
    /// so only enabled levels cross the wire.
    pub async fn list_backtest_for_chart(
        &self,
        exchange: &str,
        symbol: &str,
        timeframe: &str,
        levels: &[String],
        limit: i64,
    ) -> Result<Vec<DetectionRow>, StorageError> {
        if levels.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, DetectionRow>(
            r#"SELECT d.id, d.detected_at, d.exchange, d.symbol, d.timeframe,
                      d.family, d.subkind, d.state, d.structural_score, d.confidence,
                      d.invalidation_price, d.anchors, d.regime, d.channel_scores,
                      d.raw_meta, d.validated_at, d.mode, d.created_at, d.updated_at,
                      d.render_geometry, d.render_style, d.render_labels,
                      d.pivot_level,
                      o.outcome AS outcome,
                      o.pnl_pct AS outcome_pnl_pct,
                      o.entry_price AS outcome_entry_price,
                      o.exit_price AS outcome_exit_price,
                      o.close_reason AS outcome_close_reason
                 FROM qtss_v2_detections d
                 LEFT JOIN qtss_v2_detection_outcomes o ON o.detection_id = d.id
                WHERE d.exchange  = $1
                  AND d.symbol    = $2
                  AND d.timeframe = $3
                  AND d.mode      = 'backtest'
                  AND d.pivot_level = ANY($4)
                ORDER BY d.detected_at DESC
                LIMIT $5"#,
        )
        .bind(exchange)
        .bind(symbol)
        .bind(timeframe)
        .bind(levels)
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
                      raw_meta, validated_at, mode, created_at, updated_at,
                      render_geometry, render_style, render_labels,
                      pivot_level
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
                      raw_meta, validated_at, mode, created_at, updated_at,
                      render_geometry, render_style, render_labels,
                      pivot_level
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
                      raw_meta, validated_at, mode, created_at, updated_at,
                      render_geometry, render_style, render_labels,
                      pivot_level
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

    /// Faz 9C — confirmed backtest detections that have NOT yet been
    /// converted into a `qtss_setups` row. Feeds `backtest_setup_loop`,
    /// which arms setups directly (bypassing the live confluence gate —
    /// backtest detections are historical, so live confluence rows
    /// don't exist for their timestamps). LEFT JOIN on
    /// `qtss_setups.detection_id` is the "not yet armed" filter.
    pub async fn list_backtest_unset_detections(
        &self,
        limit: i64,
    ) -> Result<Vec<DetectionRow>, StorageError> {
        let rows = sqlx::query_as::<_, DetectionRow>(
            r#"SELECT d.id, d.detected_at, d.exchange, d.symbol, d.timeframe,
                      d.family, d.subkind, d.state, d.structural_score, d.confidence,
                      d.invalidation_price, d.anchors, d.regime, d.channel_scores,
                      d.raw_meta, d.validated_at, d.mode, d.created_at, d.updated_at,
                      d.render_geometry, d.render_style, d.render_labels,
                      d.pivot_level
                 FROM qtss_v2_detections d
                 LEFT JOIN qtss_setups s ON s.detection_id = d.id
                WHERE d.state = 'confirmed'
                  AND d.mode  = 'backtest'
                  AND s.detection_id IS NULL
                ORDER BY d.detected_at ASC
                LIMIT $1"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

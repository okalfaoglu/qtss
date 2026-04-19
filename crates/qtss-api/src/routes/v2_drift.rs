//! Faz 9B — Drift dashboard API.
//!
//! Surfaces the PSI-drift/breaker tables (migration 0169 sections F + G) to
//! the GUI so operators stop poking at them with raw SQL. Three reads +
//! one mutation:
//!
//!   * `GET  /v2/drift/snapshots`            — latest PSI per feature
//!   * `GET  /v2/drift/timeline?feature=X`   — PSI time series for one feat.
//!   * `GET  /v2/drift/breakers?open=true`   — breaker events (open-only or all recent)
//!   * `POST /v2/drift/breakers/:id/resolve` — close an open event
//!
//! CLAUDE.md #2: PSI band thresholds (`warn_threshold`,
//! `critical_threshold`) are resolved from `system_config.ai.drift.*`
//! rather than hardcoded, so when the sidecar's classifier is retuned
//! the GUI colors follow without a deploy.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use qtss_storage::resolve_system_f64;

use crate::error::ApiError;
use crate::state::SharedState;

// ---------- payloads ----------

#[derive(Debug, Serialize)]
pub struct DriftBands {
    pub warn: f64,
    pub critical: f64,
}

#[derive(Debug, Serialize)]
pub struct DriftFeature {
    pub feature_name: String,
    pub model_version: String,
    pub psi: f64,
    pub status: String, // "ok" | "warn" | "critical"
    pub computed_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct DriftSnapshots {
    pub generated_at: DateTime<Utc>,
    pub bands: DriftBands,
    pub features: Vec<DriftFeature>,
}

#[derive(Debug, Serialize)]
pub struct DriftTimelinePoint {
    pub psi: f64,
    pub computed_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct DriftTimeline {
    pub feature_name: String,
    pub bands: DriftBands,
    pub points: Vec<DriftTimelinePoint>,
}

#[derive(Debug, Serialize)]
pub struct BreakerEvent {
    pub id: Uuid,
    pub fired_at: DateTime<Utc>,
    pub model_id: Uuid,
    pub model_version: Option<String>,
    pub action: String,
    pub reason: String,
    pub critical_features: serde_json::Value,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolved_by: Option<String>,
    pub resolution_note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BreakerList {
    pub generated_at: DateTime<Utc>,
    pub events: Vec<BreakerEvent>,
}

#[derive(Debug, Deserialize)]
pub struct SnapshotsQuery {
    /// Optional model_version filter. Omitted → latest version per feature.
    pub model_version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TimelineQuery {
    pub feature: String,
    /// Window in hours (default 168 = 1 week).
    pub hours: Option<i64>,
    pub model_version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BreakerListQuery {
    /// true → only unresolved events; false/omitted → last N regardless.
    pub open: Option<bool>,
    /// Limit when not filtering to open. Default 50.
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ResolveBody {
    pub resolved_by: String,
    pub resolution_note: Option<String>,
}

// ---------- router ----------

pub fn v2_drift_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/drift/snapshots", get(snapshots))
        .route("/v2/drift/timeline", get(timeline))
        .route("/v2/drift/breakers", get(breakers))
        .route("/v2/drift/breakers/{id}/resolve", post(resolve_breaker))
        .route("/v2/drift/calibration", get(calibration))
}

// ---------- calibration (Kalem G) ----------
//
// A calibrated classifier's mean predicted probability inside a score
// bucket equals the realized win-rate in that bucket. Big gaps → the
// model is mis-scored, rerank with Platt/isotonic. This endpoint
// returns 10-bin calibration data joined to closed setups:
//   prediction.score  → bucket
//   setups.state LIKE 'closed_win'  → positive outcome
// Brier score is reported in aggregate (lower = better).

#[derive(Debug, Deserialize)]
pub struct CalibrationQuery {
    pub model_version: Option<String>,
    /// Window in days for the join. Default 30, max 365.
    pub days: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CalibrationBin {
    pub lo: f64,
    pub hi: f64,
    pub n: i64,
    pub mean_predicted: f64,
    pub realized_win_rate: f64,
    pub gap: f64, // realized - mean_predicted; signed
}

#[derive(Debug, Serialize)]
pub struct CalibrationReport {
    pub generated_at: DateTime<Utc>,
    pub model_version: Option<String>,
    pub days: i64,
    pub n_total: i64,
    pub n_positive: i64,
    pub brier: f64,
    pub bins: Vec<CalibrationBin>,
}

async fn calibration(
    State(st): State<SharedState>,
    Query(q): Query<CalibrationQuery>,
) -> Result<Json<CalibrationReport>, ApiError> {
    let days = q.days.unwrap_or(30).clamp(1, 365);

    // Bucket width fixed at 0.1 → 10 bins. Width-in-config would be
    // overkill for an observability chart; deeper calibration work
    // happens in the trainer pipeline, not the GUI.
    //
    // The join keys on `setup_id` so only scored setups that actually
    // closed contribute. Decisions like 'shadow'/'suppress' without an
    // opened setup are excluded — they have no ground-truth label.
    let rows: Vec<(f64, bool)> = sqlx::query_as(
        r#"SELECT p.score::double precision AS score,
                  s.state LIKE 'closed_win%' AS won
             FROM qtss_ml_predictions p
             JOIN qtss_setups s ON s.id = p.setup_id
            WHERE s.closed_at IS NOT NULL
              AND s.state LIKE 'closed%'
              AND p.inference_ts > now() - ($1 || ' days')::interval
              AND ($2::text IS NULL OR p.model_version = $2)"#,
    )
    .bind(days.to_string())
    .bind(q.model_version.as_deref())
    .fetch_all(&st.pool)
    .await
    .map_err(|e| ApiError::internal(format!("calibration query: {e}")))?;

    let n_total = rows.len() as i64;
    let n_positive = rows.iter().filter(|(_, w)| *w).count() as i64;

    // Brier = mean((p - y)^2).
    let brier = if rows.is_empty() {
        0.0
    } else {
        rows.iter()
            .map(|(p, w)| {
                let y = if *w { 1.0 } else { 0.0 };
                (p - y).powi(2)
            })
            .sum::<f64>()
            / rows.len() as f64
    };

    // Bucketize.
    let mut buckets: Vec<(f64, i64, i64)> = (0..10).map(|_| (0.0, 0, 0)).collect();
    for (score, won) in &rows {
        let mut idx = (*score * 10.0).floor() as usize;
        if idx >= 10 {
            idx = 9;
        }
        buckets[idx].0 += score;
        buckets[idx].1 += 1;
        if *won {
            buckets[idx].2 += 1;
        }
    }

    let bins: Vec<CalibrationBin> = buckets
        .into_iter()
        .enumerate()
        .map(|(i, (sum_p, n, n_win))| {
            let lo = i as f64 / 10.0;
            let hi = (i as f64 + 1.0) / 10.0;
            let mean_predicted = if n > 0 { sum_p / n as f64 } else { 0.0 };
            let realized = if n > 0 { n_win as f64 / n as f64 } else { 0.0 };
            CalibrationBin {
                lo,
                hi,
                n,
                mean_predicted,
                realized_win_rate: realized,
                gap: realized - mean_predicted,
            }
        })
        .collect();

    Ok(Json(CalibrationReport {
        generated_at: Utc::now(),
        model_version: q.model_version,
        days,
        n_total,
        n_positive,
        brier,
        bins,
    }))
}

// ---------- helpers ----------

async fn fetch_bands(pool: &sqlx::PgPool) -> DriftBands {
    // System_config uses JSON values; resolve_system_f64 handles both
    // number and stringified-number cases. Defaults mirror sidecar.
    let warn =
        resolve_system_f64(pool, "ai", "drift.psi_warn_threshold", "", 0.10).await;
    let critical =
        resolve_system_f64(pool, "ai", "drift.psi_critical_threshold", "", 0.25).await;
    DriftBands { warn, critical }
}

fn classify(psi: f64, bands: &DriftBands) -> &'static str {
    // Guard ladder kept shallow (CLAUDE.md #1) — three states, two thresholds.
    if psi >= bands.critical {
        return "critical";
    }
    if psi >= bands.warn {
        return "warn";
    }
    "ok"
}

// ---------- handlers ----------

async fn snapshots(
    State(st): State<SharedState>,
    Query(q): Query<SnapshotsQuery>,
) -> Result<Json<DriftSnapshots>, ApiError> {
    let bands = fetch_bands(&st.pool).await;

    // DISTINCT ON picks the latest row per feature, optionally pinned to
    // a model_version so operators can compare an older active model
    // with the freshly-trained shadow. Indexed by
    // `(feature_name, computed_at DESC)`.
    let rows: Vec<(String, String, f32, DateTime<Utc>)> = sqlx::query_as(
        r#"SELECT DISTINCT ON (feature_name)
                  feature_name, model_version, psi, computed_at
             FROM qtss_ml_drift_snapshots
            WHERE ($1::text IS NULL OR model_version = $1)
            ORDER BY feature_name, computed_at DESC"#,
    )
    .bind(q.model_version.as_deref())
    .fetch_all(&st.pool)
    .await
    .map_err(|e| ApiError::internal(format!("drift snapshots query: {e}")))?;

    let mut features: Vec<DriftFeature> = rows
        .into_iter()
        .map(|(feature_name, model_version, psi, computed_at)| {
            let psi = psi as f64;
            let status = classify(psi, &bands).to_string();
            DriftFeature {
                feature_name,
                model_version,
                psi,
                status,
                computed_at,
            }
        })
        .collect();
    // Worst first — operator scans the hot rows at the top.
    features.sort_by(|a, b| b.psi.partial_cmp(&a.psi).unwrap_or(std::cmp::Ordering::Equal));

    Ok(Json(DriftSnapshots {
        generated_at: Utc::now(),
        bands,
        features,
    }))
}

async fn timeline(
    State(st): State<SharedState>,
    Query(q): Query<TimelineQuery>,
) -> Result<Json<DriftTimeline>, ApiError> {
    let hours = q.hours.unwrap_or(168).clamp(1, 24 * 30);
    let bands = fetch_bands(&st.pool).await;

    let rows: Vec<(f32, DateTime<Utc>)> = sqlx::query_as(
        r#"SELECT psi, computed_at
             FROM qtss_ml_drift_snapshots
            WHERE feature_name = $1
              AND computed_at > now() - ($2 || ' hours')::interval
              AND ($3::text IS NULL OR model_version = $3)
            ORDER BY computed_at ASC"#,
    )
    .bind(&q.feature)
    .bind(hours.to_string())
    .bind(q.model_version.as_deref())
    .fetch_all(&st.pool)
    .await
    .map_err(|e| ApiError::internal(format!("drift timeline query: {e}")))?;

    let points = rows
        .into_iter()
        .map(|(psi, computed_at)| DriftTimelinePoint {
            psi: psi as f64,
            computed_at,
        })
        .collect();

    Ok(Json(DriftTimeline {
        feature_name: q.feature,
        bands,
        points,
    }))
}

async fn breakers(
    State(st): State<SharedState>,
    Query(q): Query<BreakerListQuery>,
) -> Result<Json<BreakerList>, ApiError> {
    let open_only = q.open.unwrap_or(false);
    let limit = q.limit.unwrap_or(50).clamp(1, 500);

    let rows: Vec<(
        Uuid,
        DateTime<Utc>,
        Uuid,
        Option<String>,
        String,
        String,
        serde_json::Value,
        Option<DateTime<Utc>>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        r#"SELECT b.id, b.fired_at, b.model_id, m.model_version,
                  b.action, b.reason, b.critical_features,
                  b.resolved_at, b.resolved_by, b.resolution_note
             FROM qtss_ml_breaker_events b
        LEFT JOIN qtss_models m ON m.id = b.model_id
            WHERE (NOT $1 OR b.resolved_at IS NULL)
            ORDER BY b.fired_at DESC
            LIMIT $2"#,
    )
    .bind(open_only)
    .bind(limit)
    .fetch_all(&st.pool)
    .await
    .map_err(|e| ApiError::internal(format!("breaker list query: {e}")))?;

    let events = rows
        .into_iter()
        .map(
            |(
                id,
                fired_at,
                model_id,
                model_version,
                action,
                reason,
                critical_features,
                resolved_at,
                resolved_by,
                resolution_note,
            )| BreakerEvent {
                id,
                fired_at,
                model_id,
                model_version,
                action,
                reason,
                critical_features,
                resolved_at,
                resolved_by,
                resolution_note,
            },
        )
        .collect();

    Ok(Json(BreakerList {
        generated_at: Utc::now(),
        events,
    }))
}

async fn resolve_breaker(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ResolveBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if body.resolved_by.trim().is_empty() {
        return Err(ApiError::bad_request("resolved_by is required"));
    }
    let res = sqlx::query(
        r#"UPDATE qtss_ml_breaker_events
              SET resolved_at    = COALESCE(resolved_at, now()),
                  resolved_by    = $2,
                  resolution_note = $3
            WHERE id = $1"#,
    )
    .bind(id)
    .bind(&body.resolved_by)
    .bind(body.resolution_note.as_deref())
    .execute(&st.pool)
    .await
    .map_err(|e| ApiError::internal(format!("resolve breaker: {e}")))?;

    if res.rows_affected() == 0 {
        return Err(ApiError::not_found("breaker event not found"));
    }
    Ok(Json(serde_json::json!({
        "ok": true,
        "id": id,
    })))
}

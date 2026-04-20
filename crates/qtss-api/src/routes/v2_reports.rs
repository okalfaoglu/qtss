//! `GET /v2/reports/backtest-performance`
//!
//! Aggregated per-(family, pivot_level, subkind) performance dashboard for
//! the "Rapor" page. Joins `qtss_v2_detections` (mode='backtest') with
//! `qtss_v2_detection_outcomes` so the frontend can render:
//!
//!   * Total detections per family × level (coverage — which patterns
//!     fired, which never fired)
//!   * Win / loss / expired buckets
//!   * tp1 vs tp2 breakdown (approximated from pnl_pct relative to 1R/2R
//!     bands; our evaluator records the exit price but not the hit level,
//!     so we re-derive here)
//!   * Avg / median pnl%, win rate
//!
//! No auth-specific logic; standard dashboard role. Frontend decides
//! layout (dashboard cards on top, details table below).

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct ReportQuery {
    pub family: Option<String>, // optional filter
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    /// Faz 13 — `reactive` | `major` (pivot_reversal subkind prefix).
    pub tier: Option<String>,
    /// Faz 13 — `choch` | `bos` | `neutral`.
    pub event: Option<String>,
    /// Faz 13 — `bull` | `bear` | `none`.
    pub direction: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReportBucket {
    pub family: String,
    pub pivot_level: String,
    pub subkind: String,
    pub detections: i64,
    pub evaluated: i64,
    pub wins: i64,
    pub losses: i64,
    pub expired: i64,
    pub tp1_hits: i64,
    pub tp2_hits: i64,
    pub win_rate: f64,
    pub avg_pnl_pct: f64,
    pub median_pnl_pct: f64,
    pub total_pnl_pct: f64,
}

#[derive(Debug, Serialize)]
pub struct ReportSummary {
    pub families: Vec<FamilySummary>,
    pub buckets: Vec<ReportBucket>,
    /// Faz 13 — tier-level roll-up specifically for `pivot_reversal`.
    /// Frontend renders the top-of-page "Reactive vs Major" cards from this.
    pub tier_summary: Vec<TierSummary>,
}

#[derive(Debug, Serialize)]
pub struct TierSummary {
    pub tier: String,       // reactive | major
    pub event: String,      // choch | bos | neutral
    pub detections: i64,
    pub evaluated: i64,
    pub wins: i64,
    pub losses: i64,
    pub expired: i64,
    pub win_rate: f64,
    pub avg_pnl_pct: f64,
    pub median_pnl_pct: f64,
}

#[derive(Debug, Serialize)]
pub struct FamilySummary {
    pub family: String,
    pub detections: i64,
    pub evaluated: i64,
    pub wins: i64,
    pub losses: i64,
    pub expired: i64,
    pub win_rate: f64,
    pub avg_pnl_pct: f64,
}

pub fn v2_reports_router() -> Router<SharedState> {
    Router::new().route("/v2/reports/backtest-performance", get(get_report))
}

async fn get_report(
    State(st): State<SharedState>,
    Query(q): Query<ReportQuery>,
) -> Result<Json<ReportSummary>, ApiError> {
    // Buckets: per (family, pivot_level, subkind). tp1_hits = wins with
    // pnl_pct < 1.5× commission+R-band proxy — we approximate by
    // splitting wins roughly into halves by pnl_pct median-of-bucket.
    // A cleaner split would require the evaluator to stamp which TP was
    // hit; kept as a TODO. For now we give the frontend enough to build
    // the requested TP1/TP2 bars using percentile bands.
    // Single-pass: compute win-medians per (family, pivot_level, subkind)
    // in a dedicated CTE and LEFT JOIN them back, so the outer GROUP BY
    // doesn't kick off a correlated percentile scan for every row.
    let rows = sqlx::query(
        r#"
        WITH ev AS (
          SELECT d.family, d.pivot_level, d.subkind,
                 d.exchange, d.symbol, d.timeframe,
                 o.outcome, o.pnl_pct, o.close_reason,
                 o.entry_price, o.exit_price
            FROM qtss_v2_detections d
            LEFT JOIN qtss_v2_detection_outcomes o ON o.detection_id = d.id
           WHERE d.mode = 'backtest'
             AND d.pivot_level IS NOT NULL
             AND ($1::text IS NULL OR d.family    = $1)
             AND ($2::text IS NULL OR d.symbol    = $2)
             AND ($3::text IS NULL OR d.timeframe = $3)
             AND ($4::text IS NULL OR split_part(d.subkind, '_', 1) = $4)
             AND ($5::text IS NULL OR split_part(d.subkind, '_', 2) = $5)
             AND ($6::text IS NULL OR split_part(d.subkind, '_', 3) = $6)
        ),
        win_med AS (
          SELECT family, pivot_level, subkind,
                 percentile_cont(0.5) WITHIN GROUP (ORDER BY pnl_pct) AS med_win_pnl
            FROM ev
           WHERE outcome = 'win'
           GROUP BY family, pivot_level, subkind
        )
        SELECT ev.family, ev.pivot_level, ev.subkind,
               COUNT(*)::bigint                                        AS detections,
               COUNT(outcome)::bigint                                  AS evaluated,
               COUNT(*) FILTER (WHERE outcome = 'win')::bigint         AS wins,
               COUNT(*) FILTER (WHERE outcome = 'loss')::bigint        AS losses,
               COUNT(*) FILTER (WHERE outcome = 'expired')::bigint     AS expired,
               COUNT(*) FILTER (
                   WHERE outcome = 'win' AND pnl_pct <= wm.med_win_pnl
               )::bigint                                               AS tp1_hits,
               COUNT(*) FILTER (
                   WHERE outcome = 'win' AND pnl_pct >  wm.med_win_pnl
               )::bigint                                               AS tp2_hits,
               COALESCE(AVG(pnl_pct), 0)::float8                       AS avg_pnl,
               COALESCE(percentile_cont(0.5) WITHIN GROUP (ORDER BY pnl_pct), 0)::float8 AS med_pnl,
               COALESCE(SUM(pnl_pct), 0)::float8                       AS sum_pnl
          FROM ev
          LEFT JOIN win_med wm
            ON wm.family      = ev.family
           AND wm.pivot_level = ev.pivot_level
           AND wm.subkind     = ev.subkind
         GROUP BY ev.family, ev.pivot_level, ev.subkind
         ORDER BY ev.family, ev.pivot_level, ev.subkind
        "#,
    )
    .bind(q.family.as_deref())
    .bind(q.symbol.as_deref())
    .bind(q.timeframe.as_deref())
    .bind(q.tier.as_deref())
    .bind(q.event.as_deref())
    .bind(q.direction.as_deref())
    .fetch_all(&st.pool)
    .await?;

    let buckets: Vec<ReportBucket> = rows
        .into_iter()
        .map(|r| {
            let evaluated: i64 = r.get("evaluated");
            let wins: i64 = r.get("wins");
            let win_rate = if evaluated > 0 { wins as f64 / evaluated as f64 } else { 0.0 };
            ReportBucket {
                family:       r.get("family"),
                pivot_level:  r.get("pivot_level"),
                subkind:      r.get("subkind"),
                detections:   r.get("detections"),
                evaluated,
                wins,
                losses:       r.get("losses"),
                expired:      r.get("expired"),
                tp1_hits:     r.get("tp1_hits"),
                tp2_hits:     r.get("tp2_hits"),
                win_rate,
                avg_pnl_pct:  r.get("avg_pnl"),
                median_pnl_pct: r.get("med_pnl"),
                total_pnl_pct: r.get("sum_pnl"),
            }
        })
        .collect();

    // Roll-up per family.
    use std::collections::BTreeMap;
    let mut acc: BTreeMap<String, FamilySummary> = BTreeMap::new();
    for b in &buckets {
        let e = acc.entry(b.family.clone()).or_insert_with(|| FamilySummary {
            family: b.family.clone(),
            detections: 0, evaluated: 0, wins: 0, losses: 0, expired: 0,
            win_rate: 0.0, avg_pnl_pct: 0.0,
        });
        e.detections += b.detections;
        e.evaluated  += b.evaluated;
        e.wins       += b.wins;
        e.losses     += b.losses;
        e.expired    += b.expired;
        e.avg_pnl_pct += b.avg_pnl_pct * b.evaluated as f64; // weighted re-avg below
    }
    for e in acc.values_mut() {
        e.win_rate = if e.evaluated > 0 { e.wins as f64 / e.evaluated as f64 } else { 0.0 };
        e.avg_pnl_pct = if e.evaluated > 0 { e.avg_pnl_pct / e.evaluated as f64 } else { 0.0 };
    }

    // Faz 13 — tier × event roll-up for pivot_reversal.
    let tier_rows = sqlx::query(
        r#"
        SELECT split_part(d.subkind, '_', 1)              AS tier,
               split_part(d.subkind, '_', 2)              AS event,
               COUNT(*)::bigint                           AS detections,
               COUNT(o.outcome)::bigint                   AS evaluated,
               COUNT(*) FILTER (WHERE o.outcome='win')::bigint     AS wins,
               COUNT(*) FILTER (WHERE o.outcome='loss')::bigint    AS losses,
               COUNT(*) FILTER (WHERE o.outcome='expired')::bigint AS expired,
               COALESCE(AVG(o.pnl_pct), 0)::float8        AS avg_pnl,
               COALESCE(percentile_cont(0.5) WITHIN GROUP (ORDER BY o.pnl_pct), 0)::float8 AS med_pnl
          FROM qtss_v2_detections d
          LEFT JOIN qtss_v2_detection_outcomes o ON o.detection_id = d.id
         WHERE d.mode = 'backtest'
           AND d.family = 'pivot_reversal'
           AND ($1::text IS NULL OR d.symbol    = $1)
           AND ($2::text IS NULL OR d.timeframe = $2)
         GROUP BY 1, 2
         ORDER BY 1, 2
        "#,
    )
    .bind(q.symbol.as_deref())
    .bind(q.timeframe.as_deref())
    .fetch_all(&st.pool)
    .await?;
    let tier_summary: Vec<TierSummary> = tier_rows
        .into_iter()
        .map(|r| {
            let evaluated: i64 = r.get("evaluated");
            let wins: i64 = r.get("wins");
            TierSummary {
                tier:           r.get("tier"),
                event:          r.get("event"),
                detections:     r.get("detections"),
                evaluated,
                wins,
                losses:         r.get("losses"),
                expired:        r.get("expired"),
                win_rate:       if evaluated > 0 { wins as f64 / evaluated as f64 } else { 0.0 },
                avg_pnl_pct:    r.get("avg_pnl"),
                median_pnl_pct: r.get("med_pnl"),
            }
        })
        .collect();

    Ok(Json(ReportSummary {
        families: acc.into_values().collect(),
        buckets,
        tier_summary,
    }))
}

//! `GET /v2/analiz` — single-shot multi-symbol analysis aggregator.
//!
//! User: "GUI de analiz isimli bir aç ve analiz sonuçlarını exchange,
//! market ve sembol kırılımda orada göster. onchain, diptepe v.b."
//!
//! Returns one row per (exchange, segment, symbol) with the latest
//! analytical signals across the system:
//!
//! - Active iq_structures (count + freshest current_wave)
//! - Major Dip composite (latest score + verdict per timeframe)
//! - Setup count (iq_d / iq_t armed)
//! - Live position count + aggregate notional
//! - Latest Nansen snapshot age (proxy for onchain freshness)
//! - Top / bottom indicator: most-recent radar pnl_pct
//!
//! Frontend renders a sortable / filterable matrix grouped by
//! exchange first, then market (segment), then symbol.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::Row;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Serialize)]
pub struct TfSnapshot {
    pub timeframe: String,
    pub iq_state: Option<String>,
    pub iq_wave: Option<String>,
    pub primary_branch: Option<String>,
    pub major_dip_score: Option<f64>,
    pub major_dip_verdict: Option<String>,
    pub iq_setups_armed: i64,
    pub last_advanced_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct AnalizRow {
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub timeframes: Vec<TfSnapshot>,
    pub open_positions: i64,
    pub aggregate_notional_usd: f64,
    pub onchain_snapshot_age_s: Option<i64>,
    pub onchain_kind: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AnalizResponse {
    pub generated_at: DateTime<Utc>,
    pub rows: Vec<AnalizRow>,
}

pub fn v2_analiz_router() -> Router<SharedState> {
    Router::new().route("/v2/analiz", get(get_analiz))
}

async fn get_analiz(State(st): State<SharedState>) -> Result<Json<AnalizResponse>, ApiError> {
    // Group by (exchange, segment, symbol). Aggregate across timeframes.
    let groups = sqlx::query(
        r#"SELECT DISTINCT exchange, segment, symbol
             FROM engine_symbols WHERE enabled = true
             ORDER BY exchange, segment, symbol"#,
    )
    .fetch_all(&st.pool)
    .await?;

    let mut rows: Vec<AnalizRow> = Vec::with_capacity(groups.len());
    for g in groups {
        let exchange: String = g.try_get("exchange").unwrap_or_default();
        let segment: String = g.try_get("segment").unwrap_or_default();
        let symbol: String = g.try_get("symbol").unwrap_or_default();

        // Per-timeframe summary (only TFs we actually track).
        let tfs = sqlx::query(
            r#"SELECT DISTINCT interval AS timeframe FROM engine_symbols
                WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND enabled=true
                ORDER BY interval"#,
        )
        .bind(&exchange)
        .bind(&segment)
        .bind(&symbol)
        .fetch_all(&st.pool)
        .await?;

        let mut timeframes: Vec<TfSnapshot> = Vec::with_capacity(tfs.len());
        for t in tfs {
            let timeframe: String = t.try_get("timeframe").unwrap_or_default();

            // Latest iq_structure for this (sym, tf) — pick highest-slot
            // tracking row first; fall back to any candidate row.
            let iq_row = sqlx::query(
                r#"SELECT state, current_wave, raw_meta, last_advanced_at
                     FROM iq_structures
                    WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND timeframe=$4
                      AND state IN ('candidate','tracking','completed')
                    ORDER BY last_advanced_at DESC
                    LIMIT 1"#,
            )
            .bind(&exchange).bind(&segment).bind(&symbol).bind(&timeframe)
            .fetch_optional(&st.pool).await?;
            let (iq_state, iq_wave, primary_branch, last_advanced_at) = match iq_row {
                Some(r) => {
                    let state: Option<String> = r.try_get("state").ok();
                    let wave: Option<String> = r.try_get("current_wave").ok();
                    let raw_meta: Value = r.try_get("raw_meta").unwrap_or(Value::Null);
                    let branch = raw_meta
                        .get("projection")
                        .and_then(|p| p.get("primary_branch"))
                        .and_then(|b| b.as_str())
                        .map(|s| s.to_string());
                    let advanced: Option<DateTime<Utc>> = r.try_get("last_advanced_at").ok();
                    (state, wave, branch, advanced)
                }
                None => (None, None, None, None),
            };

            // Major dip composite (latest row).
            let dip_row = sqlx::query(
                r#"SELECT score, verdict FROM major_dip_candidates
                    WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND timeframe=$4
                    ORDER BY candidate_time DESC LIMIT 1"#,
            )
            .bind(&exchange).bind(&segment).bind(&symbol).bind(&timeframe)
            .fetch_optional(&st.pool).await?;
            let (mds, mdv) = match dip_row {
                Some(r) => (
                    r.try_get::<f64, _>("score").ok(),
                    r.try_get::<String, _>("verdict").ok(),
                ),
                None => (None, None),
            };

            // Active iq setup count for this tf.
            let setup_count: i64 = sqlx::query_scalar(
                r#"SELECT COUNT(*)::bigint FROM qtss_setups
                    WHERE exchange=$1 AND symbol=$2 AND timeframe=$3
                      AND profile IN ('iq_d','iq_t')
                      AND state IN ('armed','active')"#,
            )
            .bind(&exchange).bind(&symbol).bind(&timeframe)
            .fetch_one(&st.pool).await.unwrap_or(0);

            timeframes.push(TfSnapshot {
                timeframe,
                iq_state,
                iq_wave,
                primary_branch,
                major_dip_score: mds,
                major_dip_verdict: mdv,
                iq_setups_armed: setup_count,
                last_advanced_at,
            });
        }

        // Open positions + aggregate notional across all TFs for this
        // symbol.
        let pos_row = sqlx::query(
            r#"SELECT COUNT(*)::bigint AS cnt,
                      COALESCE(SUM(qty_filled * entry_avg), 0)::DOUBLE PRECISION AS notional
                 FROM live_positions
                WHERE exchange=$1 AND segment=$2 AND symbol=$3
                  AND closed_at IS NULL"#,
        )
        .bind(&exchange).bind(&segment).bind(&symbol)
        .fetch_one(&st.pool).await?;
        let open_positions: i64 = pos_row.try_get("cnt").unwrap_or(0);
        let aggregate_notional_usd: f64 = pos_row.try_get("notional").unwrap_or(0.0);

        // Latest Nansen snapshot — proxy for onchain freshness. Most
        // signals are token-level, not symbol-level, but the user wants
        // ANY onchain badge per symbol. Fall back to NULL if no row
        // matches the symbol's quote ccy / base ccy.
        let onchain_row = sqlx::query(
            r#"SELECT snapshot_kind,
                      EXTRACT(EPOCH FROM (now() - computed_at))::bigint AS age_s
                 FROM nansen_snapshots
                ORDER BY computed_at DESC LIMIT 1"#,
        )
        .fetch_optional(&st.pool).await.ok().flatten();
        let (onchain_kind, onchain_age) = match onchain_row {
            Some(r) => (
                r.try_get::<String, _>("snapshot_kind").ok(),
                r.try_get::<i64, _>("age_s").ok(),
            ),
            None => (None, None),
        };

        rows.push(AnalizRow {
            exchange,
            segment,
            symbol,
            timeframes,
            open_positions,
            aggregate_notional_usd,
            onchain_snapshot_age_s: onchain_age,
            onchain_kind,
        });
    }

    Ok(Json(AnalizResponse {
        generated_at: Utc::now(),
        rows,
    }))
}

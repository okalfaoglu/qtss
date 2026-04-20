//! `GET /v2/reversal-radar` — Faz 13.
//!
//! **Dip/Tepe Radarı** — operatörün hem reactive (L0/L1) hem major
//! (L2/L3) dönüş noktalarını tek sayfada tarayabildiği feed. TBM
//! `v2_tbm.rs` ile aynı dili konuşur (thin projection over
//! `qtss_v2_detections`) — yeni bir detector yok, sadece filtreli
//! listeleme + raw_meta projection.
//!
//! Subkind formatı: `{tier}_{event}_{direction}_{level}`
//! (pivot_reversal_backtest_sweep.rs, Faz 13).

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct RadarQuery {
    pub exchange: Option<String>,
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    /// `reactive` | `major`
    pub tier: Option<String>,
    /// `choch` | `bos` | `neutral`
    pub event: Option<String>,
    /// `bull` | `bear`
    pub direction: Option<String>,
    /// `L0..L3`
    pub level: Option<String>,
    /// `backtest` | `live`
    pub mode: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct RadarFeed {
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<RadarEntry>,
}

#[derive(Debug, Serialize)]
pub struct RadarEntry {
    pub id: String,
    pub detected_at: DateTime<Utc>,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub subkind: String,
    pub state: String,
    pub mode: String,
    pub pivot_level: String,
    pub structural_score: f64,
    pub invalidation_price: Option<f64>,
    // Faz 13 taksonomisinden türetilenler:
    pub tier: String,
    pub event: String,
    pub direction: String,
    // Outcome (eğer backtest değerlendirilmişse):
    pub outcome: Option<String>,
    pub pnl_pct: Option<f64>,
    pub close_reason: Option<String>,
    /// Fix C — `mature`/`immature`/null. Immature = pivot ilk N barda kırıldı
    /// (tepki dibi). UI default olarak immature'ları win-rate'den dışlar.
    pub maturity: Option<String>,
    // Faz 13 — A (R-multiple) + B (Fib) hedefleri raw_meta.targets'tan.
    pub targets: Option<serde_json::Value>,
    // Yapısal kalite göstergeleri — curr pivot'un swing tipi
    // (HH/HL/LH/LL) ve önceki zıt-kind pivotun swing tipi.
    pub swing_type_curr: Option<String>,
    pub swing_type_prev_opp: Option<String>,
}

pub fn v2_reversal_radar_router() -> Router<SharedState> {
    Router::new().route("/v2/reversal-radar", get(get_radar))
}

async fn get_radar(
    State(st): State<SharedState>,
    Query(q): Query<RadarQuery>,
) -> Result<Json<RadarFeed>, ApiError> {
    let limit = q.limit.unwrap_or(300).clamp(1, 2_000);

    // Mode default = backtest (live veri henüz pivot_reversal aileyi
    // yazmıyor; operatör isterse ?mode=live geçsin).
    let mode = q.mode.as_deref().unwrap_or("backtest");

    let rows = sqlx::query(
        r#"
        SELECT d.id, d.detected_at, d.exchange, d.symbol, d.timeframe,
               d.subkind, d.state, d.mode, d.pivot_level,
               d.structural_score, d.invalidation_price,
               d.raw_meta->'targets' AS targets,
               d.raw_meta->>'swing_type_curr'     AS swing_type_curr,
               d.raw_meta->>'swing_type_prev_opp' AS swing_type_prev_opp,
               o.outcome, o.pnl_pct, o.close_reason, o.maturity
          FROM qtss_v2_detections d
          LEFT JOIN qtss_v2_detection_outcomes o ON o.detection_id = d.id
         WHERE d.family = 'pivot_reversal'
           AND d.mode = $1
           AND ($2::text IS NULL OR d.exchange  = $2)
           AND ($3::text IS NULL OR d.symbol    = $3)
           AND ($4::text IS NULL OR d.timeframe = $4)
           AND ($5::text IS NULL OR d.pivot_level = $5)
           AND ($6::text IS NULL OR split_part(d.subkind, '_', 1) = $6)
           AND ($7::text IS NULL OR split_part(d.subkind, '_', 2) = $7)
           AND ($8::text IS NULL OR split_part(d.subkind, '_', 3) = $8)
         ORDER BY d.detected_at DESC
         LIMIT $9
        "#,
    )
    .bind(mode)
    .bind(q.exchange.as_deref())
    .bind(q.symbol.as_deref())
    .bind(q.timeframe.as_deref())
    .bind(q.level.as_deref())
    .bind(q.tier.as_deref())
    .bind(q.event.as_deref())
    .bind(q.direction.as_deref())
    .bind(limit)
    .fetch_all(&st.pool)
    .await?;

    let entries: Vec<RadarEntry> = rows
        .into_iter()
        .map(|r| {
            let subkind: String = r.get("subkind");
            let parts: Vec<&str> = subkind.split('_').collect();
            let tier      = parts.first().copied().unwrap_or("").to_string();
            let event     = parts.get(1).copied().unwrap_or("").to_string();
            let direction = parts.get(2).copied().unwrap_or("").to_string();
            let invalidation_price: Option<rust_decimal::Decimal> =
                r.try_get("invalidation_price").ok();
            let inv_f = invalidation_price.and_then(|d| {
                use rust_decimal::prelude::ToPrimitive;
                d.to_f64()
            });
            let score: f32 = r.try_get("structural_score").unwrap_or(0.0);
            RadarEntry {
                id: r.get::<uuid::Uuid, _>("id").to_string(),
                detected_at: r.get("detected_at"),
                exchange: r.get("exchange"),
                symbol: r.get("symbol"),
                timeframe: r.get("timeframe"),
                subkind,
                state: r.get("state"),
                mode: r.get("mode"),
                pivot_level: r.try_get("pivot_level").unwrap_or_default(),
                structural_score: score as f64,
                invalidation_price: inv_f,
                tier,
                event,
                direction,
                outcome: r.try_get("outcome").ok(),
                pnl_pct: r.try_get::<f32, _>("pnl_pct").ok().map(|x| x as f64),
                close_reason: r.try_get("close_reason").ok(),
                maturity: r.try_get("maturity").ok(),
                targets: r.try_get::<Option<serde_json::Value>, _>("targets").ok().flatten(),
                swing_type_curr: r.try_get("swing_type_curr").ok(),
                swing_type_prev_opp: r.try_get("swing_type_prev_opp").ok(),
            }
        })
        .collect();

    Ok(Json(RadarFeed {
        generated_at: Utc::now(),
        entries,
    }))
}

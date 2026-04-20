//! `GET /v2/reports/performance` — Faz 15.R (rapor sayfası ön-versiyonu).
//!
//! QTSS aylık/haftalık/günlük/yıllık performans raporu. Paper-trading
//! (Faz 15) henüz canlı değil — bu endpoint, mevcut
//! `qtss_v2_detection_outcomes` tablosundan **sanal cüzdan** hesaplaması
//! yaparak raporu doldurur:
//!
//!   * Her kapanmış outcome bir "işlem" sayılır
//!   * Allocation = starting_equity × allocation_pct
//!   * Gross PnL   = allocation × pnl_pct
//!   * Net PnL     = gross − 2 × commission_bps × allocation (entry+exit)
//!   * Equity curve = Σ Net PnL (işlem sırası, resolved_at artan)
//!   * Bileşik getiri = current_equity / starting_equity − 1
//!
//! Faz 15 gerçek paper ledger tablosunu yazdığında bu endpoint kaynak
//! olarak o tabloya swap'lenir (default fallback: mevcut davranış).
//!
//! Input:
//!   * `exchange_class` : crypto | nasdaq | bist | all   (default: all)
//!   * `period`         : daily | weekly | monthly | yearly (default: monthly)
//!   * `as_of`          : ISO 8601 (default: now)   — raporun "kesit" anı
//!
//! Output: `PerformanceReport` — üst sayaçlar + kapanan işlemler listesi
//! + sermaye takibi.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct PerfQuery {
    pub exchange_class: Option<String>,
    pub period:         Option<String>,
    pub as_of:          Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ClosedTrade {
    pub symbol:          String,
    pub exchange:        String,
    pub tip:             String,          // family → tek harf (Q/H/E/C/W)
    pub resolved_at:     DateTime<Utc>,
    pub allocation:      f64,
    pub pnl:             f64,             // net (komisyon sonrası)
    pub pnl_pct:         f64,             // signed, net %
    pub outcome:         String,
    pub holding_hours:   f64,
}

#[derive(Debug, Serialize)]
pub struct PerformanceReport {
    pub generated_at:      DateTime<Utc>,
    pub exchange_class:    String,
    pub period:            String,
    pub period_start:      DateTime<Utc>,
    pub period_end:        DateTime<Utc>,
    pub currency:          String,

    // Üst KPI şeridi
    pub total_allocated:   f64,
    pub total_pnl:         f64,
    pub avg_pnl_pct:       f64,

    // Sayaç üçlüsü
    pub closed_count:      i64,
    pub win_count:         i64,
    pub loss_count:        i64,
    pub win_rate:          f64,   // 0..1

    // Sermaye takibi
    pub starting_equity:   f64,
    pub current_equity:    f64,
    pub compound_return:   f64,   // (current/starting) − 1
    pub avg_allocation:    f64,
    pub avg_holding_hours: f64,

    // Tablo
    pub trades:            Vec<ClosedTrade>,
}

pub fn v2_reports_performance_router() -> Router<SharedState> {
    Router::new().route("/v2/reports/performance", get(get_performance))
}

/// Period label → (start, end) pencereleri `as_of`'a göre geri sayar.
fn period_window(period: &str, as_of: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let start = match period {
        "daily"   => as_of - Duration::days(1),
        "weekly"  => as_of - Duration::days(7),
        "yearly"  => as_of - Duration::days(365),
        _         => as_of - Duration::days(30), // monthly default
    };
    (start, as_of)
}

/// Family → UI'de tek-harf rozet. Q = Q-RADAR (pivot_reversal),
/// H = harmonic, E = elliott, C = classical, W = wyckoff.
fn family_tip(family: &str) -> &'static str {
    match family {
        "pivot_reversal" => "Q",
        "harmonic"       => "H",
        "elliott"        => "E",
        "classical"      => "C",
        "wyckoff"        => "W",
        _                => "?",
    }
}

/// Config anahtarını `config_schema.default_value` üzerinden oku.
/// Canlı override için `system_config` tablosu kullanılıyor olabilir;
/// raporlar dashboard rolünde okunduğu için default yeterli — GUI
/// Config Editor aynı key'i canlı override edebilir.
async fn read_f64(pool: &sqlx::PgPool, key: &str, fallback: f64) -> f64 {
    sqlx::query_scalar::<_, Option<serde_json::Value>>(
        "SELECT default_value FROM config_schema WHERE key = $1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .flatten()
    .and_then(|v| match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    })
    .unwrap_or(fallback)
}

async fn get_performance(
    State(st): State<SharedState>,
    Query(q):  Query<PerfQuery>,
) -> Result<Json<PerformanceReport>, ApiError> {
    let as_of  = q.as_of.unwrap_or_else(Utc::now);
    let period = q.period.as_deref().unwrap_or("monthly").to_string();
    let class  = q.exchange_class.as_deref().unwrap_or("all").to_string();
    let (start, end) = period_window(&period, as_of);

    // Para birimi + starting equity + komisyon parametreleri.
    let (currency, starting_eq_key, commission_key) = match class.as_str() {
        "crypto" => ("USDT", "report.paper.starting_equity.crypto", "report.paper.commission_bps.crypto"),
        "nasdaq" => ("USD",  "report.paper.starting_equity.nasdaq", "report.paper.commission_bps.nasdaq"),
        "bist"   => ("TL",   "report.paper.starting_equity.bist",   "report.paper.commission_bps.bist"),
        _        => ("USDT", "report.paper.starting_equity.crypto", "report.paper.commission_bps.crypto"),
    };
    let starting_equity = read_f64(&st.pool, starting_eq_key, 10_000.0).await;
    let allocation_pct  = read_f64(&st.pool, "report.paper.allocation_pct", 0.10).await;
    let commission_bps  = read_f64(&st.pool, commission_key, 10.0).await;

    // Kapanmış işlemleri çek — exchange sınıfına göre filtre
    // `report_exchange_class` üzerinden çözülür. `all` durumunda tüm
    // borsalar katılır; tek raporda karışmaması için normalde UI
    // `class` gönderir.
    let trades_rows = sqlx::query(
        r#"
        SELECT d.symbol, d.exchange, d.family,
               o.outcome, o.pnl_pct, o.resolved_at, o.duration_secs
          FROM qtss_v2_detection_outcomes o
          JOIN qtss_v2_detections          d ON d.id = o.detection_id
          LEFT JOIN report_exchange_class  c ON c.exchange = d.exchange
         WHERE o.resolved_at >= $1
           AND o.resolved_at <  $2
           AND o.outcome IN ('win','loss')
           AND ($3::text = 'all' OR c.class = $3)
           -- Fix C (migration 0199): immature pivot_reversal'lar
           -- gerçek dip/tepe değil → raporda yer almaz.
           AND (d.family <> 'pivot_reversal' OR o.maturity IS DISTINCT FROM 'immature')
         ORDER BY o.resolved_at ASC
        "#,
    )
    .bind(start)
    .bind(end)
    .bind(&class)
    .fetch_all(&st.pool)
    .await?;

    // Her satırı sanal cüzdan mantığıyla para-birimine çevir. Allocation
    // = starting × pct (sabit; Faz 15'te cüzdan büyüdükçe güncellenecek).
    let allocation = starting_equity * allocation_pct;
    let commission_frac = commission_bps / 10_000.0; // bps → oran
    let mut trades: Vec<ClosedTrade> = Vec::with_capacity(trades_rows.len());
    let mut wins   = 0_i64;
    let mut losses = 0_i64;
    let mut total_pnl      = 0.0_f64;
    let mut total_pnl_pct  = 0.0_f64;
    let mut total_hold_hrs = 0.0_f64;

    for r in trades_rows {
        let pnl_pct_raw: f32 = r.try_get("pnl_pct").unwrap_or(0.0);
        // Net = gross − 2 × commission (entry + exit). commission_frac
        // oran cinsinden olduğu için pnl_pct ile aynı boyutta çıkarılır.
        let net_pnl_pct = (pnl_pct_raw as f64) - 2.0 * commission_frac;
        let net_pnl     = allocation * net_pnl_pct;
        let outcome: String = r.get("outcome");
        match outcome.as_str() {
            "win"  => wins += 1,
            "loss" => losses += 1,
            _ => {}
        }
        let duration_secs: i64 = r.try_get("duration_secs").unwrap_or(0);
        let hold_hrs = duration_secs as f64 / 3600.0;
        total_pnl      += net_pnl;
        total_pnl_pct  += net_pnl_pct;
        total_hold_hrs += hold_hrs;

        let family: String = r.get("family");
        trades.push(ClosedTrade {
            symbol:        r.get("symbol"),
            exchange:      r.get("exchange"),
            tip:           family_tip(&family).to_string(),
            resolved_at:   r.get("resolved_at"),
            allocation,
            pnl:           net_pnl,
            pnl_pct:       net_pnl_pct,
            outcome,
            holding_hours: hold_hrs,
        });
    }

    let closed_count = trades.len() as i64;
    let avg_pnl_pct = if closed_count > 0 {
        total_pnl_pct / closed_count as f64
    } else { 0.0 };
    let avg_holding_hours = if closed_count > 0 {
        total_hold_hrs / closed_count as f64
    } else { 0.0 };
    let avg_allocation = if closed_count > 0 { allocation } else { 0.0 };
    let current_equity = starting_equity + total_pnl;
    let compound_return = if starting_equity > 0.0 {
        current_equity / starting_equity - 1.0
    } else { 0.0 };
    let win_rate = if closed_count > 0 {
        wins as f64 / closed_count as f64
    } else { 0.0 };

    Ok(Json(PerformanceReport {
        generated_at: Utc::now(),
        exchange_class: class,
        period,
        period_start: start,
        period_end:   end,
        currency: currency.to_string(),
        total_allocated: allocation * closed_count as f64,
        total_pnl,
        avg_pnl_pct,
        closed_count,
        win_count: wins,
        loss_count: losses,
        win_rate,
        starting_equity,
        current_equity,
        compound_return,
        avg_allocation,
        avg_holding_hours,
        trades,
    }))
}

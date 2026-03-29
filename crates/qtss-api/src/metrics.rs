//! Basit Prometheus uyumlu sayaç + HTTP sayacı middleware.

use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::Query;
use axum::extract::Request;
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

static HTTP_REQUESTS: AtomicU64 = AtomicU64::new(0);

/// `POST /api/v1/reconcile/binance` — başarılı istekler.
static RECONCILE_SPOT_OK: AtomicU64 = AtomicU64::new(0);
/// Spot reconcile hata (validasyon, Binance, DB).
static RECONCILE_SPOT_ERR: AtomicU64 = AtomicU64::new(0);
/// Spot: `exchange_orders` üzerinde toplam güncellenen satır (`status_updates_applied` toplamı).
static RECONCILE_SPOT_ROWS: AtomicU64 = AtomicU64::new(0);

static RECONCILE_FUTURES_OK: AtomicU64 = AtomicU64::new(0);
static RECONCILE_FUTURES_ERR: AtomicU64 = AtomicU64::new(0);
static RECONCILE_FUTURES_ROWS: AtomicU64 = AtomicU64::new(0);

pub fn inc_http_requests() {
    HTTP_REQUESTS.fetch_add(1, Ordering::Relaxed);
}

/// `rows_updated`: başarılı yanıtta patch’lenen satır sayısı (0 olabilir).
pub fn record_reconcile_spot(ok: bool, rows_updated: u64) {
    if ok {
        RECONCILE_SPOT_OK.fetch_add(1, Ordering::Relaxed);
        RECONCILE_SPOT_ROWS.fetch_add(rows_updated, Ordering::Relaxed);
    } else {
        RECONCILE_SPOT_ERR.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn record_reconcile_futures(ok: bool, rows_updated: u64) {
    if ok {
        RECONCILE_FUTURES_OK.fetch_add(1, Ordering::Relaxed);
        RECONCILE_FUTURES_ROWS.fetch_add(rows_updated, Ordering::Relaxed);
    } else {
        RECONCILE_FUTURES_ERR.fetch_add(1, Ordering::Relaxed);
    }
}

pub async fn count_http_requests_middleware(req: Request, next: Next) -> Response {
    inc_http_requests();
    next.run(req).await
}

pub fn prometheus_text() -> String {
    let n = HTTP_REQUESTS.load(Ordering::Relaxed);
    let ver = env!("CARGO_PKG_VERSION");
    let rs_ok = RECONCILE_SPOT_OK.load(Ordering::Relaxed);
    let rs_err = RECONCILE_SPOT_ERR.load(Ordering::Relaxed);
    let rs_rows = RECONCILE_SPOT_ROWS.load(Ordering::Relaxed);
    let rf_ok = RECONCILE_FUTURES_OK.load(Ordering::Relaxed);
    let rf_err = RECONCILE_FUTURES_ERR.load(Ordering::Relaxed);
    let rf_rows = RECONCILE_FUTURES_ROWS.load(Ordering::Relaxed);
    format!(
        "# HELP qtss_build_info Sabit derleme bilgisi (etiketler)\n\
         # TYPE qtss_build_info gauge\n\
         qtss_build_info{{version=\"{ver}\",service=\"qtss-api\"}} 1\n\
         # HELP qtss_http_requests_total Yaklaşık HTTP istek sayısı (sayım middleware)\n\
         # TYPE qtss_http_requests_total counter\n\
         qtss_http_requests_total {n}\n\
         # HELP qtss_reconcile_binance_requests_total POST reconcile/binance tamamlanan istekler\n\
         # TYPE qtss_reconcile_binance_requests_total counter\n\
         qtss_reconcile_binance_requests_total{{segment=\"spot\",status=\"ok\"}} {rs_ok}\n\
         qtss_reconcile_binance_requests_total{{segment=\"spot\",status=\"error\"}} {rs_err}\n\
         qtss_reconcile_binance_requests_total{{segment=\"futures\",status=\"ok\"}} {rf_ok}\n\
         qtss_reconcile_binance_requests_total{{segment=\"futures\",status=\"error\"}} {rf_err}\n\
         # HELP qtss_reconcile_binance_exchange_orders_rows_updated_total Patch sonrası güncellenen exchange_orders satırı (kümülatif)\n\
         # TYPE qtss_reconcile_binance_exchange_orders_rows_updated_total counter\n\
         qtss_reconcile_binance_exchange_orders_rows_updated_total{{segment=\"spot\"}} {rs_rows}\n\
         qtss_reconcile_binance_exchange_orders_rows_updated_total{{segment=\"futures\"}} {rf_rows}\n"
    )
}

pub async fn prometheus_metrics() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        prometheus_text(),
    )
}

#[derive(Debug, Deserialize)]
pub struct MetricsQuery {
    pub token: Option<String>,
}

/// `QTSS_METRICS_TOKEN` doluys Bearer veya `?token=` zorunlu.
pub async fn prometheus_metrics_gate(
    headers: HeaderMap,
    Query(q): Query<MetricsQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    if let Ok(tok) = std::env::var("QTSS_METRICS_TOKEN") {
        if !tok.is_empty() {
            let bearer_ok = headers
                .get(AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .map(|t| t == tok.as_str())
                .unwrap_or(false);
            let query_ok = q.token.as_deref() == Some(tok.as_str());
            if !bearer_ok && !query_ok {
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
    }
    Ok(prometheus_metrics().await)
}

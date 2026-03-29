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

pub fn inc_http_requests() {
    HTTP_REQUESTS.fetch_add(1, Ordering::Relaxed);
}

pub async fn count_http_requests_middleware(req: Request, next: Next) -> Response {
    inc_http_requests();
    next.run(req).await
}

pub fn prometheus_text() -> String {
    let n = HTTP_REQUESTS.load(Ordering::Relaxed);
    let ver = env!("CARGO_PKG_VERSION");
    format!(
        "# HELP qtss_build_info Sabit derleme bilgisi (etiketler)\n\
         # TYPE qtss_build_info gauge\n\
         qtss_build_info{{version=\"{ver}\",service=\"qtss-api\"}} 1\n\
         # HELP qtss_http_requests_total Yaklaşık HTTP istek sayısı (sayım middleware)\n\
         # TYPE qtss_http_requests_total counter\n\
         qtss_http_requests_total {n}\n"
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

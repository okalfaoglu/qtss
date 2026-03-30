#![recursion_limit = "256"]

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use axum::http::HeaderName;
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use qtss_common::{
    ensure_postgres_scheme, init_logging, load_dotenv, postgres_url_from_env_or_default,
};
use qtss_storage::{create_pool, run_migrations};
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::GovernorLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

mod audit_event;
mod audit_http;
pub mod error;
mod locale;
mod metrics;
mod oauth;
mod rate_limit;
mod routes;
mod state;

use rate_limit::ForwardedIpKeyExtractor;
use routes::health_router;
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv();
    // Worker ile aynı: `_sqlx_migrations` CREATE IF NOT EXISTS NOTICE’larını INFO’da göstermez.
    init_logging(
        "info,qtss_api=debug,qtss_storage=debug,tower_http=info,sqlx::postgres::notice=warn",
    );

    let database_url = postgres_url_from_env_or_default("postgres://qtss:qtss@127.0.0.1:5432/qtss");
    ensure_postgres_scheme(&database_url).map_err(anyhow::Error::msg)?;
    let pool = create_pool(&database_url, 10).await?;
    run_migrations(&pool).await.context(
        "qtss-api: SQL migrations failed — süreç stdout/stderr. \
         Yaygın: checksum uyuşmazlığı → `cargo run -p qtss-storage --bin qtss-sync-sqlx-checksums` (DATABASE_URL); \
         `to_regclass('public.bar_intervals')` NULL → `0036_bar_intervals_repair_if_missing.sql` (API/worker migrate); \
         çift aynı `NNNN_*.sql` öneki. Ayrıntı: docs/QTSS_CURSOR_DEV_GUIDE.md §6.",
    )?;

    let state = Arc::new(AppState::new(pool)?);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // `tower-governor`: her `replenish_ms` ms’de kovaya 1 jeton; sürdürülebilir ~1000/replenish_ms RPS (burst sonrası).
    let replenish_ms = std::env::var("QTSS_RATE_LIMIT_REPLENISH_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_u64)
        .max(1);
    let burst = std::env::var("QTSS_RATE_LIMIT_BURST")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120_u32)
        .max(1);

    let governor_conf = Arc::new(
        GovernorConfigBuilder::default()
            .key_extractor(ForwardedIpKeyExtractor::from_env())
            .per_millisecond(replenish_ms)
            .burst_size(burst)
            .finish()
            .expect("QTSS: rate limit yapılandırması"),
    );

    let x_request_id = HeaderName::from_static("x-request-id");

    let app = Router::new()
        .merge(health_router())
        .route("/metrics", get(metrics::prometheus_metrics_gate))
        .route("/oauth/token", post(oauth::oauth_token))
        .nest(
            "/api/v1",
            routes::public_locales_routes().merge(routes::api_router(state.clone())),
        )
        .layer(middleware::from_fn(metrics::count_http_requests_middleware))
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .layer(TraceLayer::new_for_http())
        .layer(GovernorLayer {
            config: governor_conf,
        })
        .layer(cors)
        .with_state(state);

    let addr: SocketAddr = std::env::var("QTSS_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()?;
    info!("QTSS API dinleniyor: {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

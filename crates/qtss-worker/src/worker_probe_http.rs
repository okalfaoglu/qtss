//! İsteğe bağlı HTTP probe uçları — kube / systemd sağlık için (`QTSS_WORKER_HTTP_BIND`).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;
use axum::Router;
use serde::Serialize;
use sqlx::PgPool;
use tracing::{info, warn};

#[derive(Serialize)]
struct LiveBody {
    status: &'static str,
    service: &'static str,
}

#[derive(Serialize)]
struct ReadyBody {
    status: &'static str,
    service: &'static str,
    /// `ok` = `SELECT 1` başarılı; `none` = `DATABASE_URL` yok (worker yalnızca WS vb.).
    database: &'static str,
}

#[derive(Clone)]
struct ProbeState {
    pool: Option<PgPool>,
}

/// `bind` üzerinde dinler; süreç ayakta kaldığı sürece bloklar.
pub async fn serve(bind: SocketAddr, pool: Option<PgPool>) -> anyhow::Result<()> {
    let state = Arc::new(ProbeState { pool });
    let app = Router::new()
        .route("/live", get(live))
        .route("/ready", get(ready))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "worker probe HTTP (/live, /ready)");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn live() -> Json<LiveBody> {
    Json(LiveBody {
        status: "alive",
        service: "qtss-worker",
    })
}

async fn ready(State(st): State<Arc<ProbeState>>) -> impl IntoResponse {
    if let Some(ref pool) = st.pool {
        if let Err(e) = sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(pool).await {
            warn!(%e, "worker /ready: PostgreSQL ping başarısız");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        return Json(ReadyBody {
            status: "ready",
            service: "qtss-worker",
            database: "ok",
        })
        .into_response();
    }
    Json(ReadyBody {
        status: "ready",
        service: "qtss-worker",
        database: "none",
    })
    .into_response()
}

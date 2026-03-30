use axum::extract::State;
use axum::{routing::get, Json, Router};
use serde::Serialize;
use sqlx::PgPool;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: &'static str,
}

#[derive(Serialize)]
struct LiveBody {
    status: &'static str,
    service: &'static str,
}

#[derive(Serialize)]
struct ReadyBody {
    status: &'static str,
    service: &'static str,
    database: &'static str,
}

pub fn health_router() -> Router<SharedState> {
    Router::new()
        .route("/health", get(health))
        .route("/live", get(live))
        .route("/ready", get(ready))
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        service: "qtss-api",
    })
}

/// Liveness — süreç cevap veriyor; dış bağımlılık yok (kube `livenessProbe`).
async fn live() -> Json<LiveBody> {
    Json(LiveBody {
        status: "alive",
        service: "qtss-api",
    })
}

/// Readiness — PostgreSQL erişimi; başarısızsa 503 (kube `readinessProbe`).
async fn ready(State(st): State<SharedState>) -> Result<Json<ReadyBody>, ApiError> {
    ping_db(&st.pool)
        .await
        .map_err(|_| ApiError::service_unavailable("veritabanına erişilemiyor"))?;
    Ok(Json(ReadyBody {
        status: "ready",
        service: "qtss-api",
        database: "ok",
    }))
}

async fn ping_db(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await?;
    Ok(())
}

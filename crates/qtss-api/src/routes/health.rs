use axum::{routing::get, Json, Router};
use serde::Serialize;

use crate::state::SharedState;

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: &'static str,
}

pub fn health_router() -> Router<SharedState> {
    Router::new().route(
        "/health",
        get(|| async { Json(Health { status: "ok", service: "qtss-api" }) }),
    )
}

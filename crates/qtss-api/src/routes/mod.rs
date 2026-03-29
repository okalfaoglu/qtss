mod ai_approval;
mod analysis;
mod catalog_admin;
mod catalog_sync;
mod config_admin;
mod copy_trade;
mod dashboard;
mod external_fetch;
mod health;
mod market_binance;
mod notify;
mod onchain_signals;
mod orders_binance;
mod orders_dry;
mod reconcile;
mod session;

use axum::middleware::from_fn;
use axum::middleware::from_fn_with_state;
use axum::Router;

pub use catalog_sync::catalog_sync_router;
pub use config_admin::config_router;
pub use dashboard::{dashboard_admin_router, dashboard_router};
pub use health::health_router;

use crate::audit_http::audit_http_middleware;
use crate::oauth::middleware::require_jwt;
use crate::oauth::rbac::{require_admin, require_dashboard_roles, require_ops_roles};
use crate::state::SharedState;

use catalog_admin::{catalog_read_router, catalog_write_router};

/// `/api/v1` altında: korumalı uçlar (Bearer + rol).
pub fn api_router(state: SharedState) -> Router<SharedState> {
    let jwt_layer = from_fn_with_state(state.clone(), require_jwt);
    let audit_layer = from_fn_with_state(state.clone(), audit_http_middleware);

    Router::new()
        .merge(
            session::session_router().layer(from_fn(require_dashboard_roles)),
        )
        .merge(
            config_router()
                .layer(from_fn(require_admin)),
        )
        .merge(
            dashboard_router()
                .layer(from_fn(require_dashboard_roles)),
        )
        .merge(
            dashboard_admin_router()
                .layer(from_fn(require_admin)),
        )
        .merge(
            catalog_sync_router()
                .layer(from_fn(require_ops_roles)),
        )
        .merge(
            catalog_read_router()
                .layer(from_fn(require_dashboard_roles)),
        )
        .merge(
            catalog_write_router()
                .layer(from_fn(require_ops_roles)),
        )
        .merge(
            market_binance::market_binance_router()
                .layer(from_fn(require_dashboard_roles)),
        )
        .merge(
            market_binance::market_binance_write_router()
                .layer(from_fn(require_ops_roles)),
        )
        .merge(
            reconcile::reconcile_router()
                .layer(from_fn(require_admin)),
        )
        .merge(
            orders_binance::orders_binance_read_router()
                .layer(from_fn(require_dashboard_roles)),
        )
        .merge(
            orders_binance::orders_binance_write_router()
                .layer(from_fn(require_ops_roles)),
        )
        .merge(
            orders_dry::orders_dry_read_router()
                .layer(from_fn(require_dashboard_roles)),
        )
        .merge(
            orders_dry::orders_dry_write_router()
                .layer(from_fn(require_ops_roles)),
        )
        .merge(
            copy_trade::copy_trade_read_router()
                .layer(from_fn(require_dashboard_roles)),
        )
        .merge(
            copy_trade::copy_trade_write_router()
                .layer(from_fn(require_ops_roles)),
        )
        .merge(
            notify::notify_outbox_write_router()
                .layer(from_fn(require_ops_roles)),
        )
        .merge(
            ai_approval::ai_approval_read_router()
                .layer(from_fn(require_dashboard_roles)),
        )
        .merge(
            ai_approval::ai_approval_submit_router()
                .layer(from_fn(require_ops_roles)),
        )
        .merge(
            ai_approval::ai_approval_admin_router()
                .layer(from_fn(require_admin)),
        )
        .merge(
            analysis::analysis_read_router()
                .merge(notify::notify_router())
                .merge(external_fetch::external_fetch_read_router())
                .merge(onchain_signals::onchain_signals_router())
                .layer(from_fn(require_dashboard_roles)),
        )
        .merge(
            analysis::analysis_write_router()
                .merge(external_fetch::external_fetch_write_router())
                .layer(from_fn(require_ops_roles)),
        )
        .layer(audit_layer)
        .layer(jwt_layer)
}

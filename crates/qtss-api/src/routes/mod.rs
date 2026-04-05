mod ai_approval;
mod ai_decisions;
mod analysis;
mod bootstrap;
mod audit_admin;
mod backtest;
mod catalog_admin;
mod catalog_sync;
mod config_admin;
mod copy_trade;
mod dashboard;
mod external_fetch;
mod fills;
mod health;
mod kill_switch_admin;
pub mod locales;
mod market_binance;
mod notify;
mod onchain_signals;
mod orders_binance;
mod orders_bybit;
mod orders_dry;
mod orders_okx;
mod reconcile;
mod session;
mod system_config_admin;
mod telegram_setup_analysis;
mod user_permissions_admin;

use axum::middleware::from_fn;
use axum::middleware::from_fn_with_state;
use axum::Router;

pub use catalog_sync::catalog_sync_router;
pub use config_admin::config_router;
pub use dashboard::{dashboard_admin_router, dashboard_router};
pub use health::health_router;
pub use bootstrap::public_bootstrap_routes;
pub use locales::public_locales_routes;

use crate::audit_http::audit_http_middleware;
use crate::locale::locale_middleware;
use crate::oauth::middleware::require_jwt;
use crate::oauth::rbac::{
    require_admin, require_audit_read, require_dashboard_roles, require_ops_roles,
};
use crate::state::SharedState;

use catalog_admin::{catalog_read_router, catalog_write_router};
use backtest::backtest_router;

/// `/api/v1` altında: korumalı uçlar (Bearer + rol).
pub fn api_router(state: SharedState) -> Router<SharedState> {
    let jwt_layer = from_fn_with_state(state.clone(), require_jwt);
    let audit_layer = from_fn_with_state(state.clone(), audit_http_middleware);

    Router::new()
        .merge(session::session_router().layer(from_fn(require_dashboard_roles)))
        .merge(
            user_permissions_admin::user_permissions_admin_router().layer(from_fn(require_admin)),
        )
        .merge(audit_admin::audit_admin_router().layer(from_fn(require_audit_read)))
        .merge(
            config_router()
                .merge(system_config_admin::system_config_admin_router())
                .layer(from_fn(require_admin)),
        )
        .merge(dashboard_router().layer(from_fn(require_dashboard_roles)))
        .merge(dashboard_admin_router().layer(from_fn(require_admin)))
        .merge(kill_switch_admin::kill_switch_admin_router().layer(from_fn(require_admin)))
        .merge(catalog_sync_router().layer(from_fn(require_ops_roles)))
        .merge(catalog_read_router().layer(from_fn(require_dashboard_roles)))
        .merge(catalog_write_router().layer(from_fn(require_ops_roles)))
        .merge(market_binance::market_binance_router().layer(from_fn(require_dashboard_roles)))
        .merge(market_binance::market_binance_write_router().layer(from_fn(require_ops_roles)))
        .merge(reconcile::reconcile_router().layer(from_fn(require_admin)))
        .merge(orders_binance::orders_binance_read_router().layer(from_fn(require_dashboard_roles)))
        .merge(orders_binance::orders_binance_write_router().layer(from_fn(require_ops_roles)))
        .merge(orders_bybit::orders_bybit_write_router().layer(from_fn(require_ops_roles)))
        .merge(orders_okx::orders_okx_write_router().layer(from_fn(require_ops_roles)))
        .merge(orders_dry::orders_dry_read_router().layer(from_fn(require_dashboard_roles)))
        .merge(orders_dry::orders_dry_write_router().layer(from_fn(require_ops_roles)))
        .merge(fills::fills_router().layer(from_fn(require_dashboard_roles)))
        .merge(backtest_router().layer(from_fn(require_dashboard_roles)))
        .merge(copy_trade::copy_trade_read_router().layer(from_fn(require_dashboard_roles)))
        .merge(copy_trade::copy_trade_write_router().layer(from_fn(require_ops_roles)))
        .merge(notify::notify_outbox_write_router().layer(from_fn(require_ops_roles)))
        .merge(ai_decisions::ai_decisions_read_router())
        .merge(ai_approval::ai_approval_read_router().layer(from_fn(require_dashboard_roles)))
        .merge(ai_approval::ai_approval_submit_router().layer(from_fn(require_ops_roles)))
        .merge(ai_decisions::ai_decisions_admin_router().layer(from_fn(require_admin)))
        .merge(ai_approval::ai_approval_admin_router().layer(from_fn(require_admin)))
        .merge(
            analysis::analysis_read_router()
                .merge(notify::notify_router())
                .merge(external_fetch::external_fetch_read_router())
                .merge(onchain_signals::onchain_signals_router())
                .merge(telegram_setup_analysis::telegram_setup_analysis_status_router())
                .layer(from_fn(require_dashboard_roles)),
        )
        .merge(
            analysis::analysis_write_router()
                .merge(external_fetch::external_fetch_write_router())
                .layer(from_fn(require_ops_roles)),
        )
        .layer(from_fn(locale_middleware))
        .layer(audit_layer)
        .layer(jwt_layer)
}

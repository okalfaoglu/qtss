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
mod v2_ai_decisions;
mod v2_live_positions;
mod v2_position_events;
mod v2_selected_candidates;
mod v2_audit;
mod v2_backtest;
mod v2_blotter;
mod v2_chart;
mod v2_elliott;
mod v2_elliott_db;
mod v2_confluence;
mod v2_radar;
mod v2_harmonic;
mod v2_harmonic_db;
mod v2_indicators;
mod v2_zigzag;
mod v2_config;
mod v2_dashboard;
mod v2_fees;
mod v2_montecarlo;
mod v2_regime;
mod v2_scenarios;
mod v2_strategies;
mod v2_pivots;
mod v2_reconcile;
mod v2_engine_symbols;
mod v2_events_stream;
mod v2_q_radar;
mod v2_models;
mod v2_setups;
mod v2_users;
mod v2_wave_projections;
mod v2_wave_tree;
mod v2_symbols;

pub use v2_ai_decisions::v2_ai_decisions_router;
pub use v2_live_positions::v2_live_positions_router;
pub use v2_position_events::v2_position_events_router;
pub use v2_selected_candidates::v2_selected_candidates_router;
pub use v2_audit::v2_audit_router;
pub use v2_backtest::v2_backtest_router;
pub use v2_blotter::v2_blotter_router;
pub use v2_chart::v2_chart_router;
pub use v2_elliott::v2_elliott_router;
pub use v2_elliott_db::v2_elliott_db_router;
pub use v2_confluence::v2_confluence_router;
pub use v2_radar::v2_radar_router;
pub use v2_harmonic::v2_harmonic_router;
pub use v2_harmonic_db::v2_harmonic_db_router;
pub use v2_indicators::v2_indicators_router;
pub use v2_zigzag::v2_zigzag_router;
pub use v2_config::{v2_config_admin_router, v2_config_router};
pub use v2_dashboard::{v2_dashboard_router, V2DashboardHandle};
pub use v2_fees::v2_fees_router;
pub use v2_montecarlo::v2_montecarlo_router;
pub use v2_regime::v2_regime_router;
pub use v2_scenarios::v2_scenarios_router;
pub use v2_strategies::{
    default_seed_card, v2_strategies_admin_router, v2_strategies_router, V2StrategyRegistry,
};
pub use v2_pivots::v2_pivots_router;
pub use v2_reconcile::v2_reconcile_router;
pub use v2_engine_symbols::v2_engine_symbols_router;
pub use v2_events_stream::v2_events_stream_router;
pub use v2_q_radar::v2_q_radar_router;
pub use v2_models::v2_models_router;
pub use v2_setups::v2_setups_router;
pub use v2_users::v2_users_router;
pub use v2_wave_projections::v2_wave_projections_router;
pub use v2_wave_tree::v2_wave_tree_router;
pub use v2_symbols::v2_symbols_router;

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
        .merge(v2_dashboard_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_chart_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_zigzag_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_elliott_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_elliott_db_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_harmonic_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_harmonic_db_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_indicators_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_confluence_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_radar_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_pivots_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_setups_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_fees_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_models_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_q_radar_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_engine_symbols_router().layer(from_fn(require_admin)))
        .merge(v2_events_stream_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_scenarios_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_regime_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_montecarlo_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_wave_tree_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_wave_projections_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_symbols_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_reconcile_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_backtest_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_blotter_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_live_positions_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_selected_candidates_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_position_events_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_strategies_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_strategies_admin_router().layer(from_fn(require_admin)))
        .merge(v2_config_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_config_admin_router().layer(from_fn(require_admin)))
        .merge(v2_ai_decisions_router().layer(from_fn(require_dashboard_roles)))
        .merge(v2_audit_router().layer(from_fn(require_audit_read)))
        .merge(v2_users_router().layer(from_fn(require_admin)))
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
        .merge(
            ai_decisions::ai_decisions_ops_router().layer(from_fn(require_ops_roles)),
        )
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

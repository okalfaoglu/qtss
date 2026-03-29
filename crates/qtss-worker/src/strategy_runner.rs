//! Opsiyonel dry-run strateji döngüleri (`QTSS_STRATEGY_RUNNER_ENABLED=1`, dev guide ADIM 7 + §7.2).

use std::str::FromStr;
use std::sync::Arc;

use qtss_domain::commission::CommissionPolicy;
use qtss_domain::execution::VirtualLedgerParams;
use qtss_execution::DryRunGateway;
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::info;

fn enabled() -> bool {
    std::env::var("QTSS_STRATEGY_RUNNER_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

pub fn dry_gateway_from_env() -> Arc<DryRunGateway> {
    let init = std::env::var("QTSS_STRATEGY_RUNNER_QUOTE_BALANCE_USDT")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .unwrap_or_else(|| Decimal::new(100_000, 0));
    Arc::new(DryRunGateway::new(
        VirtualLedgerParams {
            initial_quote_balance: init,
        },
        CommissionPolicy::default(),
        None,
    ))
}

pub fn spawn_if_enabled(pool: &PgPool) {
    if !enabled() {
        return;
    }
    let gw = dry_gateway_from_env();
    info!(
        "QTSS_STRATEGY_RUNNER_ENABLED: dry strateji döngüleri (signal_filter, whale_momentum, arb_funding, copy_trade)"
    );
    let p = pool.clone();
    let g = gw.clone();
    tokio::spawn(async move {
        qtss_strategy::signal_filter::run(p, g).await;
    });
    let p = pool.clone();
    let g = gw.clone();
    tokio::spawn(async move {
        qtss_strategy::whale_momentum::run(p, g).await;
    });
    let p = pool.clone();
    let g = gw.clone();
    tokio::spawn(async move {
        qtss_strategy::arb_funding::run(p, g).await;
    });
    let p = pool.clone();
    let g = gw.clone();
    tokio::spawn(async move {
        qtss_strategy::copy_trade::run(p, g).await;
    });
}

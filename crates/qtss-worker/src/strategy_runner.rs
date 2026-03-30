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

fn env_strategy_balance_usdt(strategy_env_suffix: &str) -> Option<Decimal> {
    let key = format!("QTSS_STRATEGY_{}_BALANCE", strategy_env_suffix.to_ascii_uppercase());
    std::env::var(&key)
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
}

/// Her strateji için ayrı sanal bakiye: önce `QTSS_STRATEGY_<NAME>_BALANCE`, yoksa `QTSS_STRATEGY_RUNNER_QUOTE_BALANCE_USDT / 4`.
pub fn dry_gateway_for_strategy(strategy_name: &str) -> Arc<DryRunGateway> {
    let default_total = std::env::var("QTSS_STRATEGY_RUNNER_QUOTE_BALANCE_USDT")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .unwrap_or_else(|| Decimal::new(100_000, 0));
    let per_default = default_total / Decimal::from(4u32);

    let suffix = strategy_name
        .trim()
        .chars()
        .map(|c| if c == '-' { '_' } else { c })
        .collect::<String>();
    let init = env_strategy_balance_usdt(&suffix).unwrap_or(per_default);

    Arc::new(DryRunGateway::new(
        VirtualLedgerParams {
            initial_quote_balance: init,
        },
        CommissionPolicy::default(),
        None,
    ))
}

/// Geriye uyumluluk — tek gateway (ör. `position_manager` dry); tam `QTSS_STRATEGY_RUNNER_QUOTE_BALANCE_USDT`.
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
    info!(
        "QTSS_STRATEGY_RUNNER_ENABLED: dry strateji döngüleri (signal_filter, whale_momentum, arb_funding, copy_trade) — ayrı bakiye: QTSS_STRATEGY_<NAME>_BALANCE veya toplam/4"
    );
    let p = pool.clone();
    let g = dry_gateway_for_strategy("signal_filter");
    tokio::spawn(async move {
        qtss_strategy::signal_filter::run(p, g).await;
    });
    let p = pool.clone();
    let g = dry_gateway_for_strategy("whale_momentum");
    tokio::spawn(async move {
        qtss_strategy::whale_momentum::run(p, g).await;
    });
    let p = pool.clone();
    let g = dry_gateway_for_strategy("arb_funding");
    tokio::spawn(async move {
        qtss_strategy::arb_funding::run(p, g).await;
    });
    let p = pool.clone();
    let g = dry_gateway_for_strategy("copy_trade");
    tokio::spawn(async move {
        qtss_strategy::copy_trade::run(p, g).await;
    });
}

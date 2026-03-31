//! Opsiyonel dry-run strateji döngüleri (`QTSS_STRATEGY_RUNNER_ENABLED=1`, dev guide ADIM 7 + §7.2).
//!
//! Her strateji **ayrı** [`DryRunGateway`] ile çalışır (paylaşılan sanal bakiye yok).
//! - Toplam: `QTSS_STRATEGY_RUNNER_QUOTE_BALANCE_USDT` (varsayılan `100000` USDT), strateji başına varsayılan = toplam / N.
//! - Override: `QTSS_STRATEGY_<UPPER_NAME>_BALANCE` (örn. `QTSS_STRATEGY_SIGNAL_FILTER_BALANCE`), isimde tire → alt çizgi.
//! `position_manager` dry kapanışı tam toplam havuzu için [`dry_gateway_from_env`] kullanır (FAZ 0.2 ile uyumlu).

use std::str::FromStr;
use std::sync::Arc;

use qtss_domain::commission::CommissionPolicy;
use qtss_domain::execution::VirtualLedgerParams;
use qtss_execution::DryRunGateway;
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::info;

/// Dry runner’da eşzamanlı strateji sayısı — bölme paydası ve spawn listesi buradan türetilir.
const DRY_RUNNER_STRATEGIES: &[&str] = &[
    "signal_filter",
    "whale_momentum",
    "arb_funding",
    "copy_trade",
];

fn dry_runner_strategy_count_dec() -> Decimal {
    Decimal::from(DRY_RUNNER_STRATEGIES.len() as u32)
}

fn enabled() -> bool {
    std::env::var("QTSS_STRATEGY_RUNNER_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

/// Normalizes strategy name for `QTSS_STRATEGY_<SUFFIX>_BALANCE` (hyphen → underscore, ASCII upper).
#[must_use]
pub fn strategy_env_suffix_normalized(strategy_name: &str) -> String {
    strategy_name
        .trim()
        .chars()
        .map(|c| if c == '-' { '_' } else { c })
        .collect::<String>()
        .to_ascii_uppercase()
}

fn env_strategy_balance_usdt(strategy_name: &str) -> Option<Decimal> {
    let key = format!(
        "QTSS_STRATEGY_{}_BALANCE",
        strategy_env_suffix_normalized(strategy_name)
    );
    std::env::var(&key)
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
}

/// Her strateji için ayrı sanal bakiye: önce `QTSS_STRATEGY_<NAME>_BALANCE`, yoksa `QTSS_STRATEGY_RUNNER_QUOTE_BALANCE_USDT / N` (`N` = [`DRY_RUNNER_STRATEGIES`].len()).
pub fn dry_gateway_for_strategy(strategy_name: &str) -> Arc<DryRunGateway> {
    let default_total = std::env::var("QTSS_STRATEGY_RUNNER_QUOTE_BALANCE_USDT")
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .unwrap_or_else(|| Decimal::new(100_000, 0));
    let per_default = default_total / dry_runner_strategy_count_dec();

    let init = env_strategy_balance_usdt(strategy_name).unwrap_or(per_default);

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
        "QTSS_STRATEGY_RUNNER_ENABLED: dry strateji döngüleri ({}) — her biri ayrı gateway; QTSS_STRATEGY_<NAME>_BALANCE veya toplam/{}",
        DRY_RUNNER_STRATEGIES.join(", "),
        DRY_RUNNER_STRATEGIES.len(),
    );
    for &name in DRY_RUNNER_STRATEGIES {
        let p = pool.clone();
        let g = dry_gateway_for_strategy(name);
        match name {
            "signal_filter" => tokio::spawn(async move {
                qtss_strategy::signal_filter::run(p, g).await;
            }),
            "whale_momentum" => tokio::spawn(async move {
                qtss_strategy::whale_momentum::run(p, g).await;
            }),
            "arb_funding" => tokio::spawn(async move {
                qtss_strategy::arb_funding::run(p, g).await;
            }),
            "copy_trade" => tokio::spawn(async move {
                qtss_strategy::copy_trade::run(p, g).await;
            }),
            _ => unreachable!("DRY_RUNNER_STRATEGIES out of sync with spawn match"),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_env_suffix_hyphen_to_underscore_uppercase() {
        assert_eq!(
            strategy_env_suffix_normalized("signal-filter"),
            "SIGNAL_FILTER"
        );
        assert_eq!(
            strategy_env_suffix_normalized("whale_momentum"),
            "WHALE_MOMENTUM"
        );
    }

    #[test]
    fn dry_runner_strategy_count_matches_four() {
        assert_eq!(DRY_RUNNER_STRATEGIES.len(), 4);
        assert_eq!(dry_runner_strategy_count_dec(), Decimal::from(4u32));
    }
}

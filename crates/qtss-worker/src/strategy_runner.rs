//! Opsiyonel dry-run strateji döngüleri (`worker.strategy_runner_enabled`, dev guide ADIM 7 + §7.2).
//!
//! Mimari not: strateji adları ve bakiye anahtarları şu an bu dosyada dağınık sabit dizelerdir.
//! Yeni strateji eklemek için [`DRY_RUNNER_STRATEGIES`], [`worker_balance_key`], spawn listesi ve env
//! soneklerinin hepsi güncellenir; ileride `RunnableStrategy` trait + kayıt defteri (registry) ile
//! tek kaynaktan türetilmelidir.
//!
//! Her strateji **ayrı** [`DryRunGateway`] ile çalışır (paylaşılan sanal bakiye yok).
//! - Toplam: `worker.strategy_runner_quote_balance_usdt` (varsayılan 100000 USDT), strateji başına = toplam / N.
//! - Override: `worker.strategy_*_balance` veya env `QTSS_STRATEGY_<NAME>_BALANCE`.

use std::str::FromStr;

use qtss_domain::commission::CommissionPolicy;
use qtss_domain::execution::VirtualLedgerParams;
use qtss_execution::{DryRunGateway, ExecutionGateway};
use qtss_storage::{resolve_system_string, resolve_worker_enabled_flag};
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::sync::Arc;
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

fn worker_balance_key(strategy_name: &str) -> Option<&'static str> {
    match strategy_name {
        "signal_filter" => Some("strategy_signal_filter_balance"),
        "whale_momentum" => Some("strategy_whale_momentum_balance"),
        "arb_funding" => Some("strategy_arb_funding_balance"),
        "copy_trade" => Some("strategy_copy_trade_balance"),
        _ => None,
    }
}

async fn strategy_balance_usdt(strategy_name: &str, pool: &PgPool) -> Option<Decimal> {
    let Some(wkey) = worker_balance_key(strategy_name) else {
        return None;
    };
    let env_key = format!(
        "QTSS_STRATEGY_{}_BALANCE",
        strategy_env_suffix_normalized(strategy_name)
    );
    let raw = resolve_system_string(pool, "worker", wkey, &env_key, "").await;
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    Decimal::from_str(t).ok()
}

/// Her strateji için ayrı sanal bakiye — önce worker.strategy_*_balance, yoksa toplam / N.
pub async fn dry_gateway_for_strategy(strategy_name: &str, pool: &PgPool) -> Arc<DryRunGateway> {
    let default_total_s = resolve_system_string(
        pool,
        "worker",
        "strategy_runner_quote_balance_usdt",
        "QTSS_STRATEGY_RUNNER_QUOTE_BALANCE_USDT",
        "100000",
    )
    .await;
    let default_total =
        Decimal::from_str(default_total_s.trim()).unwrap_or_else(|_| Decimal::new(100_000, 0));
    let per_default = default_total / dry_runner_strategy_count_dec();

    let init = strategy_balance_usdt(strategy_name, pool)
        .await
        .unwrap_or(per_default);

    Arc::new(DryRunGateway::new(
        VirtualLedgerParams {
            initial_quote_balance: init,
        },
        CommissionPolicy::default(),
        None,
    ))
}

/// Tek gateway (`position_manager` dry) — toplam quote bakiyesi.
pub async fn dry_gateway_from_pool(pool: &PgPool) -> Arc<DryRunGateway> {
    let raw = resolve_system_string(
        pool,
        "worker",
        "strategy_runner_quote_balance_usdt",
        "QTSS_STRATEGY_RUNNER_QUOTE_BALANCE_USDT",
        "100000",
    )
    .await;
    let init = Decimal::from_str(raw.trim()).unwrap_or_else(|_| Decimal::new(100_000, 0));
    Arc::new(DryRunGateway::new(
        VirtualLedgerParams {
            initial_quote_balance: init,
        },
        CommissionPolicy::default(),
        None,
    ))
}

async fn gateway_for_strategy_async(name: &str, pool: &PgPool) -> Arc<dyn ExecutionGateway> {
    let dry = dry_gateway_for_strategy(name, pool).await;
    if let Some((org_id, user_id)) = qtss_strategy::paper_ledger_target_from_db(pool).await {
        info!(
            strategy = name,
            %org_id,
            %user_id,
            "dry gateway + paper ledger persist (worker.paper_ledger_enabled)"
        );
        Arc::new(qtss_strategy::PaperRecordingDryGateway::new(
            dry,
            pool.clone(),
            org_id,
            user_id,
            name,
        ))
    } else {
        dry
    }
}

pub async fn spawn_if_enabled(pool: &PgPool) {
    let on = resolve_worker_enabled_flag(
        pool,
        "worker",
        "strategy_runner_enabled",
        "QTSS_STRATEGY_RUNNER_ENABLED",
        false,
    )
    .await;
    if !on {
        return;
    }
    info!(
        "worker.strategy_runner_enabled: dry strateji döngüleri ({}) — worker.strategy_*_balance veya toplam/{}",
        DRY_RUNNER_STRATEGIES.join(", "),
        DRY_RUNNER_STRATEGIES.len(),
    );
    for &name in DRY_RUNNER_STRATEGIES {
        let p = pool.clone();
        let g = gateway_for_strategy_async(name, pool).await;
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

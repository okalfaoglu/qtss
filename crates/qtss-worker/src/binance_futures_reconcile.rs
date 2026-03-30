//! Periyodik Binance USDT-M futures açık emir ↔ yerel `exchange_orders` mutabakatı (`docs/QTSS_CURSOR_DEV_GUIDE.md` §9.1 madde 3).
//!
//! API `POST /api/v1/reconcile/binance/futures` ile aynı `reconcile_binance_futures_open_orders` mantığı.

use std::time::Duration;

use qtss_binance::{BinanceClient, BinanceClientConfig};
use qtss_execution::{
    reconcile_binance_futures_open_orders, ExchangeOrderVenueSnapshot, ReconcileReport,
};
use qtss_reconcile::{apply_binance_futures_open_orders_patch, BinanceOpenOrdersPatchConfig};
use qtss_storage::{ExchangeAccountRepository, ExchangeOrderRepository, ExchangeOrderRow};
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

fn enabled() -> bool {
    std::env::var("QTSS_RECONCILE_BINANCE_FUTURES_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn tick_secs() -> u64 {
    std::env::var("QTSS_RECONCILE_BINANCE_FUTURES_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600)
        .max(120)
}

fn patch_exchange_order_status_enabled() -> bool {
    std::env::var("QTSS_RECONCILE_BINANCE_FUTURES_PATCH_STATUS")
        .ok()
        .map(|s| {
            let t = s.trim();
            !(t == "0"
                || t.eq_ignore_ascii_case("false")
                || t.eq_ignore_ascii_case("no")
                || t.eq_ignore_ascii_case("off"))
        })
        .unwrap_or(true)
}

fn local_snapshots_futures(rows: Vec<ExchangeOrderRow>) -> Vec<ExchangeOrderVenueSnapshot> {
    rows.into_iter()
        .filter(|r| r.exchange == "binance" && r.segment == "futures")
        .filter_map(|r| {
            r.venue_order_id.map(|id| ExchangeOrderVenueSnapshot {
                venue_order_id: id,
                status: r.status,
            })
        })
        .collect()
}

fn log_report(user_id: Uuid, report: &ReconcileReport) {
    if report.mismatches > 0 {
        warn!(
            %user_id,
            mismatches = report.mismatches,
            checked_remote = report.checked_remote_orders,
            checked_local = report.checked_local_orders,
            local_submitted_not_open = report.local_submitted_not_open_on_venue,
            remote_unknown = report.remote_open_unknown_locally,
            notes = %report.notes,
            "binance_futures_reconcile: mismatch"
        );
    } else {
        tracing::debug!(
            %user_id,
            checked_remote = report.checked_remote_orders,
            checked_local = report.checked_local_orders,
            "binance_futures_reconcile: ok"
        );
    }
}

pub async fn binance_futures_reconcile_loop(pool: PgPool) {
    if !enabled() {
        info!("QTSS_RECONCILE_BINANCE_FUTURES_ENABLED kapalı — binance_futures_reconcile_loop çıkıyor");
        return;
    }
    let tick = Duration::from_secs(tick_secs());
    let accts = ExchangeAccountRepository::new(pool.clone());
    let orders = ExchangeOrderRepository::new(pool.clone());
    info!(
        poll_secs = tick.as_secs(),
        "binance_futures_reconcile_loop: periodic fapi openOrders vs exchange_orders"
    );
    loop {
        tokio::time::sleep(tick).await;
        let user_ids = match accts.list_user_ids_binance_segment("futures").await {
            Ok(u) => u,
            Err(e) => {
                warn!(%e, "binance_futures_reconcile: list_user_ids_binance_segment");
                continue;
            }
        };
        if user_ids.is_empty() {
            tracing::debug!("binance_futures_reconcile: no binance futures exchange_accounts");
            continue;
        }
        for user_id in user_ids {
            let creds = match accts.binance_for_user(user_id, "futures").await {
                Ok(c) => c,
                Err(e) => {
                    warn!(%e, %user_id, "binance_futures_reconcile: binance_for_user");
                    continue;
                }
            };
            let Some(creds) = creds else {
                continue;
            };
            let cfg = BinanceClientConfig::mainnet_with_keys(creds.api_key, creds.api_secret);
            let client = match BinanceClient::new(cfg) {
                Ok(c) => c,
                Err(e) => {
                    warn!(%e, %user_id, "binance_futures_reconcile: BinanceClient::new");
                    continue;
                }
            };
            let remote = match client.fapi_open_orders(None).await {
                Ok(r) => r,
                Err(e) => {
                    warn!(%e, %user_id, "binance_futures_reconcile: fapi_open_orders");
                    continue;
                }
            };
            let rows = match orders.list_for_user(user_id, 500).await {
                Ok(r) => r,
                Err(e) => {
                    warn!(%e, %user_id, "binance_futures_reconcile: list_for_user");
                    continue;
                }
            };
            let local = local_snapshots_futures(rows);
            let mut report = match reconcile_binance_futures_open_orders(&remote, &local) {
                Ok(r) => r,
                Err(e) => {
                    warn!(%e, %user_id, "binance_futures_reconcile: reconcile_binance_futures_open_orders");
                    continue;
                }
            };
            let patch_cfg =
                BinanceOpenOrdersPatchConfig::worker_futures(patch_exchange_order_status_enabled());
            if patch_cfg.refine_via_order_query || patch_cfg.patch_submitted_to_reconciled_not_open
            {
                match apply_binance_futures_open_orders_patch(
                    &orders, &client, user_id, &remote, &local, &patch_cfg,
                )
                .await
                {
                    Ok(n) if n > 0 => {
                        report.status_updates_applied = Some(n);
                        info!(
                            %user_id,
                            updated = n,
                            "binance_futures_reconcile: exchange_orders status patch"
                        );
                    }
                    Ok(_) => {}
                    Err(e) => warn!(
                        %e,
                        %user_id,
                        "binance_futures_reconcile: apply_binance_futures_open_orders_patch"
                    ),
                }
            }
            log_report(user_id, &report);
        }
    }
}

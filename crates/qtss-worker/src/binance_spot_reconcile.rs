//! Periyodik Binance spot açık emir ↔ yerel `exchange_orders` mutabakatı (`docs/QTSS_CURSOR_DEV_GUIDE.md` §9.1 madde 3).
//!
//! API `POST /api/v1/reconcile/binance` ile aynı `reconcile_binance_spot_open_orders` mantığı; `exchange_accounts` (spot)
//! kaydı olan her kullanıcı için periyodik `info!` / uyumsuzlukta `warn!`.

use std::time::Duration;

use qtss_binance::{BinanceClient, BinanceClientConfig};
use qtss_execution::{reconcile_binance_spot_open_orders, ExchangeOrderVenueSnapshot, ReconcileReport};
use qtss_reconcile::{
    apply_binance_spot_open_orders_patch, BinanceOpenOrdersPatchConfig,
};
use qtss_storage::{
    resolve_system_string, resolve_worker_enabled_flag, resolve_worker_tick_secs,
    ExchangeAccountRepository, ExchangeOrderRepository, ExchangeOrderRow,
};
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

fn local_snapshots_spot(rows: Vec<ExchangeOrderRow>) -> Vec<ExchangeOrderVenueSnapshot> {
    rows.into_iter()
        .filter(|r| r.exchange == "binance" && r.segment == "spot")
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
            "binance_spot_reconcile: mismatch"
        );
    } else {
        tracing::debug!(
            %user_id,
            checked_remote = report.checked_remote_orders,
            checked_local = report.checked_local_orders,
            "binance_spot_reconcile: ok"
        );
    }
}

pub async fn binance_spot_reconcile_loop(pool: PgPool) {
    let accts = ExchangeAccountRepository::new(pool.clone());
    let orders = ExchangeOrderRepository::new(pool.clone());
    info!("binance_spot_reconcile_loop: periodic openOrders vs exchange_orders (system_config + env yedek)");
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "worker",
            "reconcile_binance_spot_enabled",
            "QTSS_RECONCILE_BINANCE_SPOT_ENABLED",
            false,
        )
        .await;
        if !enabled {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }
        let tick_secs = resolve_worker_tick_secs(
            &pool,
            "worker",
            "reconcile_binance_spot_tick_secs",
            "QTSS_RECONCILE_BINANCE_SPOT_TICK_SECS",
            3600,
            120,
        )
        .await;
        let patch_on = resolve_worker_enabled_flag(
            &pool,
            "worker",
            "reconcile_binance_spot_patch_status",
            "QTSS_RECONCILE_BINANCE_SPOT_PATCH_STATUS",
            true,
        )
        .await;
        let refine_on = resolve_worker_enabled_flag(
            &pool,
            "worker",
            "reconcile_binance_spot_refine_order_status",
            "QTSS_RECONCILE_BINANCE_SPOT_REFINE_ORDER_STATUS",
            false,
        )
        .await;
        let refine_max: usize = resolve_system_string(
            &pool,
            "worker",
            "reconcile_binance_spot_refine_max",
            "QTSS_RECONCILE_BINANCE_SPOT_REFINE_MAX",
            "30",
        )
        .await
        .parse()
        .unwrap_or(30);
        let patch_cfg = BinanceOpenOrdersPatchConfig::spot_worker(refine_on, refine_max, patch_on);
        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
        let user_ids = match accts.list_user_ids_binance_segment("spot").await {
            Ok(u) => u,
            Err(e) => {
                warn!(%e, "binance_spot_reconcile: list_user_ids_binance_segment");
                continue;
            }
        };
        if user_ids.is_empty() {
            tracing::debug!("binance_spot_reconcile: no binance spot exchange_accounts");
            continue;
        }
        for user_id in user_ids {
            let creds = match accts.binance_for_user(user_id, "spot").await {
                Ok(c) => c,
                Err(e) => {
                    warn!(%e, %user_id, "binance_spot_reconcile: binance_for_user");
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
                    warn!(%e, %user_id, "binance_spot_reconcile: BinanceClient::new");
                    continue;
                }
            };
            let remote = match client.spot_open_orders(None).await {
                Ok(r) => r,
                Err(e) => {
                    warn!(%e, %user_id, "binance_spot_reconcile: spot_open_orders");
                    continue;
                }
            };
            let rows = match orders.list_for_user(user_id, 500).await {
                Ok(r) => r,
                Err(e) => {
                    warn!(%e, %user_id, "binance_spot_reconcile: list_for_user");
                    continue;
                }
            };
            let local = local_snapshots_spot(rows);
            let mut report = match reconcile_binance_spot_open_orders(&remote, &local) {
                Ok(r) => r,
                Err(e) => {
                    warn!(%e, %user_id, "binance_spot_reconcile: reconcile_binance_spot_open_orders");
                    continue;
                }
            };
            if patch_cfg.refine_via_order_query || patch_cfg.patch_submitted_to_reconciled_not_open {
                match apply_binance_spot_open_orders_patch(
                    &orders,
                    &client,
                    user_id,
                    &remote,
                    &local,
                    &patch_cfg,
                )
                .await
                {
                    Ok(n) if n > 0 => {
                        report.status_updates_applied = Some(n);
                        info!(
                            %user_id,
                            updated = n,
                            "binance_spot_reconcile: exchange_orders status patch"
                        );
                    }
                    Ok(_) => {}
                    Err(e) => warn!(
                        %e,
                        %user_id,
                        "binance_spot_reconcile: apply_binance_spot_open_orders_patch"
                    ),
                }
            }
            log_report(user_id, &report);
        }
    }
}

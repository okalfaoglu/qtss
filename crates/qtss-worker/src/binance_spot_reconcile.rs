//! Periyodik Binance spot açık emir ↔ yerel `exchange_orders` mutabakatı (`docs/QTSS_CURSOR_DEV_GUIDE.md` §9.1 madde 3).
//!
//! API `POST /api/v1/reconcile/binance` ile aynı `reconcile_binance_spot_open_orders` mantığı; `exchange_accounts` (spot)
//! kaydı olan her kullanıcı için periyodik `info!` / uyumsuzlukta `warn!`.

use std::time::Duration;

use qtss_binance::{BinanceClient, BinanceClientConfig};
use qtss_execution::{
    reconcile_binance_spot_open_orders, venue_order_ids_submitted_not_on_open_list,
    ExchangeOrderVenueSnapshot, ReconcileReport,
};
use qtss_storage::{ExchangeAccountRepository, ExchangeOrderRepository, ExchangeOrderRow};
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

fn enabled() -> bool {
    std::env::var("QTSS_RECONCILE_BINANCE_SPOT_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn tick_secs() -> u64 {
    std::env::var("QTSS_RECONCILE_BINANCE_SPOT_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600)
        .max(120)
}

/// Varsayılan açık; `0` / `false` / `off` ile kapatılır.
fn patch_exchange_order_status_enabled() -> bool {
    std::env::var("QTSS_RECONCILE_BINANCE_SPOT_PATCH_STATUS")
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
    if !enabled() {
        info!("QTSS_RECONCILE_BINANCE_SPOT_ENABLED kapalı — binance_spot_reconcile_loop çıkıyor");
        return;
    }
    let tick = Duration::from_secs(tick_secs());
    let accts = ExchangeAccountRepository::new(pool.clone());
    let orders = ExchangeOrderRepository::new(pool.clone());
    info!(
        poll_secs = tick.as_secs(),
        "binance_spot_reconcile_loop: periodic openOrders vs exchange_orders"
    );
    loop {
        tokio::time::sleep(tick).await;
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
            if patch_exchange_order_status_enabled() {
                match venue_order_ids_submitted_not_on_open_list(&remote, &local) {
                    Ok(ids) if !ids.is_empty() => {
                        match orders
                            .mark_submitted_reconciled_not_open_by_venue_ids(
                                user_id,
                                "binance",
                                "spot",
                                &ids,
                            )
                            .await
                        {
                            Ok(n) if n > 0 => {
                                report.status_updates_applied = Some(n);
                                info!(
                                    %user_id,
                                    updated = n,
                                    "binance_spot_reconcile: exchange_orders reconciled_not_open"
                                );
                            }
                            Ok(_) => {}
                            Err(e) => warn!(
                                %e,
                                %user_id,
                                "binance_spot_reconcile: mark_submitted_reconciled_not_open_by_venue_ids"
                            ),
                        }
                    }
                    Ok(_) => {}
                    Err(e) => warn!(
                        %e,
                        %user_id,
                        "binance_spot_reconcile: venue_order_ids_submitted_not_on_open_list"
                    ),
                }
            }
            log_report(user_id, &report);
        }
    }
}

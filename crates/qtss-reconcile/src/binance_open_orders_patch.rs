use std::collections::HashSet;

use qtss_binance::BinanceClient;
use qtss_execution::{
    exchange_order_status_from_binance_json, venue_order_ids_submitted_not_on_open_list,
    ExchangeOrderVenueSnapshot,
};
use qtss_storage::{ExchangeOrderRepository, StorageError};
use serde_json::Value;
use tracing::warn;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct BinanceOpenOrdersPatchConfig {
    /// `GET .../order` ile FILLED / CANCELED vb. netleştirme.
    pub refine_via_order_query: bool,
    pub refine_max_orders: usize,
    /// Kalan `submitted` satırlar için `reconciled_not_open`.
    pub patch_submitted_to_reconciled_not_open: bool,
}

impl BinanceOpenOrdersPatchConfig {
    pub fn worker_spot(patch_reconciled: bool) -> Self {
        Self {
            refine_via_order_query: std::env::var(
                "QTSS_RECONCILE_BINANCE_SPOT_REFINE_ORDER_STATUS",
            )
            .ok()
            .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on")),
            refine_max_orders: std::env::var("QTSS_RECONCILE_BINANCE_SPOT_REFINE_MAX")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30)
                .clamp(1, 200),
            patch_submitted_to_reconciled_not_open: patch_reconciled,
        }
    }

    pub fn worker_futures(patch_reconciled: bool) -> Self {
        Self {
            refine_via_order_query: std::env::var(
                "QTSS_RECONCILE_BINANCE_FUTURES_REFINE_ORDER_STATUS",
            )
            .ok()
            .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on")),
            refine_max_orders: std::env::var("QTSS_RECONCILE_BINANCE_FUTURES_REFINE_MAX")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30)
                .clamp(1, 200),
            patch_submitted_to_reconciled_not_open: patch_reconciled,
        }
    }

    /// HTTP reconcile uçları: varsayılan olarak `reconciled_not_open` yazar; order query isteğe bağlı (spot/futures env).
    pub fn http_spot() -> Self {
        Self::worker_spot(true)
    }

    pub fn http_futures() -> Self {
        Self::worker_futures(true)
    }
}

fn venue_id_to_u64(vid: i64) -> Option<u64> {
    if vid < 0 {
        return None;
    }
    Some(vid as u64)
}

/// Açık emir listesi ile karşılaştırma sonrası DB güncellemesi. Dönüş: etkilenen satır sayısı.
pub async fn apply_binance_spot_open_orders_patch(
    orders: &ExchangeOrderRepository,
    client: &BinanceClient,
    user_id: Uuid,
    remote_open: &Value,
    local: &[ExchangeOrderVenueSnapshot],
    cfg: &BinanceOpenOrdersPatchConfig,
) -> Result<u64, StorageError> {
    let ids = venue_order_ids_submitted_not_on_open_list(remote_open, local).map_err(|e| {
        StorageError::Other(format!("venue_order_ids_submitted_not_on_open_list: {e}"))
    })?;
    if ids.is_empty() {
        return Ok(0);
    }

    let mut updated: u64 = 0;
    let mut resolved: HashSet<i64> = HashSet::new();

    if cfg.refine_via_order_query {
        let pairs = orders
            .list_submitted_venue_symbol_pairs(user_id, "binance", "spot", &ids)
            .await?;
        for (vid, symbol) in pairs.into_iter().take(cfg.refine_max_orders) {
            let Some(oid) = venue_id_to_u64(vid) else {
                warn!(%user_id, venue_order_id = vid, "binance spot refine: skip negative order id");
                continue;
            };
            match client.spot_query_order(&symbol, Some(oid), None).await {
                Ok(json) => {
                    if let Some(st) = exchange_order_status_from_binance_json(&json) {
                        let n = orders
                            .update_submitted_status_and_venue_response(
                                user_id, "binance", "spot", vid, &st, &json,
                            )
                            .await?;
                        if n > 0 {
                            updated += n;
                            resolved.insert(vid);
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        %user_id,
                        %symbol,
                        venue_order_id = vid,
                        %e,
                        "binance spot refine: spot_query_order failed"
                    );
                }
            }
        }
    }

    if cfg.patch_submitted_to_reconciled_not_open {
        let remaining: Vec<i64> = ids
            .iter()
            .copied()
            .filter(|i| !resolved.contains(i))
            .collect();
        if !remaining.is_empty() {
            let n = orders
                .mark_submitted_reconciled_not_open_by_venue_ids(
                    user_id, "binance", "spot", &remaining,
                )
                .await?;
            updated += n;
        }
    }

    Ok(updated)
}

pub async fn apply_binance_futures_open_orders_patch(
    orders: &ExchangeOrderRepository,
    client: &BinanceClient,
    user_id: Uuid,
    remote_open: &Value,
    local: &[ExchangeOrderVenueSnapshot],
    cfg: &BinanceOpenOrdersPatchConfig,
) -> Result<u64, StorageError> {
    let ids = venue_order_ids_submitted_not_on_open_list(remote_open, local).map_err(|e| {
        StorageError::Other(format!("venue_order_ids_submitted_not_on_open_list: {e}"))
    })?;
    if ids.is_empty() {
        return Ok(0);
    }

    let mut updated: u64 = 0;
    let mut resolved: HashSet<i64> = HashSet::new();

    if cfg.refine_via_order_query {
        let pairs = orders
            .list_submitted_venue_symbol_pairs(user_id, "binance", "futures", &ids)
            .await?;
        for (vid, symbol) in pairs.into_iter().take(cfg.refine_max_orders) {
            let Some(oid) = venue_id_to_u64(vid) else {
                warn!(%user_id, venue_order_id = vid, "binance futures refine: skip negative order id");
                continue;
            };
            match client.fapi_query_order(&symbol, Some(oid), None).await {
                Ok(json) => {
                    if let Some(st) = exchange_order_status_from_binance_json(&json) {
                        let n = orders
                            .update_submitted_status_and_venue_response(
                                user_id, "binance", "futures", vid, &st, &json,
                            )
                            .await?;
                        if n > 0 {
                            updated += n;
                            resolved.insert(vid);
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        %user_id,
                        %symbol,
                        venue_order_id = vid,
                        %e,
                        "binance futures refine: fapi_query_order failed"
                    );
                }
            }
        }
    }

    if cfg.patch_submitted_to_reconciled_not_open {
        let remaining: Vec<i64> = ids
            .iter()
            .copied()
            .filter(|i| !resolved.contains(i))
            .collect();
        if !remaining.is_empty() {
            let n = orders
                .mark_submitted_reconciled_not_open_by_venue_ids(
                    user_id, "binance", "futures", &remaining,
                )
                .await?;
            updated += n;
        }
    }

    Ok(updated)
}

//! `exchangeInfo` → `exchanges` / `markets` / `instruments` senkronu (halka açık uçlar, API key gerekmez).
//! Tüm uygun semboller yazılır; `TRADING` olmayanlar `is_trading=false` + Binance `status` ile işaretlenir.
//! Senkron sonunda yanıtta artık görünmeyen satırlar (delist / kaldırılmış) `is_trading=false` yapılır.

use chrono::Utc;
use serde_json::{json, Value};

use crate::error::BinanceError;
use crate::BinanceClient;
use qtss_storage::{CatalogRepository, StorageError};

#[derive(Debug, Default, serde::Serialize)]
pub struct CatalogSyncStats {
    pub spot_instruments_upserted: usize,
    pub usdt_futures_instruments_upserted: usize,
    pub spot_deactivated_stale: u64,
    pub futures_deactivated_stale: u64,
}

fn storage_err(e: StorageError) -> BinanceError {
    BinanceError::Other(e.to_string())
}

fn spot_symbol_has_spot_permission(s: &Value) -> bool {
    let Some(perms) = s["permissions"].as_array() else {
        return true;
    };
    perms
        .iter()
        .filter_map(|x| x.as_str())
        .any(|p| p.eq_ignore_ascii_case("SPOT"))
}

/// Spot: `permissions` içinde `SPOT` olan tüm semboller (durum `TRADING` / `BREAK` / …).
pub async fn sync_spot_instruments(
    client: &BinanceClient,
    catalog: &CatalogRepository,
) -> Result<(usize, u64), BinanceError> {
    catalog
        .upsert_exchange("binance", "Binance", true, json!({}))
        .await
        .map_err(storage_err)?;

    let market = catalog
        .upsert_market("binance", "spot", "", Some("Spot"), true, json!({}))
        .await
        .map_err(storage_err)?;

    let sync_started = Utc::now();
    let info = client.spot_exchange_info(None).await?;
    let symbols = info["symbols"]
        .as_array()
        .ok_or_else(|| BinanceError::Other("exchangeInfo.symbols bekleniyor".into()))?;

    let mut n = 0usize;
    for s in symbols {
        if !spot_symbol_has_spot_permission(s) {
            continue;
        }

        let native = s["symbol"].as_str().unwrap_or("");
        if native.is_empty() {
            continue;
        }
        let status = s["status"].as_str().unwrap_or("UNKNOWN");
        let is_trading = status.eq_ignore_ascii_case("TRADING");
        let base = s["baseAsset"].as_str().unwrap_or("");
        let quote = s["quoteAsset"].as_str().unwrap_or("");
        let (price_f, lot_f) = extract_filters(s);
        let meta = s.clone();

        catalog
            .upsert_instrument(
                market.id,
                native,
                base,
                quote,
                status,
                is_trading,
                price_f,
                lot_f,
                meta,
            )
            .await
            .map_err(storage_err)?;
        n += 1;
    }

    let stale = catalog
        .deactivate_instruments_not_updated_since(market.id, sync_started)
        .await
        .map_err(storage_err)?;

    Ok((n, stale))
}

/// USDT-M sürekli vadeli sözleşmeler (`quoteAsset == USDT`, `contractType == PERPETUAL`) — tüm durumlar.
pub async fn sync_usdt_futures_instruments(
    client: &BinanceClient,
    catalog: &CatalogRepository,
) -> Result<(usize, u64), BinanceError> {
    catalog
        .upsert_exchange("binance", "Binance", true, json!({}))
        .await
        .map_err(storage_err)?;

    let market = catalog
        .upsert_market(
            "binance",
            "futures",
            "usdt_m",
            Some("USDT-M Futures"),
            true,
            json!({ "venue": "binance_fapi" }),
        )
        .await
        .map_err(storage_err)?;

    let sync_started = Utc::now();
    let info = client.fapi_exchange_info(None).await?;
    let symbols = info["symbols"]
        .as_array()
        .ok_or_else(|| BinanceError::Other("fapi exchangeInfo.symbols bekleniyor".into()))?;

    let mut n = 0usize;
    for s in symbols {
        if s["contractType"].as_str() != Some("PERPETUAL") {
            continue;
        }
        if s["quoteAsset"].as_str() != Some("USDT") {
            continue;
        }

        let native = s["symbol"].as_str().unwrap_or("");
        if native.is_empty() {
            continue;
        }
        let status = s["status"].as_str().unwrap_or("UNKNOWN");
        let is_trading = status.eq_ignore_ascii_case("TRADING");
        let base = s["baseAsset"].as_str().unwrap_or("");
        let quote = s["quoteAsset"].as_str().unwrap_or("");
        let (price_f, lot_f) = extract_filters(s);
        let meta = s.clone();

        catalog
            .upsert_instrument(
                market.id,
                native,
                base,
                quote,
                status,
                is_trading,
                price_f,
                lot_f,
                meta,
            )
            .await
            .map_err(storage_err)?;
        n += 1;
    }

    let stale = catalog
        .deactivate_instruments_not_updated_since(market.id, sync_started)
        .await
        .map_err(storage_err)?;

    Ok((n, stale))
}

pub async fn sync_full_binance_catalog(
    client: &BinanceClient,
    catalog: &CatalogRepository,
) -> Result<CatalogSyncStats, BinanceError> {
    let (spot_n, spot_stale) = sync_spot_instruments(client, catalog).await?;
    let (fut_n, fut_stale) = sync_usdt_futures_instruments(client, catalog).await?;
    Ok(CatalogSyncStats {
        spot_instruments_upserted: spot_n,
        usdt_futures_instruments_upserted: fut_n,
        spot_deactivated_stale: spot_stale,
        futures_deactivated_stale: fut_stale,
    })
}

fn extract_filters(s: &Value) -> (Option<Value>, Option<Value>) {
    let mut price = None;
    let mut lot = None;
    if let Some(filters) = s["filters"].as_array() {
        for f in filters {
            let t = f["filterType"].as_str().unwrap_or("");
            if t == "PRICE_FILTER" {
                price = Some(f.clone());
            }
            if t == "LOT_SIZE" {
                lot = Some(f.clone());
            }
        }
    }
    (price, lot)
}

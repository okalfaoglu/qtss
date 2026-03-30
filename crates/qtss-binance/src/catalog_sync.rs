//! `exchangeInfo` → `exchanges` / `markets` / `instruments` senkronu (halka açık uçlar, API key gerekmez).

use serde_json::{json, Value};

use crate::error::BinanceError;
use crate::BinanceClient;
use qtss_storage::{CatalogRepository, StorageError};

#[derive(Debug, Default, serde::Serialize)]
pub struct CatalogSyncStats {
    pub spot_instruments: usize,
    pub usdt_futures_instruments: usize,
}

fn storage_err(e: StorageError) -> BinanceError {
    BinanceError::Other(e.to_string())
}

/// Spot `TRADING` + `permissions` içinde `SPOT` olan semboller.
pub async fn sync_spot_instruments(
    client: &BinanceClient,
    catalog: &CatalogRepository,
) -> Result<usize, BinanceError> {
    catalog
        .upsert_exchange("binance", "Binance", true, json!({}))
        .await
        .map_err(storage_err)?;

    let market = catalog
        .upsert_market("binance", "spot", "", Some("Spot"), true, json!({}))
        .await
        .map_err(storage_err)?;

    let info = client.spot_exchange_info(None).await?;
    let symbols = info["symbols"]
        .as_array()
        .ok_or_else(|| BinanceError::Other("exchangeInfo.symbols bekleniyor".into()))?;

    let mut n = 0usize;
    for s in symbols {
        if s["status"].as_str() != Some("TRADING") {
            continue;
        }
        if let Some(perms) = s["permissions"].as_array() {
            let has_spot = perms
                .iter()
                .filter_map(|x| x.as_str())
                .any(|p| p.eq_ignore_ascii_case("SPOT"));
            if !has_spot {
                continue;
            }
        }

        let native = s["symbol"].as_str().unwrap_or("");
        if native.is_empty() {
            continue;
        }
        let base = s["baseAsset"].as_str().unwrap_or("");
        let quote = s["quoteAsset"].as_str().unwrap_or("");
        let (price_f, lot_f) = extract_filters(s);
        let meta = s.clone();

        catalog
            .upsert_instrument(
                market.id, native, base, quote, "TRADING", true, price_f, lot_f, meta,
            )
            .await
            .map_err(storage_err)?;
        n += 1;
    }
    Ok(n)
}

/// USDT-M sürekli vadeli sözleşmeler (`quoteAsset == USDT`, `contractType == PERPETUAL`).
pub async fn sync_usdt_futures_instruments(
    client: &BinanceClient,
    catalog: &CatalogRepository,
) -> Result<usize, BinanceError> {
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
        if s["status"].as_str() != Some("TRADING") {
            continue;
        }

        let native = s["symbol"].as_str().unwrap_or("");
        if native.is_empty() {
            continue;
        }
        let base = s["baseAsset"].as_str().unwrap_or("");
        let quote = s["quoteAsset"].as_str().unwrap_or("");
        let (price_f, lot_f) = extract_filters(s);
        let meta = s.clone();

        catalog
            .upsert_instrument(
                market.id, native, base, quote, "TRADING", true, price_f, lot_f, meta,
            )
            .await
            .map_err(storage_err)?;
        n += 1;
    }
    Ok(n)
}

pub async fn sync_full_binance_catalog(
    client: &BinanceClient,
    catalog: &CatalogRepository,
) -> Result<CatalogSyncStats, BinanceError> {
    let spot = sync_spot_instruments(client, catalog).await?;
    let fut = sync_usdt_futures_instruments(client, catalog).await?;
    Ok(CatalogSyncStats {
        spot_instruments: spot,
        usdt_futures_instruments: fut,
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

//! DefiLlama ücretsiz API entegrasyonu — TVL, stablecoin flow, DEX volume, protocol verileri.
//! Coinglass'ın (paralı) yerine geçer. API key gerektirmez.
//!
//! Endpoints:
//! - /v2/historicalChainTvl/{chain} — zincir TVL geçmişi
//! - /stablecoins — stablecoin piyasa verileri
//! - /overview/dexs — DEX hacim verileri
//! - /protocols — tüm protokol listesi + TVL

use std::time::Duration;

use qtss_storage::{resolve_worker_enabled_flag, resolve_worker_tick_secs, upsert_data_snapshot};
use reqwest::Client;
use serde_json::{json, Value as JsonValue};
use sqlx::PgPool;
use tracing::{info, warn};

const DEFILLAMA_BASE: &str = "https://api.llama.fi";
const DEFILLAMA_STABLECOINS: &str = "https://stablecoins.llama.fi";

async fn fetch_json(client: &Client, url: &str) -> Result<JsonValue, String> {
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("HTTP error: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<JsonValue>()
        .await
        .map_err(|e| format!("JSON parse: {e}"))
}

/// Ethereum TVL trend — son 7 günlük değişimi hesaplar.
async fn fetch_chain_tvl(client: &Client) -> Result<JsonValue, String> {
    let url = format!("{DEFILLAMA_BASE}/v2/historicalChainTvl/Ethereum");
    let data = fetch_json(client, &url).await?;
    let arr = data.as_array().ok_or("expected array")?;
    if arr.len() < 7 {
        return Err("insufficient TVL data".into());
    }
    let latest = arr.last().and_then(|v| v.get("tvl")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let week_ago = arr.get(arr.len().saturating_sub(8))
        .and_then(|v| v.get("tvl"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let change_pct = if week_ago > 0.0 { (latest - week_ago) / week_ago * 100.0 } else { 0.0 };

    Ok(json!({
        "chain": "ethereum",
        "tvl_latest": latest,
        "tvl_7d_ago": week_ago,
        "tvl_change_pct_7d": change_pct,
        "data_points": arr.len(),
    }))
}

/// Stablecoin toplam market cap ve değişim.
async fn fetch_stablecoin_flow(client: &Client) -> Result<JsonValue, String> {
    let url = format!("{DEFILLAMA_STABLECOINS}/stablecoins?includePrices=false");
    let data = fetch_json(client, &url).await?;

    let pegged_assets = data.get("peggedAssets").and_then(|v| v.as_array());
    if pegged_assets.is_none() {
        return Err("no peggedAssets".into());
    }
    let assets = pegged_assets.unwrap();

    // Top 5 stablecoin (USDT, USDC, DAI, BUSD, TUSD)
    let mut total_mcap = 0.0_f64;
    let mut total_mcap_7d_ago = 0.0_f64;
    let mut details: Vec<JsonValue> = Vec::new();

    for asset in assets.iter().take(10) {
        let name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let symbol = asset.get("symbol").and_then(|v| v.as_str()).unwrap_or("?");

        // circulating peggedUSD
        let mcap = asset
            .pointer("/circulating/peggedUSD")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let mcap_7d = asset
            .pointer("/circulatingPrevDay/peggedUSD")
            .and_then(|v| v.as_f64())
            .unwrap_or(mcap); // fallback to current if missing

        total_mcap += mcap;
        total_mcap_7d_ago += mcap_7d;

        if mcap > 1_000_000_000.0 {
            details.push(json!({
                "symbol": symbol,
                "name": name,
                "mcap_b": (mcap / 1e9 * 100.0).round() / 100.0,
            }));
        }
    }

    let flow_pct = if total_mcap_7d_ago > 0.0 {
        (total_mcap - total_mcap_7d_ago) / total_mcap_7d_ago * 100.0
    } else {
        0.0
    };

    Ok(json!({
        "total_stablecoin_mcap_b": (total_mcap / 1e9 * 100.0).round() / 100.0,
        "stablecoin_flow_pct": flow_pct,
        "top_stablecoins": details,
    }))
}

/// DEX toplam hacim (24h).
async fn fetch_dex_volume(client: &Client) -> Result<JsonValue, String> {
    let url = format!("{DEFILLAMA_BASE}/overview/dexs?excludeTotalDataChart=true&excludeTotalDataChartBreakdown=true");
    let data = fetch_json(client, &url).await?;

    let total_24h = data
        .get("totalDataChart")
        .or_else(|| data.get("total24h"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let change_1d = data.get("change_1d").and_then(|v| v.as_f64()).unwrap_or(0.0);

    // Protocol bazlı top 5
    let protocols = data.get("protocols").and_then(|v| v.as_array());
    let mut top_dexes: Vec<JsonValue> = Vec::new();
    if let Some(protos) = protocols {
        for p in protos.iter().take(5) {
            let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let vol = p.get("total24h").and_then(|v| v.as_f64()).unwrap_or(0.0);
            top_dexes.push(json!({ "name": name, "vol_24h_m": (vol / 1e6).round() }));
        }
    }

    Ok(json!({
        "total_dex_volume_24h_b": (total_24h / 1e9 * 100.0).round() / 100.0,
        "dex_volume_change_1d_pct": change_1d,
        "top_dexes": top_dexes,
    }))
}

/// DefiLlama ana loop — tüm verileri çeker, `data_snapshots` tablosuna yazar.
pub async fn defillama_loop(pool: PgPool) {
    let client = match Client::builder().timeout(Duration::from_secs(60)).build() {
        Ok(c) => c,
        Err(e) => {
            warn!(%e, "defillama reqwest client");
            return;
        }
    };

    info!("defillama_loop başlatıldı");

    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool, "worker", "defillama_enabled", "QTSS_DEFILLAMA_ENABLED", true,
        ).await;
        if !enabled {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }

        let tick = resolve_worker_tick_secs(
            &pool, "worker", "defillama_tick_secs", "QTSS_DEFILLAMA_TICK_SECS",
            1800, 300,
        ).await;

        // 1) Chain TVL
        match fetch_chain_tvl(&client).await {
            Ok(payload) => {
                if let Err(e) = upsert_data_snapshot(&pool, "defillama_chain_tvl", &json!({}), Some(&payload), None, None).await {
                    warn!(%e, "defillama_chain_tvl snapshot");
                } else {
                    info!("defillama_chain_tvl güncellendi");
                }
            }
            Err(e) => warn!(%e, "defillama_chain_tvl fetch"),
        }

        // 2) Stablecoin flow
        match fetch_stablecoin_flow(&client).await {
            Ok(payload) => {
                if let Err(e) = upsert_data_snapshot(&pool, "defillama_stablecoin_flow", &json!({}), Some(&payload), None, None).await {
                    warn!(%e, "defillama_stablecoin_flow snapshot");
                } else {
                    info!("defillama_stablecoin_flow güncellendi");
                }
            }
            Err(e) => warn!(%e, "defillama_stablecoin_flow fetch"),
        }

        // 3) DEX volume
        match fetch_dex_volume(&client).await {
            Ok(payload) => {
                if let Err(e) = upsert_data_snapshot(&pool, "defillama_dex_volume", &json!({}), Some(&payload), None, None).await {
                    warn!(%e, "defillama_dex_volume snapshot");
                } else {
                    info!("defillama_dex_volume güncellendi");
                }
            }
            Err(e) => warn!(%e, "defillama_dex_volume fetch"),
        }

        tokio::time::sleep(Duration::from_secs(tick)).await;
    }
}

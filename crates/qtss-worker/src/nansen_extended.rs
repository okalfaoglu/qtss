//! Nansen smart-money / TGM / profiler HTTP loops → `data_snapshots` (dev guide ADIM 1–3, §3.8).
//!
//! Her döngü `NANSEN_*_ENABLED=0|false|off|no` ile kapatılabilir (varsayılan: açık).

use std::time::{Duration, Instant};

use qtss_storage::{
    list_enabled_engine_symbols, upsert_data_snapshot, AppConfigRepository,
};
use reqwest::Client;
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{info, warn};

use crate::data_sources::registry::{
    NANSEN_FLOW_INTEL_DATA_KEY, NANSEN_HOLDINGS_DATA_KEY, NANSEN_NETFLOWS_DATA_KEY,
    NANSEN_PERP_TRADES_DATA_KEY, NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY,
    NANSEN_WHALE_WATCHLIST_CONFIG_KEY, NANSEN_WHO_BOUGHT_DATA_KEY,
};

fn nansen_api_base() -> String {
    std::env::var("NANSEN_API_BASE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| qtss_nansen::default_api_base().to_string())
}

fn loop_enabled(var: &str) -> bool {
    !matches!(
        std::env::var(var).ok().as_deref().map(str::trim),
        Some("0") | Some("false") | Some("no") | Some("off")
    )
}

fn default_pagination_body() -> Value {
    json!({ "pagination": { "page": 1, "per_page": 100 } })
}

fn leaderboard_body() -> Value {
    json!({ "pagination": { "page": 1, "per_page": 20 } })
}

fn meta_json(
    meta: &qtss_nansen::NansenResponseMeta,
    insufficient: bool,
    fetch_ms: u64,
) -> Value {
    let mut m = json!({
        "qtss_fetch_duration_ms": fetch_ms,
        "nansen_insufficient_credits": insufficient,
    });
    if let Some(x) = &meta.credits_used {
        m["x_nansen_credits_used"] = json!(x);
    }
    if let Some(x) = &meta.credits_remaining {
        m["x_nansen_credits_remaining"] = json!(x);
    }
    if let Some(x) = &meta.rate_limit_remaining {
        m["ratelimit_remaining"] = json!(x);
    }
    m
}

async fn persist_nansen_result(
    pool: &PgPool,
    source_key: &str,
    request: &Value,
    res: Result<(Value, qtss_nansen::NansenResponseMeta), qtss_nansen::NansenError>,
    started: Instant,
) {
    let ms = started.elapsed().as_millis() as u64;
    match res {
        Ok((v, meta)) => {
            let m = meta_json(&meta, false, ms);
            if let Err(e) = upsert_data_snapshot(
                pool,
                source_key,
                request,
                Some(&v),
                Some(&m),
                None,
            )
            .await
            {
                warn!(%e, %source_key, "nansen_extended upsert_data_snapshot");
            } else {
                info!(%source_key, "nansen data_snapshots güncellendi");
            }
        }
        Err(e) => {
            let insufficient = e.is_insufficient_credits();
            let err_s = e.to_string();
            let m = json!({
                "qtss_fetch_duration_ms": ms,
                "nansen_insufficient_credits": insufficient,
                "error": err_s.chars().take(500).collect::<String>(),
            });
            if let Err(e2) = upsert_data_snapshot(pool, source_key, request, None, Some(&m), Some(&err_s)).await
            {
                warn!(%e2, %source_key, "nansen_extended error upsert");
            }
            if insufficient {
                qtss_common::log_critical(
                    "qtss_worker_nansen_extended",
                    "Nansen extended endpoint: insufficient credits (403).",
                );
            } else {
                warn!(%source_key, %err_s, "nansen_extended request failed");
            }
        }
    }
}

fn insufficient_sleep_default(tick: u64) -> u64 {
    std::env::var("NANSEN_INSUFFICIENT_CREDITS_SLEEP_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600)
        .max(tick)
}

async fn nansen_client() -> Option<Client> {
    match Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
    {
        Ok(c) => Some(c),
        Err(e) => {
            warn!(%e, "nansen_extended: reqwest client");
            None
        }
    }
}

pub async fn nansen_netflows_loop(pool: PgPool) {
    if !loop_enabled("NANSEN_NETFLOWS_ENABLED") {
        info!("NANSEN_NETFLOWS_ENABLED kapalı — nansen_netflows döngüsü çıkıyor");
        return;
    }
    let tick: u64 = std::env::var("NANSEN_NETFLOWS_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800)
        .max(900);
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    let body = default_pagination_body();
    loop {
        let Some(key) = std::env::var("NANSEN_API_KEY").ok().filter(|s| !s.trim().is_empty()) else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let started = Instant::now();
        let res = qtss_nansen::post_smart_money_netflows(&client, &base, key.trim(), &body).await;
        let mut next = tick;
        if res.as_ref().err().map(|e| e.is_insufficient_credits()).unwrap_or(false) {
            next = insufficient_sleep_default(tick);
        }
        persist_nansen_result(&pool, NANSEN_NETFLOWS_DATA_KEY, &body, res, started).await;
        tokio::time::sleep(Duration::from_secs(next)).await;
    }
}

pub async fn nansen_holdings_loop(pool: PgPool) {
    if !loop_enabled("NANSEN_HOLDINGS_ENABLED") {
        info!("NANSEN_HOLDINGS_ENABLED kapalı — nansen_holdings döngüsü çıkıyor");
        return;
    }
    let tick: u64 = std::env::var("NANSEN_HOLDINGS_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800)
        .max(900);
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    let body = default_pagination_body();
    loop {
        let Some(key) = std::env::var("NANSEN_API_KEY").ok().filter(|s| !s.trim().is_empty()) else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let started = Instant::now();
        let res = qtss_nansen::post_smart_money_holdings(&client, &base, key.trim(), &body).await;
        let mut next = tick;
        if res.as_ref().err().map(|e| e.is_insufficient_credits()).unwrap_or(false) {
            next = insufficient_sleep_default(tick);
        }
        persist_nansen_result(&pool, NANSEN_HOLDINGS_DATA_KEY, &body, res, started).await;
        tokio::time::sleep(Duration::from_secs(next)).await;
    }
}

pub async fn nansen_perp_trades_loop(pool: PgPool) {
    if !loop_enabled("NANSEN_PERP_TRADES_ENABLED") {
        info!("NANSEN_PERP_TRADES_ENABLED kapalı — nansen_perp_trades döngüsü çıkıyor");
        return;
    }
    let tick: u64 = std::env::var("NANSEN_PERP_TRADES_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800)
        .max(900);
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    let body = default_pagination_body();
    loop {
        let Some(key) = std::env::var("NANSEN_API_KEY").ok().filter(|s| !s.trim().is_empty()) else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let started = Instant::now();
        let res =
            qtss_nansen::post_smart_money_perp_trades(&client, &base, key.trim(), &body).await;
        let mut next = tick;
        if res.as_ref().err().map(|e| e.is_insufficient_credits()).unwrap_or(false) {
            next = insufficient_sleep_default(tick);
        }
        persist_nansen_result(&pool, NANSEN_PERP_TRADES_DATA_KEY, &body, res, started).await;
        tokio::time::sleep(Duration::from_secs(next)).await;
    }
}

pub async fn nansen_who_bought_loop(pool: PgPool) {
    if !loop_enabled("NANSEN_WHO_BOUGHT_ENABLED") {
        info!("NANSEN_WHO_BOUGHT_ENABLED kapalı — nansen_who_bought_sold döngüsü çıkıyor");
        return;
    }
    let tick: u64 = std::env::var("NANSEN_WHO_BOUGHT_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800)
        .max(900);
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    let body = default_pagination_body();
    loop {
        let Some(key) = std::env::var("NANSEN_API_KEY").ok().filter(|s| !s.trim().is_empty()) else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let started = Instant::now();
        let res = qtss_nansen::post_tgm_who_bought_sold(&client, &base, key.trim(), &body).await;
        let mut next = tick;
        if res.as_ref().err().map(|e| e.is_insufficient_credits()).unwrap_or(false) {
            next = insufficient_sleep_default(tick);
        }
        persist_nansen_result(&pool, NANSEN_WHO_BOUGHT_DATA_KEY, &body, res, started).await;
        tokio::time::sleep(Duration::from_secs(next)).await;
    }
}

pub async fn nansen_flow_intel_loop(pool: PgPool) {
    if !loop_enabled("NANSEN_FLOW_INTEL_ENABLED") {
        info!("NANSEN_FLOW_INTEL_ENABLED kapalı — nansen_flow_intelligence döngüsü çıkıyor");
        return;
    }
    let tick: u64 = std::env::var("NANSEN_FLOW_INTEL_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(900)
        .max(600);
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    loop {
        let Some(api_key) = std::env::var("NANSEN_API_KEY").ok().filter(|s| !s.trim().is_empty()) else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let map_v = AppConfigRepository::get_value_json(&pool, "nansen_flow_intel_by_symbol")
            .await
            .ok()
            .flatten();
        let Some(map_v) = map_v else {
            tracing::debug!("nansen_flow_intel: app_config nansen_flow_intel_by_symbol yok — atlanıyor");
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let Some(obj) = map_v.as_object() else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        if obj.is_empty() {
            tracing::debug!("nansen_flow_intel: boş harita — atlanıyor");
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        }
        let rows = list_enabled_engine_symbols(&pool).await.unwrap_or_default();
        let mut any = false;
        for es in rows {
            let sym = es.symbol.trim().to_uppercase();
            let body = obj.get(&sym).cloned().or_else(|| obj.get(sym.as_str()).cloned());
            let Some(body) = body else {
                continue;
            };
            if !body.is_object() {
                continue;
            }
            any = true;
            let started = Instant::now();
            let res =
                qtss_nansen::post_tgm_flow_intelligence(&client, &base, api_key.trim(), &body).await;
            persist_nansen_result(&pool, NANSEN_FLOW_INTEL_DATA_KEY, &body, res, started).await;
        }
        if !any {
            tracing::debug!("nansen_flow_intel: eşleşen engine_symbol yok");
        }
        tokio::time::sleep(Duration::from_secs(tick)).await;
    }
}

fn extract_wallet_addresses(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let rows = v
        .get("data")
        .and_then(|d| d.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    for row in rows {
        for key in ["address", "wallet", "wallet_address", "user_address"] {
            if let Some(s) = row.get(key).and_then(|x| x.as_str()) {
                let t = s.trim();
                if !t.is_empty() {
                    out.push(t.to_string());
                }
                break;
            }
        }
        if out.len() >= 20 {
            break;
        }
    }
    out
}

fn merge_position_rows(into: &mut Vec<Value>, v: &Value) {
    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
        for x in arr.iter().take(500) {
            into.push(x.clone());
        }
    }
}

pub async fn nansen_perp_leaderboard_loop(pool: PgPool) {
    if !loop_enabled("NANSEN_PERP_LEADERBOARD_ENABLED") {
        info!("NANSEN_PERP_LEADERBOARD_ENABLED kapalı — perp_leaderboard döngüsü çıkıyor");
        return;
    }
    let tick: u64 = std::env::var("NANSEN_PERP_LEADERBOARD_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(604_800)
        .max(3_600);
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    let body = leaderboard_body();
    let repo = AppConfigRepository::new(pool.clone());
    loop {
        let Some(api_key) = std::env::var("NANSEN_API_KEY").ok().filter(|s| !s.trim().is_empty()) else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let started = Instant::now();
        let res = qtss_nansen::post_profiler_perp_leaderboard(&client, &base, api_key.trim(), &body).await;
        if let Ok((ref v, _)) = res {
            let wallets = extract_wallet_addresses(v);
            let cfg = json!({
                "wallets": wallets,
                "last_updated": chrono::Utc::now().to_rfc3339(),
            });
            if let Err(e) = repo
                .upsert(
                    NANSEN_WHALE_WATCHLIST_CONFIG_KEY,
                    cfg,
                    Some("Whale watchlist from Nansen perp-leaderboard"),
                    None,
                )
                .await
            {
                warn!(%e, "nansen_whale_watchlist app_config upsert");
            }
        }
        persist_nansen_result(
            &pool,
            "nansen_perp_leaderboard",
            &body,
            res,
            started,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(tick)).await;
    }
}

pub async fn nansen_whale_perp_aggregate_loop(pool: PgPool) {
    if !loop_enabled("NANSEN_WHALE_PERP_AGGREGATE_ENABLED") {
        info!("NANSEN_WHALE_PERP_AGGREGATE_ENABLED kapalı — whale perp aggregate döngüsü çıkıyor");
        return;
    }
    let tick: u64 = std::env::var("NANSEN_WHALE_PERP_POSITIONS_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800)
        .max(600);
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    loop {
        let Some(api_key) = std::env::var("NANSEN_API_KEY").ok().filter(|s| !s.trim().is_empty()) else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let wl = AppConfigRepository::get_value_json(&pool, NANSEN_WHALE_WATCHLIST_CONFIG_KEY)
            .await
            .ok()
            .flatten();
        let wallets: Vec<String> = wl
            .as_ref()
            .and_then(|v| v.get("wallets"))
            .and_then(|w| w.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .take(20)
                    .collect()
            })
            .unwrap_or_default();
        if wallets.is_empty() {
            tracing::debug!("nansen_whale_perp: watchlist boş — leaderboard bekleniyor");
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        }
        let started = Instant::now();
        let mut merged: Vec<Value> = Vec::new();
        let request = json!({ "wallets": wallets.clone(), "count": wallets.len() });
        let mut last_err: Option<qtss_nansen::NansenError> = None;
        for w in &wallets {
            let body = json!({ "address": w });
            match qtss_nansen::post_profiler_perp_positions(&client, &base, api_key.trim(), &body).await {
                Ok((v, _)) => merge_position_rows(&mut merged, &v),
                Err(e) => last_err = Some(e),
            }
        }
        let elapsed = started.elapsed().as_millis() as u64;
        if merged.is_empty() {
            if let Some(e) = last_err {
                let insufficient = e.is_insufficient_credits();
                let err_s = e.to_string();
                let m = json!({
                    "qtss_fetch_duration_ms": elapsed,
                    "nansen_insufficient_credits": insufficient,
                });
                let _ = upsert_data_snapshot(
                    &pool,
                    NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY,
                    &request,
                    None,
                    Some(&m),
                    Some(err_s.as_str()),
                )
                .await;
            }
        } else {
            let response = json!({ "data": merged });
            let m = json!({ "qtss_fetch_duration_ms": elapsed, "nansen_insufficient_credits": false });
            if let Err(e) = upsert_data_snapshot(
                &pool,
                NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY,
                &request,
                Some(&response),
                Some(&m),
                None,
            )
            .await
            {
                warn!(%e, "nansen_whale_perp aggregate upsert");
            } else {
                info!("nansen_whale_perp_aggregate güncellendi");
            }
        }
        tokio::time::sleep(Duration::from_secs(tick)).await;
    }
}

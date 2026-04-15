//! Nansen smart-money / TGM / profiler HTTP loops → `data_snapshots` (dev guide ADIM 1–3, §3.8).
//!
//! Her döngü `NANSEN_*_ENABLED=0|false|off|no` ile kapatılabilir (varsayılan: açık).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use qtss_storage::{
    list_enabled_engine_symbols, resolve_nansen_loop_default_on, resolve_nansen_loop_opt_in,
    resolve_worker_enabled_flag, resolve_worker_tick_secs, upsert_data_snapshot, AppConfigRepository,
};
use reqwest::Client;
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{debug, info, warn};

use crate::data_sources::registry::{
    NANSEN_FLOW_INTEL_DATA_KEY, NANSEN_HOLDINGS_DATA_KEY, NANSEN_NETFLOWS_DATA_KEY,
    NANSEN_PERP_LEADERBOARD_DATA_KEY, NANSEN_PERP_SCREENER_DATA_KEY, NANSEN_PERP_TRADES_DATA_KEY,
    NANSEN_SMART_MONEY_DEX_TRADES_DATA_KEY, NANSEN_TGM_DEX_TRADES_DATA_KEY,
    NANSEN_TGM_FLOWS_DATA_KEY, NANSEN_TGM_HOLDERS_DATA_KEY, NANSEN_TGM_INDICATORS_DATA_KEY,
    NANSEN_TGM_PERP_POSITIONS_DATA_KEY, NANSEN_TGM_PERP_TRADES_DATA_KEY,
    NANSEN_TGM_TOKEN_INFORMATION_DATA_KEY, NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY,
    NANSEN_WHALE_WATCHLIST_KEY, NANSEN_WHO_BOUGHT_DATA_KEY,
};

/// Read Nansen API key: DB (`worker.nansen_api_key`) first, env
/// `NANSEN_API_KEY` as fallback. Returns `None` when blank/missing so
/// the loop can sleep-skip cleanly (CLAUDE.md #2).
async fn resolve_nansen_api_key(pool: &PgPool) -> Option<String> {
    let s = qtss_storage::resolve_system_string(
        pool, "worker", "nansen_api_key", "NANSEN_API_KEY", "",
    ).await;
    if s.is_empty() { None } else { Some(s) }
}

fn nansen_api_base() -> String {
    std::env::var("NANSEN_API_BASE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| qtss_nansen::default_api_base().to_string())
}

fn default_pagination_body() -> Value {
    let per_page: u64 = std::env::var("NANSEN_PER_PAGE")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(500);
    json!({ "pagination": { "page": 1, "per_page": per_page } })
}

fn json_merge(base: &mut Value, extra: Value) {
    let Some(bo) = base.as_object_mut() else {
        return;
    };
    if let Some(eo) = extra.as_object() {
        for (k, v) in eo {
            bo.insert(k.clone(), v.clone());
        }
    }
}

/// `smart-money/netflow` + `holdings`: API `chains` dizisi bekler.
fn default_chains_for_smart_money() -> Value {
    std::env::var("NANSEN_SMART_MONEY_CHAINS_JSON")
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(s.trim()).ok())
        .unwrap_or_else(|| json!(["all"]))
}

fn default_netflows_request_body() -> Value {
    let mut body = default_pagination_body();
    json_merge(
        &mut body,
        json!({ "chains": default_chains_for_smart_money() }),
    );
    body
}

fn default_holdings_request_body() -> Value {
    let mut body = default_pagination_body();
    json_merge(
        &mut body,
        json!({ "chains": default_chains_for_smart_money() }),
    );
    body
}

static WHO_BOUGHT_MISSING_CONFIG_LOGGED: AtomicBool = AtomicBool::new(false);

/// Tam gövde: `NANSEN_WHO_BOUGHT_BODY_JSON` (JSON object).
fn who_bought_body_from_env_json() -> Option<Value> {
    let raw = std::env::var("NANSEN_WHO_BOUGHT_BODY_JSON").ok()?;
    serde_json::from_str::<Value>(raw.trim())
        .ok()
        .filter(|v| v.is_object())
}

/// `tgm/who-bought-sold`: API `chain` + `token_address` zorunlu.
fn who_bought_body_chain_and_token() -> Option<Value> {
    let token = std::env::var("NANSEN_WHO_BOUGHT_TOKEN_ADDRESS")
        .ok()
        .filter(|s| !s.trim().is_empty())?;
    let chain = std::env::var("NANSEN_WHO_BOUGHT_CHAIN").unwrap_or_else(|_| "ethereum".into());
    let mut body = default_pagination_body();
    json_merge(
        &mut body,
        json!({ "chain": chain.trim(), "token_address": token.trim() }),
    );
    Some(body)
}

fn resolve_who_bought_request_body() -> Option<Value> {
    who_bought_body_from_env_json().or_else(who_bought_body_chain_and_token)
}

fn leaderboard_body() -> Value {
    json!({ "pagination": { "page": 1, "per_page": 20 } })
}

/// `POST /api/v1/tgm/perp-pnl-leaderboard` — Nansen şeması `token_symbol` + `date` zorunlu.
/// Tüm gövde: `NANSEN_PERP_LEADERBOARD_BODY_JSON` (JSON object).
fn perp_pnl_leaderboard_request_body() -> Value {
    if let Ok(raw) = std::env::var("NANSEN_PERP_LEADERBOARD_BODY_JSON") {
        if let Ok(v) = serde_json::from_str::<Value>(raw.trim()) {
            if v.is_object() {
                return v;
            }
        }
    }
    let token = std::env::var("NANSEN_PERP_LEADERBOARD_TOKEN_SYMBOL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_uppercase())
        .unwrap_or_else(|| "BTC".into());
    let lookback: i64 = std::env::var("NANSEN_PERP_LEADERBOARD_LOOKBACK_DAYS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7)
        .clamp(1, 90);
    let to = chrono::Utc::now().date_naive();
    let day_span = (lookback - 1).max(0);
    let from = to
        .checked_sub_signed(chrono::Duration::days(day_span))
        .unwrap_or(to);
    let mut body = json!({
        "token_symbol": token,
        "date": {
            "from": from.format("%Y-%m-%d").to_string(),
            "to": to.format("%Y-%m-%d").to_string(),
        },
    });
    json_merge(&mut body, leaderboard_body());
    body
}

fn meta_json(meta: &qtss_nansen::NansenResponseMeta, insufficient: bool, fetch_ms: u64) -> Value {
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
            if let Err(e) =
                upsert_data_snapshot(pool, source_key, request, Some(&v), Some(&m), None).await
            {
                warn!(%e, %source_key, "nansen_extended upsert_data_snapshot");
            } else {
                info!(%source_key, "nansen data_snapshots güncellendi");
            }
            // Persist raw flow rows for AI consumption
            persist_raw_flows(pool, source_key, &v).await;
        }
        Err(e) => {
            let insufficient = e.is_insufficient_credits();
            let err_s = e.to_string();
            let m = json!({
                "qtss_fetch_duration_ms": ms,
                "nansen_insufficient_credits": insufficient,
                "error": err_s.chars().take(500).collect::<String>(),
            });
            if let Err(e2) =
                upsert_data_snapshot(pool, source_key, request, None, Some(&m), Some(&err_s)).await
            {
                warn!(%e2, %source_key, "nansen_extended error upsert");
            }
            if insufficient {
                qtss_common::log_critical(
                    "qtss_worker_nansen_extended",
                    "Nansen extended endpoint: insufficient credits (403).",
                );
            } else if e.http_status() == Some(404) {
                debug!(
                    %source_key,
                    %err_s,
                    "nansen_extended request failed (HTTP 404; path/plan — NANSEN_PERP_LEADERBOARD_PATH / Nansen docs)"
                );
            } else {
                warn!(%source_key, %err_s, "nansen_extended request failed");
            }
        }
    }
}

/// Map source_key to source_type for raw flow extraction.
fn source_type_for_key(source_key: &str) -> Option<&'static str> {
    match source_key {
        "nansen_netflows" => Some("netflow"),
        "nansen_holdings" => Some("holdings"),
        "nansen_smart_money_dex_trades" => Some("dex_trades"),
        "nansen_flow_intelligence" => Some("flow_intel"),
        _ => None,
    }
}

async fn persist_raw_flows(pool: &PgPool, source_key: &str, response: &Value) {
    let Some(source_type) = source_type_for_key(source_key) else {
        return; // not a flow-type snapshot
    };
    let raw_rows = qtss_onchain::nansen_enriched::extract_raw_flow_rows(source_type, response);
    if raw_rows.is_empty() {
        return;
    }
    let now = chrono::Utc::now();
    let inserts: Vec<qtss_storage::nansen_enriched::NansenRawFlowInsert<'_>> = raw_rows
        .iter()
        .map(|r| qtss_storage::nansen_enriched::NansenRawFlowInsert {
            source_type: &r.source_type,
            chain: r.chain.as_deref(),
            token_symbol: r.token_symbol.as_deref(),
            token_address: r.token_address.as_deref(),
            engine_symbol: None, // mapped later by enriched analyzer
            direction: r.direction.as_deref(),
            value_usd: r.value_usd,
            balance_pct_change: r.balance_pct_change,
            raw_row: &r.raw_row,
            snapshot_at: now,
        })
        .collect();
    match qtss_storage::nansen_enriched::insert_raw_flows(pool, &inserts).await {
        Ok(n) => debug!(%source_key, rows = n, "nansen raw flows persisted"),
        Err(e) => warn!(%e, %source_key, "nansen raw flows insert failed"),
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
    match Client::builder().timeout(Duration::from_secs(120)).build() {
        Ok(c) => Some(c),
        Err(e) => {
            warn!(%e, "nansen_extended: reqwest client");
            None
        }
    }
}

pub async fn nansen_netflows_loop(pool: PgPool) {
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    let body = default_netflows_request_body();
    loop {
        if !resolve_nansen_loop_default_on(&pool, "nansen_loop_netflows_enabled", "NANSEN_NETFLOWS_ENABLED").await {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        if !resolve_worker_enabled_flag(&pool, "worker", "nansen_enabled", "QTSS_NANSEN_ENABLED", true).await {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "nansen_netflows_tick_secs",
            "NANSEN_NETFLOWS_TICK_SECS",
            1800,
            900,
        )
        .await;
        let Some(key) = resolve_nansen_api_key(&pool).await else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let netflow_path = std::env::var("NANSEN_SMART_MONEY_NETFLOW_PATH")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "api/v1/smart-money/netflow".to_string());
        let started = Instant::now();
        let res = qtss_nansen::post_smart_money_netflow(
            &client,
            &base,
            key.trim(),
            &netflow_path,
            &body,
        )
        .await;
        let mut next = tick;
        if res
            .as_ref()
            .err()
            .map(|e| e.is_insufficient_credits())
            .unwrap_or(false)
        {
            next = insufficient_sleep_default(tick);
        }
        persist_nansen_result(&pool, NANSEN_NETFLOWS_DATA_KEY, &body, res, started).await;
        tokio::time::sleep(Duration::from_secs(next)).await;
    }
}

pub async fn nansen_holdings_loop(pool: PgPool) {
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    let body = default_holdings_request_body();
    loop {
        if !resolve_nansen_loop_default_on(&pool, "nansen_loop_holdings_enabled", "NANSEN_HOLDINGS_ENABLED").await {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        if !resolve_worker_enabled_flag(&pool, "worker", "nansen_enabled", "QTSS_NANSEN_ENABLED", true).await {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "nansen_holdings_tick_secs",
            "NANSEN_HOLDINGS_TICK_SECS",
            1800,
            900,
        )
        .await;
        let Some(key) = resolve_nansen_api_key(&pool).await else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let started = Instant::now();
        let res = qtss_nansen::post_smart_money_holdings(&client, &base, key.trim(), &body).await;
        let mut next = tick;
        if res
            .as_ref()
            .err()
            .map(|e| e.is_insufficient_credits())
            .unwrap_or(false)
        {
            next = insufficient_sleep_default(tick);
        }
        persist_nansen_result(&pool, NANSEN_HOLDINGS_DATA_KEY, &body, res, started).await;
        tokio::time::sleep(Duration::from_secs(next)).await;
    }
}

pub async fn nansen_perp_trades_loop(pool: PgPool) {
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    let body = default_pagination_body();
    loop {
        if !resolve_nansen_loop_default_on(
            &pool,
            "nansen_loop_smart_money_perp_trades_enabled",
            "NANSEN_PERP_TRADES_ENABLED",
        )
        .await
        {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        if !resolve_worker_enabled_flag(&pool, "worker", "nansen_enabled", "QTSS_NANSEN_ENABLED", true).await {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "nansen_perp_trades_tick_secs",
            "NANSEN_PERP_TRADES_TICK_SECS",
            1800,
            900,
        )
        .await;
        let Some(key) = resolve_nansen_api_key(&pool).await else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let started = Instant::now();
        let res =
            qtss_nansen::post_smart_money_perp_trades(&client, &base, key.trim(), &body).await;
        let mut next = tick;
        if res
            .as_ref()
            .err()
            .map(|e| e.is_insufficient_credits())
            .unwrap_or(false)
        {
            next = insufficient_sleep_default(tick);
        }
        persist_nansen_result(&pool, NANSEN_PERP_TRADES_DATA_KEY, &body, res, started).await;
        tokio::time::sleep(Duration::from_secs(next)).await;
    }
}

pub async fn nansen_who_bought_loop(pool: PgPool) {
    let tick: u64 = std::env::var("NANSEN_WHO_BOUGHT_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800)
        .max(900);
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    loop {
        if !resolve_nansen_loop_default_on(&pool, "nansen_loop_who_bought_sold_enabled", "NANSEN_WHO_BOUGHT_ENABLED")
            .await
        {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        if !resolve_worker_enabled_flag(&pool, "worker", "nansen_enabled", "QTSS_NANSEN_ENABLED", true).await {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        let Some(body) = resolve_who_bought_request_body() else {
            if !WHO_BOUGHT_MISSING_CONFIG_LOGGED.swap(true, Ordering::SeqCst) {
                debug!(
                    "NANSEN_WHO_BOUGHT_BODY_JSON veya NANSEN_WHO_BOUGHT_TOKEN_ADDRESS tanımsız — \
                     tgm/who-bought-sold çağrılmıyor (422 önlemi); qtss_worker=debug ile görünür"
                );
            }
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let Some(key) = resolve_nansen_api_key(&pool).await else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let started = Instant::now();
        let res = qtss_nansen::post_tgm_who_bought_sold(&client, &base, key.trim(), &body).await;
        let mut next = tick;
        if res
            .as_ref()
            .err()
            .map(|e| e.is_insufficient_credits())
            .unwrap_or(false)
        {
            next = insufficient_sleep_default(tick);
        }
        persist_nansen_result(&pool, NANSEN_WHO_BOUGHT_DATA_KEY, &body, res, started).await;
        tokio::time::sleep(Duration::from_secs(next)).await;
    }
}

pub async fn nansen_flow_intel_loop(pool: PgPool) {
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    loop {
        if !resolve_nansen_loop_default_on(&pool, "nansen_loop_flow_intelligence_enabled", "NANSEN_FLOW_INTEL_ENABLED")
            .await
        {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        if !resolve_worker_enabled_flag(&pool, "worker", "nansen_enabled", "QTSS_NANSEN_ENABLED", true).await {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "nansen_flow_intel_tick_secs",
            "NANSEN_FLOW_INTEL_TICK_SECS",
            900,
            600,
        )
        .await;
        let Some(api_key) = resolve_nansen_api_key(&pool).await else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let map_v = AppConfigRepository::get_value_json(&pool, "nansen_flow_intel_by_symbol")
            .await
            .ok()
            .flatten();
        let Some(map_v) = map_v else {
            tracing::debug!(
                "nansen_flow_intel: app_config nansen_flow_intel_by_symbol yok — atlanıyor"
            );
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
            if !qtss_storage::is_backfill_ready(&pool, es.id).await {
                continue;
            }
            let sym = es.symbol.trim().to_uppercase();
            let body = obj
                .get(&sym)
                .cloned()
                .or_else(|| obj.get(sym.as_str()).cloned());
            let Some(body) = body else {
                continue;
            };
            if !body.is_object() {
                continue;
            }
            any = true;
            let started = Instant::now();
            let res =
                qtss_nansen::post_tgm_flow_intelligence(&client, &base, api_key.trim(), &body)
                    .await;
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
        for key in [
            "trader_address",
            "address",
            "wallet",
            "wallet_address",
            "user_address",
        ] {
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
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    let body = perp_pnl_leaderboard_request_body();
    let leaderboard_path = std::env::var("NANSEN_PERP_LEADERBOARD_PATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "api/v1/tgm/perp-pnl-leaderboard".to_string());
    let repo = AppConfigRepository::new(pool.clone());
    loop {
        if !resolve_nansen_loop_default_on(
            &pool,
            "nansen_loop_perp_pnl_leaderboard_enabled",
            "NANSEN_PERP_LEADERBOARD_ENABLED",
        )
        .await
        {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        if !resolve_worker_enabled_flag(&pool, "worker", "nansen_enabled", "QTSS_NANSEN_ENABLED", true).await {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "nansen_perp_leaderboard_tick_secs",
            "NANSEN_PERP_LEADERBOARD_TICK_SECS",
            604_800,
            3_600,
        )
        .await;
        let Some(api_key) = resolve_nansen_api_key(&pool).await else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let started = Instant::now();
        let res = qtss_nansen::post_profiler_perp_leaderboard(
            &client,
            &base,
            api_key.trim(),
            &leaderboard_path,
            &body,
        )
        .await;
        if let Ok((ref v, _)) = res {
            let wallets = extract_wallet_addresses(v);
            let cfg = json!({
                "wallets": wallets,
                "last_updated": chrono::Utc::now().to_rfc3339(),
            });
            if let Err(e) = repo
                .upsert(
                    NANSEN_WHALE_WATCHLIST_KEY,
                    cfg,
                    Some("Whale watchlist from Nansen tgm/perp-pnl-leaderboard"),
                    None,
                )
                .await
            {
                warn!(%e, "nansen_whale_watchlist app_config upsert");
            }
        }
        persist_nansen_result(&pool, NANSEN_PERP_LEADERBOARD_DATA_KEY, &body, res, started).await;
        tokio::time::sleep(Duration::from_secs(tick)).await;
    }
}

pub async fn nansen_whale_perp_aggregate_loop(pool: PgPool) {
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    loop {
        if !resolve_nansen_loop_default_on(
            &pool,
            "nansen_loop_whale_perp_aggregate_enabled",
            "NANSEN_WHALE_PERP_AGGREGATE_ENABLED",
        )
        .await
        {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        if !resolve_worker_enabled_flag(&pool, "worker", "nansen_enabled", "QTSS_NANSEN_ENABLED", true).await {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "nansen_whale_perp_positions_tick_secs",
            "NANSEN_WHALE_PERP_POSITIONS_TICK_SECS",
            1800,
            600,
        )
        .await;
        let Some(api_key) = resolve_nansen_api_key(&pool).await else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let wl = AppConfigRepository::get_value_json(&pool, NANSEN_WHALE_WATCHLIST_KEY)
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
            match qtss_nansen::post_profiler_perp_positions(&client, &base, api_key.trim(), &body)
                .await
            {
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
            let m =
                json!({ "qtss_fetch_duration_ms": elapsed, "nansen_insufficient_credits": false });
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

macro_rules! nansen_opt_in_tgm_loop {
    (
        $fname:ident,
        $cfg:literal,
        $opt_in:literal,
        $env_json:literal,
        $app:literal,
        $tdb:literal,
        $tenv:literal,
        $def:expr,
        $min:expr,
        $sk:expr,
        $post:path
    ) => {
        pub async fn $fname(pool: PgPool) {
            let Some(client) = nansen_client().await else {
                return;
            };
            let base = nansen_api_base();
            loop {
                if !resolve_nansen_loop_opt_in(&pool, $cfg, $opt_in).await {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    continue;
                }
                if !resolve_worker_enabled_flag(
                    &pool,
                    "worker",
                    "nansen_enabled",
                    "QTSS_NANSEN_ENABLED",
                    true,
                )
                .await
                {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    continue;
                }
                let tick =
                    resolve_worker_tick_secs(&pool, "worker", $tdb, $tenv, $def, $min).await;
                let Some(api_key) = resolve_nansen_api_key(&pool).await else {
                    tokio::time::sleep(Duration::from_secs(tick)).await;
                    continue;
                };

                if let Ok(raw) = std::env::var($env_json) {
                    if let Ok(b) = serde_json::from_str::<Value>(raw.trim()) {
                        if b.is_object() {
                            let started = Instant::now();
                            let res = $post(&client, &base, api_key.trim(), &b).await;
                            let mut next = tick;
                            if res
                                .as_ref()
                                .err()
                                .map(|e| e.is_insufficient_credits())
                                .unwrap_or(false)
                            {
                                next = insufficient_sleep_default(tick);
                            }
                            persist_nansen_result(&pool, $sk, &b, res, started).await;
                            tokio::time::sleep(Duration::from_secs(next)).await;
                            continue;
                        }
                    }
                }

                let map_v = AppConfigRepository::get_value_json(&pool, $app)
                    .await
                    .ok()
                    .flatten();
                let Some(map_v) = map_v else {
                    debug!(
                        app_key = $app,
                        slice = stringify!($fname),
                        "nansen: app_config missing, skip tick"
                    );
                    tokio::time::sleep(Duration::from_secs(tick)).await;
                    continue;
                };
                let Some(obj) = map_v.as_object() else {
                    tokio::time::sleep(Duration::from_secs(tick)).await;
                    continue;
                };
                if obj.is_empty() {
                    tokio::time::sleep(Duration::from_secs(tick)).await;
                    continue;
                }
                let rows = list_enabled_engine_symbols(&pool).await.unwrap_or_default();
                let mut any = false;
                for es in rows {
                    if !qtss_storage::is_backfill_ready(&pool, es.id).await {
                        continue;
                    }
                    let sym = es.symbol.trim().to_uppercase();
                    let body = obj
                        .get(&sym)
                        .cloned()
                        .or_else(|| obj.get(sym.as_str()).cloned());
                    let Some(body) = body else {
                        continue;
                    };
                    if !body.is_object() {
                        continue;
                    }
                    any = true;
                    let started = Instant::now();
                    let res = $post(&client, &base, api_key.trim(), &body).await;
                    persist_nansen_result(&pool, $sk, &body, res, started).await;
                }
                if !any {
                    debug!(
                        app_key = $app,
                        slice = stringify!($fname),
                        "nansen: no matching engine_symbol"
                    );
                }
                tokio::time::sleep(Duration::from_secs(tick)).await;
            }
        }
    };
}

nansen_opt_in_tgm_loop!(
    nansen_tgm_flows_loop,
    "nansen_loop_tgm_flows_enabled",
    "NANSEN_TGM_FLOWS_ENABLED",
    "NANSEN_TGM_FLOWS_BODY_JSON",
    "nansen_tgm_flows_by_symbol",
    "nansen_tgm_flows_tick_secs",
    "NANSEN_TGM_FLOWS_TICK_SECS",
    3600_u64,
    600_u64,
    NANSEN_TGM_FLOWS_DATA_KEY,
    qtss_nansen::post_tgm_flows
);

nansen_opt_in_tgm_loop!(
    nansen_tgm_perp_trades_tgm_loop,
    "nansen_loop_tgm_perp_trades_enabled",
    "NANSEN_TGM_PERP_TRADES_ENABLED",
    "NANSEN_TGM_PERP_TRADES_BODY_JSON",
    "nansen_tgm_perp_trades_by_symbol",
    "nansen_tgm_perp_trades_tick_secs",
    "NANSEN_TGM_PERP_TRADES_TICK_SECS",
    3600_u64,
    600_u64,
    NANSEN_TGM_PERP_TRADES_DATA_KEY,
    qtss_nansen::post_tgm_perp_trades
);

nansen_opt_in_tgm_loop!(
    nansen_tgm_dex_trades_loop,
    "nansen_loop_tgm_dex_trades_enabled",
    "NANSEN_TGM_DEX_TRADES_ENABLED",
    "NANSEN_TGM_DEX_TRADES_BODY_JSON",
    "nansen_tgm_dex_trades_by_symbol",
    "nansen_tgm_dex_trades_tick_secs",
    "NANSEN_TGM_DEX_TRADES_TICK_SECS",
    3600_u64,
    600_u64,
    NANSEN_TGM_DEX_TRADES_DATA_KEY,
    qtss_nansen::post_tgm_dex_trades
);

nansen_opt_in_tgm_loop!(
    nansen_tgm_token_information_loop,
    "nansen_loop_tgm_token_information_enabled",
    "NANSEN_TGM_TOKEN_INFORMATION_ENABLED",
    "NANSEN_TGM_TOKEN_INFORMATION_BODY_JSON",
    "nansen_tgm_token_information_by_symbol",
    "nansen_tgm_token_information_tick_secs",
    "NANSEN_TGM_TOKEN_INFORMATION_TICK_SECS",
    7200_u64,
    900_u64,
    NANSEN_TGM_TOKEN_INFORMATION_DATA_KEY,
    qtss_nansen::post_tgm_token_information
);

nansen_opt_in_tgm_loop!(
    nansen_tgm_indicators_loop,
    "nansen_loop_tgm_indicators_enabled",
    "NANSEN_TGM_INDICATORS_ENABLED",
    "NANSEN_TGM_INDICATORS_BODY_JSON",
    "nansen_tgm_indicators_by_symbol",
    "nansen_tgm_indicators_tick_secs",
    "NANSEN_TGM_INDICATORS_TICK_SECS",
    7200_u64,
    900_u64,
    NANSEN_TGM_INDICATORS_DATA_KEY,
    qtss_nansen::post_tgm_indicators
);

nansen_opt_in_tgm_loop!(
    nansen_tgm_perp_positions_loop,
    "nansen_loop_tgm_perp_positions_enabled",
    "NANSEN_TGM_PERP_POSITIONS_ENABLED",
    "NANSEN_TGM_PERP_POSITIONS_BODY_JSON",
    "nansen_tgm_perp_positions_by_symbol",
    "nansen_tgm_perp_positions_tick_secs",
    "NANSEN_TGM_PERP_POSITIONS_TICK_SECS",
    3600_u64,
    600_u64,
    NANSEN_TGM_PERP_POSITIONS_DATA_KEY,
    qtss_nansen::post_tgm_perp_positions
);

nansen_opt_in_tgm_loop!(
    nansen_tgm_holders_loop,
    "nansen_loop_tgm_holders_enabled",
    "NANSEN_TGM_HOLDERS_ENABLED",
    "NANSEN_TGM_HOLDERS_BODY_JSON",
    "nansen_tgm_holders_by_symbol",
    "nansen_tgm_holders_tick_secs",
    "NANSEN_TGM_HOLDERS_TICK_SECS",
    3600_u64,
    600_u64,
    NANSEN_TGM_HOLDERS_DATA_KEY,
    qtss_nansen::post_tgm_holders
);

fn perp_screener_request_body() -> Value {
    if let Ok(raw) = std::env::var("NANSEN_PERP_SCREENER_BODY_JSON") {
        if let Ok(v) = serde_json::from_str::<Value>(raw.trim()) {
            if v.is_object() {
                return v;
            }
        }
    }
    let to = chrono::Utc::now().date_naive();
    let from = to
        .checked_sub_signed(chrono::Duration::days(6))
        .unwrap_or(to);
    json!({
        "date": {
            "from": from.format("%Y-%m-%d").to_string(),
            "to": to.format("%Y-%m-%d").to_string(),
        },
        "pagination": { "page": 1, "per_page": 50 },
        "filters": { "only_smart_money": true },
    })
}

pub async fn nansen_perp_screener_loop(pool: PgPool) {
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    loop {
        if !resolve_nansen_loop_opt_in(&pool, "nansen_loop_perp_screener_enabled", "NANSEN_PERP_SCREENER_ENABLED")
            .await
        {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        if !resolve_worker_enabled_flag(&pool, "worker", "nansen_enabled", "QTSS_NANSEN_ENABLED", true)
            .await
        {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "nansen_perp_screener_tick_secs",
            "NANSEN_PERP_SCREENER_TICK_SECS",
            3600,
            600,
        )
        .await;
        let Some(api_key) = resolve_nansen_api_key(&pool).await else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let body = perp_screener_request_body();
        let started = Instant::now();
        let res = qtss_nansen::post_perp_screener(&client, &base, api_key.trim(), &body).await;
        let mut next = tick;
        if res
            .as_ref()
            .err()
            .map(|e| e.is_insufficient_credits())
            .unwrap_or(false)
        {
            next = insufficient_sleep_default(tick);
        }
        persist_nansen_result(
            &pool,
            NANSEN_PERP_SCREENER_DATA_KEY,
            &body,
            res,
            started,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(next)).await;
    }
}

fn smart_money_dex_trades_request_body() -> Value {
    if let Ok(raw) = std::env::var("NANSEN_SM_DEX_TRADES_BODY_JSON") {
        if let Ok(v) = serde_json::from_str::<Value>(raw.trim()) {
            if v.is_object() {
                return v;
            }
        }
    }
    let mut body = default_pagination_body();
    json_merge(&mut body, json!({ "chains": default_chains_for_smart_money() }));
    body
}

pub async fn nansen_smart_money_dex_trades_loop(pool: PgPool) {
    let Some(client) = nansen_client().await else {
        return;
    };
    let base = nansen_api_base();
    let body = smart_money_dex_trades_request_body();
    loop {
        if !resolve_nansen_loop_opt_in(&pool, "nansen_loop_smart_money_dex_trades_enabled", "NANSEN_SM_DEX_TRADES_ENABLED")
            .await
        {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        if !resolve_worker_enabled_flag(&pool, "worker", "nansen_enabled", "QTSS_NANSEN_ENABLED", true)
            .await
        {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "nansen_sm_dex_trades_tick_secs",
            "NANSEN_SM_DEX_TRADES_TICK_SECS",
            3600,
            900,
        )
        .await;
        let Some(api_key) = resolve_nansen_api_key(&pool).await else {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        };
        let started = Instant::now();
        let res =
            qtss_nansen::post_smart_money_dex_trades(&client, &base, api_key.trim(), &body).await;
        let mut next = tick;
        if res
            .as_ref()
            .err()
            .map(|e| e.is_insufficient_credits())
            .unwrap_or(false)
        {
            next = insufficient_sleep_default(tick);
        }
        persist_nansen_result(
            &pool,
            NANSEN_SMART_MONEY_DEX_TRADES_DATA_KEY,
            &body,
            res,
            started,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(next)).await;
    }
}

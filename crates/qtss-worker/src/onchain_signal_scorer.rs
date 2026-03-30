//! SPEC_ONCHAIN_SIGNALS §8 — `onchain_signal_scores` (`data_snapshots` + Nansen).
//!
//! Confluence ayrıca `analysis_snapshots` içinde üç sütunlu skor üretir; bu modül SPEC’teki tablo ve
//! `/analysis/onchain-signals/*` uçları için alt skor kolonları + tarihçe yazar.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use qtss_notify::{Notification, NotificationChannel, NotificationDispatcher};
use qtss_storage::{
    data_snapshot_age_secs, delete_onchain_signal_scores_older_than,
    fetch_analysis_snapshot_payload, fetch_data_snapshot, fetch_latest_onchain_signal_score,
    insert_onchain_signal_score, list_enabled_engine_symbols, list_engine_symbols_matching,
    resolve_worker_tick_secs, AppConfigRepository, OnchainSignalScoreInsert, OnchainSignalScoreRow,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{info, warn};

use crate::data_sources::registry::{
    NANSEN_FLOW_INTELLIGENCE_DATA_KEY, NANSEN_NETFLOWS_DATA_KEY, NANSEN_PERP_TRADES_DATA_KEY,
    NANSEN_TOKEN_SCREENER_DATA_KEY, NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY,
    NANSEN_WHO_BOUGHT_SOLD_DATA_KEY,
};
use crate::signal_scorer::{
    score_binance_global_long_short_account_ratio, score_binance_open_interest_heat,
    score_binance_premium_funding, score_binance_taker_ratio, score_coinglass_liquidations_like,
    score_coinglass_netflow_like, score_hl_meta_asset_ctxs_for_coin, score_nansen_buyer_quality,
    score_nansen_dex_buy_sell_pressure, score_nansen_flow_intelligence, score_nansen_netflows,
    score_nansen_perp_direction,
};

fn engine_enabled() -> bool {
    match std::env::var("QTSS_ONCHAIN_SIGNAL_ENGINE")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("0") | Some("false") | Some("no") | Some("off") => false,
        _ => true,
    }
}

fn notify_on_signal_edge_enabled() -> bool {
    let onchain = std::env::var("QTSS_NOTIFY_ON_ONCHAIN_SIGNAL")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"));
    let generic = std::env::var("QTSS_NOTIFY_SIGNAL_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"));
    onchain || generic
}

fn notify_threshold() -> f64 {
    std::env::var("QTSS_ONCHAIN_NOTIFY_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.6)
}

fn retention_prune_interval_secs() -> u64 {
    std::env::var("QTSS_ONCHAIN_SCORE_RETENTION_PRUNE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(86_400)
        .max(3_600)
}

fn retention_days() -> i32 {
    std::env::var("QTSS_ONCHAIN_SCORE_RETENTION_DAYS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7)
        .max(1)
}

fn onchain_weights_config_key() -> String {
    std::env::var("QTSS_ONCHAIN_SIGNAL_WEIGHTS_KEY")
        .unwrap_or_else(|_| "onchain_signal_weights".into())
        .trim()
        .to_string()
}

#[derive(Debug, Clone, Copy)]
struct ComponentWeights {
    taker: f64,
    funding: f64,
    oi: f64,
    ls_ratio: f64,
    coinglass_netflow: f64,
    coinglass_liquidations: f64,
    hl_meta: f64,
    nansen: f64,
    nansen_netflows: f64,
    nansen_perp: f64,
    nansen_buyer_quality: f64,
    nansen_flow_intelligence: f64,
    hl_whale: f64,
}

async fn load_component_weights(pool: &PgPool) -> ComponentWeights {
    let key = onchain_weights_config_key();
    let raw = match AppConfigRepository::get_value_json(pool, &key).await {
        Ok(Some(v)) if v.is_object() => v,
        Ok(Some(_)) | Ok(None) => json!({}),
        Err(e) => {
            warn!(%e, %key, "onchain_signal_weights okunamadı — varsayılan 1.0");
            json!({})
        }
    };
    let o = raw.as_object().cloned().unwrap_or_default();
    let g = |k: &str| {
        o.get(k)
            .and_then(|x| x.as_f64())
            .or_else(|| o.get(k).and_then(|x| x.as_i64().map(|i| i as f64)))
            .unwrap_or(1.0)
            .clamp(0.0, 10.0)
    };
    ComponentWeights {
        taker: g("taker"),
        funding: g("funding"),
        oi: g("oi"),
        ls_ratio: g("ls_ratio"),
        coinglass_netflow: g("coinglass_netflow"),
        coinglass_liquidations: g("coinglass_liquidations"),
        hl_meta: g("hl_meta"),
        nansen: g("nansen"),
        nansen_netflows: g("nansen_netflows"),
        nansen_perp: g("nansen_perp"),
        nansen_buyer_quality: g("nansen_buyer_quality"),
        nansen_flow_intelligence: g("nansen_flow_intelligence"),
        hl_whale: g("hl_whale"),
    }
}

fn notify_channels() -> Vec<NotificationChannel> {
    let raw = std::env::var("QTSS_NOTIFY_ON_ONCHAIN_SIGNAL_CHANNELS")
        .unwrap_or_else(|_| "webhook".into());
    raw.split(',')
        .filter_map(|s| NotificationChannel::parse(s.trim()))
        .collect()
}

fn symbol_base_lower(symbol: &str) -> String {
    let sym = symbol.trim().to_uppercase();
    sym.strip_suffix("USDT")
        .unwrap_or(sym.as_str())
        .to_lowercase()
}

fn hl_perp_coin(symbol: &str) -> String {
    let sym = symbol.trim().to_uppercase();
    sym.strip_suffix("USDT")
        .unwrap_or(sym.as_str())
        .to_uppercase()
}

fn snapshot_age_confidence(age_secs: Option<i64>, tick_secs: i32) -> f64 {
    let t = tick_secs.max(1) as f64;
    match age_secs {
        None => 0.1,
        Some(age) if (age as f64) < 2.0 * t => 1.0,
        Some(age) if (age as f64) < 5.0 * t => 0.5,
        _ => 0.1,
    }
}

async fn snapshot_score_with_conf(
    pool: &PgPool,
    key: &str,
    tick_secs: i32,
    score: impl FnOnce(&Value) -> f64,
) -> (Option<f64>, f64, bool) {
    let Ok(Some(row)) = fetch_data_snapshot(pool, key).await else {
        return (None, 0.0, false);
    };
    if row.error.is_some() {
        return (None, 0.0, false);
    }
    let Some(ref j) = row.response_json else {
        return (None, 0.0, false);
    };
    let age = data_snapshot_age_secs(pool, key).await.ok().flatten();
    let c = snapshot_age_confidence(age, tick_secs);
    (Some(score(j)), c, true)
}

fn ta_long_short_bias(dash: &Value) -> f64 {
    if let Some(v2) = dash.get("signal_dashboard_v2") {
        if v2.get("schema_version").and_then(|x| x.as_u64()) == Some(3) {
            if let Some(s) = v2.get("status").and_then(|x| x.as_str()) {
                match s.trim().to_uppercase().replace('İ', "I").as_str() {
                    "LONG" => return 1.0,
                    "SHORT" => return -1.0,
                    _ => {}
                }
            }
        }
    }
    if let Some(d) = dash.get("durum").and_then(|x| x.as_str()) {
        match d.trim().to_uppercase().replace('İ', "I").as_str() {
            "LONG" => return 1.0,
            "SHORT" => return -1.0,
            _ => {}
        }
    }
    0.0
}

fn market_regime_raw(dash: &Value) -> Option<String> {
    if let Some(v2) = dash.get("signal_dashboard_v2") {
        if v2.get("schema_version").and_then(|x| x.as_u64()) == Some(3) {
            if let Some(m) = v2.get("market_mode").and_then(|x| x.as_str()) {
                let s = m.trim();
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    dash.get("piyasa_modu")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn direction_from_aggregate(agg: f64) -> &'static str {
    if agg >= 0.6 {
        "strong_buy"
    } else if agg >= 0.2 {
        "buy"
    } else if agg > -0.2 {
        "neutral"
    } else if agg > -0.6 {
        "sell"
    } else {
        "strong_sell"
    }
}

/// SPEC §4.2 — `Σ(score × conf × weight) / Σ(conf × weight)`.
fn weighted_aggregate_spec(parts: &[(f64, f64, f64)]) -> (f64, f64) {
    let mut num = 0.0;
    let mut den = 0.0;
    let mut conf_sum = 0.0f64;
    let mut n = 0usize;
    for (s, c, w) in parts {
        if *w <= 0.0 || *c <= 0.05 || !s.is_finite() {
            continue;
        }
        num += s * c * w;
        den += c * w;
        conf_sum += *c;
        n += 1;
    }
    if den <= 1e-12 || n == 0 {
        return (0.0, 0.0);
    }
    let conf = (conf_sum / n as f64).clamp(0.0, 1.0);
    (num / den, conf)
}

async fn load_signal_dashboard(pool: &PgPool, symbol_upper: &str) -> Option<Value> {
    let rows = list_engine_symbols_matching(pool, symbol_upper, None, None, None)
        .await
        .ok()?;
    let id = rows.first()?.id;
    fetch_analysis_snapshot_payload(pool, id, "signal_dashboard")
        .await
        .ok()?
}

async fn maybe_notify_edge(
    symbol: &str,
    aggregate: f64,
    conflict: bool,
    direction: &str,
    regime: Option<&str>,
    confidence: f64,
    prev: Option<&OnchainSignalScoreRow>,
) {
    if !notify_on_signal_edge_enabled() || conflict {
        return;
    }
    let th = notify_threshold();
    if aggregate.abs() <= th {
        return;
    }
    let crossed = prev
        .map(|p| p.aggregate_score.abs() <= th && aggregate.abs() > th)
        .unwrap_or(true);
    if !crossed {
        return;
    }
    let chans = notify_channels();
    if chans.is_empty() {
        return;
    }
    let title = format!("{symbol} — {direction}");
    let body = format!(
        "On-chain aggregate: {aggregate:.3}, regime: {:?}, confidence: {:.0}%",
        regime,
        confidence * 100.0
    );
    let n = Notification::new(title, body);
    let d = NotificationDispatcher::from_env();
    for r in d.send_all(&chans, &n).await {
        if r.ok {
            info!(channel = ?r.channel, %symbol, "onchain signal notify");
        } else {
            warn!(channel = ?r.channel, detail = ?r.detail, %symbol, "onchain signal notify başarısız");
        }
    }
}

/// Tek sembol için skor hesapla ve `onchain_signal_scores` satırı ekle.
pub async fn compute_and_persist_for_symbol(pool: &PgPool, symbol: &str) -> Result<(), String> {
    let sym = symbol.trim().to_uppercase();
    if sym.is_empty() {
        return Ok(());
    }
    let base = symbol_base_lower(&sym);
    let coin = hl_perp_coin(&sym);

    let taker_key = format!("binance_taker_{base}usdt");
    let prem_key = format!("binance_premium_{base}usdt");
    let oi_key = format!("binance_open_interest_{base}usdt");
    let ls_key = format!("binance_ls_ratio_{base}usdt");

    let (taker_v, taker_c, taker_ok) = snapshot_score_with_conf(&pool, &taker_key, 60, |j| {
        score_binance_taker_ratio(j).unwrap_or(0.0)
    })
    .await;
    let (fund_v, fund_c, fund_ok) =
        snapshot_score_with_conf(&pool, &prem_key, 60, score_binance_premium_funding).await;
    let (oi_v, oi_c, oi_ok) =
        snapshot_score_with_conf(&pool, &oi_key, 60, score_binance_open_interest_heat).await;
    let (ls_v, ls_c, ls_ok) = snapshot_score_with_conf(
        &pool,
        &ls_key,
        300,
        score_binance_global_long_short_account_ratio,
    )
    .await;

    let (nf_v, nf_c, nf_ok) = if base == "btc" {
        snapshot_score_with_conf(
            &pool,
            "coinglass_netflow_btc",
            300,
            score_coinglass_netflow_like,
        )
        .await
    } else {
        (None, 0.0, false)
    };
    let (liq_v, liq_c, liq_ok) = if base == "btc" {
        snapshot_score_with_conf(
            &pool,
            "coinglass_liquidations_btc",
            300,
            score_coinglass_liquidations_like,
        )
        .await
    } else {
        (None, 0.0, false)
    };

    let (hl_v, hl_c, hl_ok) = snapshot_score_with_conf(&pool, "hl_meta_asset_ctxs", 60, |j| {
        score_hl_meta_asset_ctxs_for_coin(j, &coin)
    })
    .await;

    // Token screener: DEX buy/sell pressure only (no row-count “depth” blend — dev guide §3.1).
    let (nansen_dex_v, nansen_dex_c, nansen_dex_ok) = snapshot_score_with_conf(
        &pool,
        NANSEN_TOKEN_SCREENER_DATA_KEY,
        1800,
        score_nansen_dex_buy_sell_pressure,
    )
    .await;

    let (nansen_nf_v, nansen_nf_c, nansen_nf_ok) =
        snapshot_score_with_conf(&pool, NANSEN_NETFLOWS_DATA_KEY, 1800, score_nansen_netflows)
            .await;
    let (nansen_pt_v, nansen_pt_c, nansen_pt_ok) = snapshot_score_with_conf(
        &pool,
        NANSEN_PERP_TRADES_DATA_KEY,
        1800,
        score_nansen_perp_direction,
    )
    .await;
    let (nansen_fi_v, nansen_fi_c, nansen_fi_ok) = snapshot_score_with_conf(
        &pool,
        NANSEN_FLOW_INTELLIGENCE_DATA_KEY,
        1800,
        score_nansen_flow_intelligence,
    )
    .await;
    let (nansen_bq_v, nansen_bq_c, nansen_bq_ok) = snapshot_score_with_conf(
        &pool,
        NANSEN_WHO_BOUGHT_SOLD_DATA_KEY,
        1800,
        score_nansen_buyer_quality,
    )
    .await;

    let (hl_whale_v, hl_whale_c, hl_whale_ok) = snapshot_score_with_conf(
        &pool,
        NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY,
        1800,
        score_nansen_perp_direction,
    )
    .await;

    let w = load_component_weights(pool).await;
    // §3.2: same exchange-flow idea as Coinglass netflow — halve Coinglass weight when both contribute.
    let coinglass_nf_effective_w =
        if nf_v.is_some() && nf_ok && nansen_fi_v.is_some() && nansen_fi_ok {
            w.coinglass_netflow * 0.5
        } else {
            w.coinglass_netflow
        };

    let mut parts: Vec<(f64, f64, f64)> = Vec::new();
    let mut breakdown: Vec<Value> = Vec::new();
    if let Some(s) = taker_v {
        parts.push((s, taker_c, w.taker));
        if taker_ok {
            breakdown.push(json!({
                "component": "taker",
                "source_key": taker_key,
                "score": s,
                "confidence": taker_c,
                "weight": w.taker
            }));
        }
    }
    if let Some(s) = fund_v {
        parts.push((s, fund_c, w.funding));
        if fund_ok {
            breakdown.push(json!({
                "component": "funding",
                "source_key": prem_key,
                "score": s,
                "confidence": fund_c,
                "weight": w.funding
            }));
        }
    }
    if let Some(s) = oi_v {
        parts.push((s, oi_c, w.oi));
        if oi_ok {
            breakdown.push(json!({
                "component": "oi",
                "source_key": oi_key,
                "score": s,
                "confidence": oi_c,
                "weight": w.oi
            }));
        }
    }
    if let Some(s) = ls_v {
        parts.push((s, ls_c, w.ls_ratio));
        if ls_ok {
            breakdown.push(json!({
                "component": "ls_ratio",
                "source_key": ls_key,
                "score": s,
                "confidence": ls_c,
                "weight": w.ls_ratio
            }));
        }
    }
    if let Some(s) = nf_v {
        parts.push((s, nf_c, coinglass_nf_effective_w));
        if nf_ok {
            breakdown.push(json!({
                "component": "coinglass_netflow",
                "source_key": "coinglass_netflow_btc",
                "score": s,
                "confidence": nf_c,
                "weight": coinglass_nf_effective_w,
                "weight_base": w.coinglass_netflow,
                "nansen_flow_intelligence_active": nansen_fi_v.is_some() && nansen_fi_ok
            }));
        }
    }
    if let Some(s) = liq_v {
        parts.push((s, liq_c, w.coinglass_liquidations));
        if liq_ok {
            breakdown.push(json!({
                "component": "coinglass_liquidations",
                "source_key": "coinglass_liquidations_btc",
                "score": s,
                "confidence": liq_c,
                "weight": w.coinglass_liquidations
            }));
        }
    }
    if let Some(s) = hl_v {
        parts.push((s, hl_c, w.hl_meta));
        if hl_ok {
            breakdown.push(json!({
                "component": "hl_meta",
                "source_key": "hl_meta_asset_ctxs",
                "score": s,
                "confidence": hl_c,
                "weight": w.hl_meta
            }));
        }
    }
    if let Some(s) = nansen_dex_v {
        parts.push((s, nansen_dex_c, w.nansen));
        if nansen_dex_ok {
            breakdown.push(json!({
                "component": "nansen_dex_pressure",
                "source_key": NANSEN_TOKEN_SCREENER_DATA_KEY,
                "score": s,
                "confidence": nansen_dex_c,
                "weight": w.nansen
            }));
        }
    }
    if let Some(s) = nansen_nf_v {
        parts.push((s, nansen_nf_c, w.nansen_netflows));
        if nansen_nf_ok {
            breakdown.push(json!({
                "component": "nansen_netflows",
                "source_key": NANSEN_NETFLOWS_DATA_KEY,
                "score": s,
                "confidence": nansen_nf_c,
                "weight": w.nansen_netflows
            }));
        }
    }
    if let Some(s) = nansen_pt_v {
        parts.push((s, nansen_pt_c, w.nansen_perp));
        if nansen_pt_ok {
            breakdown.push(json!({
                "component": "nansen_perp",
                "source_key": NANSEN_PERP_TRADES_DATA_KEY,
                "score": s,
                "confidence": nansen_pt_c,
                "weight": w.nansen_perp
            }));
        }
    }
    if let Some(s) = nansen_fi_v {
        parts.push((s, nansen_fi_c, w.nansen_flow_intelligence));
        if nansen_fi_ok {
            breakdown.push(json!({
                "component": "nansen_flow_intelligence",
                "source_key": NANSEN_FLOW_INTELLIGENCE_DATA_KEY,
                "score": s,
                "confidence": nansen_fi_c,
                "weight": w.nansen_flow_intelligence
            }));
        }
    }
    if let Some(s) = nansen_bq_v {
        parts.push((s, nansen_bq_c, w.nansen_buyer_quality));
        if nansen_bq_ok {
            breakdown.push(json!({
                "component": "nansen_buyer_quality",
                "source_key": NANSEN_WHO_BOUGHT_SOLD_DATA_KEY,
                "score": s,
                "confidence": nansen_bq_c,
                "weight": w.nansen_buyer_quality
            }));
        }
    }
    if let Some(s) = hl_whale_v {
        parts.push((s, hl_whale_c, w.hl_whale));
        if hl_whale_ok {
            breakdown.push(json!({
                "component": "hl_whale",
                "source_key": NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY,
                "score": s,
                "confidence": hl_whale_c,
                "weight": w.hl_whale
            }));
        }
    }

    let (aggregate_score, confidence) = weighted_aggregate_spec(&parts);
    let direction = direction_from_aggregate(aggregate_score).to_string();

    let dash = load_signal_dashboard(pool, &sym).await;
    let (conflict_detected, conflict_detail, market_regime) = match dash.as_ref() {
        Some(d) => {
            let bias = ta_long_short_bias(d);
            let regime = market_regime_raw(d);
            let (cd, det) = if bias > 0.25 && aggregate_score < -0.3 {
                (
                    true,
                    Some("TA LONG vs on-chain aggregate bearish (< -0.3)".into()),
                )
            } else if bias < -0.25 && aggregate_score > 0.3 {
                (
                    true,
                    Some("TA SHORT vs on-chain aggregate bullish (> 0.3)".into()),
                )
            } else {
                (false, None)
            };
            (cd, det, regime)
        }
        None => (false, None, None),
    };

    let mut snapshot_keys: Vec<String> = Vec::new();
    if taker_ok {
        snapshot_keys.push(taker_key.clone());
    }
    if fund_ok {
        snapshot_keys.push(prem_key.clone());
    }
    if oi_ok {
        snapshot_keys.push(oi_key.clone());
    }
    if ls_ok {
        snapshot_keys.push(ls_key.clone());
    }
    if nf_ok {
        snapshot_keys.push("coinglass_netflow_btc".into());
    }
    if liq_ok {
        snapshot_keys.push("coinglass_liquidations_btc".into());
    }
    if hl_ok {
        snapshot_keys.push("hl_meta_asset_ctxs".into());
    }
    if nansen_dex_ok {
        snapshot_keys.push(NANSEN_TOKEN_SCREENER_DATA_KEY.to_string());
    }
    if nansen_nf_ok {
        snapshot_keys.push(NANSEN_NETFLOWS_DATA_KEY.to_string());
    }
    if nansen_pt_ok {
        snapshot_keys.push(NANSEN_PERP_TRADES_DATA_KEY.to_string());
    }
    if nansen_fi_ok {
        snapshot_keys.push(NANSEN_FLOW_INTELLIGENCE_DATA_KEY.to_string());
    }
    if nansen_bq_ok {
        snapshot_keys.push(NANSEN_WHO_BOUGHT_SOLD_DATA_KEY.to_string());
    }
    if hl_whale_ok {
        snapshot_keys.push(NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY.to_string());
    }

    let meta_json = json!({
        "schema_version": 3,
        "weights_config_key": onchain_weights_config_key(),
        "weights_used": {
            "taker": w.taker,
            "funding": w.funding,
            "oi": w.oi,
            "ls_ratio": w.ls_ratio,
            "coinglass_netflow": w.coinglass_netflow,
            "coinglass_netflow_effective": coinglass_nf_effective_w,
            "coinglass_liquidations": w.coinglass_liquidations,
            "hl_meta": w.hl_meta,
            "nansen": w.nansen,
            "nansen_netflows": w.nansen_netflows,
            "nansen_perp": w.nansen_perp,
            "nansen_buyer_quality": w.nansen_buyer_quality,
            "nansen_flow_intelligence": w.nansen_flow_intelligence,
            "hl_whale": w.hl_whale
        },
        "source_breakdown": breakdown,
        "per_key_confidence": {
            "taker": taker_c,
            "funding": fund_c,
            "oi": oi_c,
            "ls_ratio": ls_c,
            "coinglass_netflow_btc": nf_c,
            "coinglass_liquidations_btc": liq_c,
            "hl_meta": hl_c,
            "nansen_token_screener_dex": nansen_dex_c,
            "nansen_netflows": nansen_nf_c,
            "nansen_perp_trades": nansen_pt_c,
            "nansen_flow_intelligence": nansen_fi_c,
            "nansen_who_bought_sold": nansen_bq_c,
            "nansen_whale_perp_aggregate": hl_whale_c
        },
        "aggregate_formula": "SPEC §4.2: sum(score*conf*weight)/sum(conf*weight)",
        "nansen_note": "nansen_sm_score column stores DEX pressure only (legacy name); extended Nansen in dedicated columns",
        "note": "exchange_balance / tvl: kapalı veya seri yok — null kolonlar; hl_whale Nansen profiler aggregate ile dolar"
    });

    let prev = fetch_latest_onchain_signal_score(pool, &sym)
        .await
        .map_err(|e| e.to_string())?;

    let insert = OnchainSignalScoreInsert {
        symbol: sym.clone(),
        funding_score: fund_v,
        oi_score: oi_v,
        ls_ratio_score: ls_v,
        taker_vol_score: taker_v,
        exchange_netflow_score: nf_v,
        exchange_balance_score: None,
        hl_bias_score: hl_v,
        hl_whale_score: hl_whale_v,
        liquidation_score: liq_v,
        nansen_sm_score: nansen_dex_v,
        nansen_netflow_score: nansen_nf_v,
        nansen_perp_score: nansen_pt_v,
        nansen_buyer_quality_score: nansen_bq_v,
        tvl_trend_score: None,
        aggregate_score,
        confidence,
        direction: direction.clone(),
        market_regime: market_regime.clone(),
        conflict_detected,
        conflict_detail: conflict_detail.clone(),
        snapshot_keys,
        meta_json: Some(meta_json),
    };

    insert_onchain_signal_score(pool, &insert)
        .await
        .map_err(|e| e.to_string())?;

    maybe_notify_edge(
        &sym,
        aggregate_score,
        conflict_detected,
        &direction,
        market_regime.as_deref(),
        confidence,
        prev.as_ref(),
    )
    .await;

    Ok(())
}

pub async fn onchain_signal_loop(pool: PgPool) {
    if !engine_enabled() {
        info!("QTSS_ONCHAIN_SIGNAL_ENGINE kapalı — onchain_signal_scores yazılmıyor");
        return;
    }
    let prune_every = Duration::from_secs(retention_prune_interval_secs());
    let days = retention_days();
    let mut last_prune = Instant::now();
    info!(
        prune_secs = prune_every.as_secs(),
        retention_days = days,
        "onchain_signal_scorer döngüsü (poll: system_config worker.onchain_signal_tick_secs / QTSS_ONCHAIN_SIGNAL_TICK_SECS)"
    );

    loop {
        let tick_secs = resolve_worker_tick_secs(
            &pool,
            "worker",
            "onchain_signal_tick_secs",
            "QTSS_ONCHAIN_SIGNAL_TICK_SECS",
            60,
            30,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
        if last_prune.elapsed() >= prune_every {
            match delete_onchain_signal_scores_older_than(&pool, days).await {
                Ok(n) if n > 0 => {
                    info!(rows = n, "onchain_signal_scores retention prune (SPEC §10)")
                }
                Ok(_) => {}
                Err(e) => warn!(%e, "onchain_signal_scores prune"),
            }
            last_prune = Instant::now();
        }
        let rows = match list_enabled_engine_symbols(&pool).await {
            Ok(r) => r,
            Err(e) => {
                warn!(%e, "list_enabled_engine_symbols");
                continue;
            }
        };
        let mut symbols: BTreeSet<String> = BTreeSet::new();
        for r in rows {
            symbols.insert(r.symbol.to_uppercase());
        }
        if symbols.is_empty() {
            tracing::debug!("onchain_signal_scorer: etkin engine_symbol yok");
            continue;
        }
        for sym in symbols {
            if let Err(e) = compute_and_persist_for_symbol(&pool, &sym).await {
                warn!(%e, %sym, "onchain_signal_scorer tick");
            } else {
                tracing::debug!(%sym, "onchain_signal_scores güncellendi");
            }
        }
    }
}

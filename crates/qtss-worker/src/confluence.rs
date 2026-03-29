//! Regime-weighted confluence → `analysis_snapshots` with `engine_kind = "confluence"`.
//! Reads `data_snapshots` + `app_config.confluence_weights_by_regime` (English keys).

use chrono::{DateTime, Utc};
use qtss_storage::{
    fetch_data_snapshot, insert_market_confluence_snapshot, upsert_analysis_snapshot,
    AppConfigRepository, EngineSymbolRow, MarketConfluenceSnapshotInsert,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{info, warn};

use crate::data_sources::registry::NANSEN_TOKEN_SCREENER_DATA_KEY;
use crate::signal_scorer::{
    score_binance_open_interest_heat, score_binance_premium_funding,
    score_coinglass_liquidations_like, score_coinglass_netflow_like, score_for_source_key,
    score_hl_meta_asset_ctxs_for_coin, score_nansen_dex_buy_sell_pressure,
    score_nansen_smart_money_depth,
};

const CONF_CONFIG_KEY: &str = "confluence_weights_by_regime";

fn confluence_engine_enabled() -> bool {
    match std::env::var("QTSS_CONFLUENCE_ENGINE")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("0") | Some("false") | Some("no") | Some("off") => false,
        _ => true,
    }
}

fn default_weights_by_regime() -> Value {
    json!({
        "range": {"technical": 0.50, "onchain": 0.35, "smart_money": 0.15},
        "trend": {"technical": 0.30, "onchain": 0.40, "smart_money": 0.30},
        "breakout": {"technical": 0.40, "onchain": 0.45, "smart_money": 0.15},
        "uncertain": {"technical": 0.20, "onchain": 0.30, "smart_money": 0.50}
    })
}

/// Advisory position-size multiplier for execution policy (PLAN §4.2); does not place orders.
fn lot_scale_hint_from_conflict_count(n: usize) -> f64 {
    let raw = 1.0 - 0.12 * (n as f64);
    raw.clamp(0.5, 1.0)
}

/// SPEC §4.3 — bileşik skordan yön etiketi (`signal_dashboard_v2` / API).
fn direction_from_composite_score(agg: f64) -> &'static str {
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

/// PLAN §4.1 — `SignalDashboardV2Envelope.market_mode` is English (`range`, `breakout`, `trend`, `uncertain`); v1 stays Turkish (`RANGE`, `KOPUS`, …).
fn map_market_mode_to_regime(raw: &str) -> &'static str {
    let t = raw.trim();
    match t.to_ascii_lowercase().as_str() {
        "range" => "range",
        "breakout" => "breakout",
        "trend" => "trend",
        "uncertain" => "uncertain",
        _ => match t.to_uppercase().replace('İ', "I").as_str() {
            "RANGE" => "range",
            "KOPUS" => "breakout",
            "TREND" => "trend",
            "BELIRSIZ" => "uncertain",
            _ => "uncertain",
        },
    }
}

/// Prefer nested `signal_dashboard_v2.market_mode` when `schema_version` is 3; else v1 `piyasa_modu`.
fn effective_market_mode_label(dash: &Value) -> String {
    if let Some(v2) = dash.get("signal_dashboard_v2") {
        if v2.get("schema_version").and_then(|x| x.as_u64()) == Some(3) {
            if let Some(m) = v2.get("market_mode").and_then(|x| x.as_str()) {
                let s = m.trim();
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }
    dash.get("piyasa_modu")
        .and_then(|x| x.as_str())
        .unwrap_or("uncertain")
        .to_string()
}

fn pillar_from_durum_and_strength(durum: &str, strength_0_10: f64) -> f64 {
    let st = (strength_0_10 / 10.0).clamp(0.0, 1.0);
    let d = durum.trim().to_uppercase().replace('İ', "I");
    let base = match d.as_str() {
        "LONG" => 0.65,
        "SHORT" => -0.65,
        _ => 0.0,
    };
    (base * st).clamp(-1.0, 1.0)
}

async fn load_regime_weights(pool: &PgPool, regime: &str) -> (f64, f64, f64) {
    let w = match AppConfigRepository::get_value_json(pool, CONF_CONFIG_KEY).await {
        Ok(Some(v)) if v.is_object() => v,
        Ok(Some(_)) | Ok(None) => default_weights_by_regime(),
        Err(e) => {
            warn!(%e, "confluence_weights_by_regime read failed — defaults");
            default_weights_by_regime()
        }
    };
    let fallback = default_weights_by_regime();
    let regime_obj = w
        .get(regime)
        .and_then(|x| x.as_object())
        .or_else(|| fallback.get(regime).and_then(|x| x.as_object()));
    let o = regime_obj.cloned().unwrap_or_else(|| {
        fallback
            .get(regime)
            .and_then(|x| x.as_object())
            .cloned()
            .unwrap_or_default()
    });
    let t = o
        .get("technical")
        .and_then(|x| x.as_f64())
        .or_else(|| o.get("technical").and_then(|x| x.as_i64().map(|i| i as f64)))
        .unwrap_or(0.33);
    let c = o
        .get("onchain")
        .and_then(|x| x.as_f64())
        .or_else(|| o.get("onchain").and_then(|x| x.as_i64().map(|i| i as f64)))
        .unwrap_or(0.34);
    let s = o
        .get("smart_money")
        .and_then(|x| x.as_f64())
        .or_else(|| o.get("smart_money").and_then(|x| x.as_i64().map(|i| i as f64)))
        .unwrap_or(0.33);
    let sum = t + c + s;
    if sum <= 1e-9 {
        return (0.33, 0.34, 0.33);
    }
    (t / sum, c / sum, s / sum)
}

fn technical_pillar_score(dash: &Value) -> f64 {
    if let Some(v2) = dash.get("signal_dashboard_v2") {
        if v2.get("schema_version").and_then(|x| x.as_u64()) == Some(3) {
            let st = v2.get("status").and_then(|x| x.as_str()).unwrap_or("");
            let str_u = v2
                .get("position_strength_10")
                .and_then(|x| x.as_u64().or_else(|| x.as_i64().map(|i| i as u64)))
                .unwrap_or(5) as f64;
            if !st.trim().is_empty() {
                return pillar_from_durum_and_strength(st, str_u);
            }
        }
    }
    let durum = dash.get("durum").and_then(|x| x.as_str()).unwrap_or("NOTR");
    let strength = dash
        .get("pozisyon_gucu_10")
        .and_then(|x| x.as_u64().or_else(|| x.as_i64().map(|i| i as u64)))
        .unwrap_or(5) as f64;
    pillar_from_durum_and_strength(durum, strength)
}

async fn data_snapshot_json(pool: &PgPool, key: &str) -> Option<Value> {
    let row = fetch_data_snapshot(pool, key).await.ok()??;
    if row.error.is_some() {
        return None;
    }
    row.response_json
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

/// Taker + funding + OI + HL + (BTC ise) Coinglass netflow/likidasyon — ağırlıklı ortalama (mevcut sinyaller).
async fn onchain_pillar_score(pool: &PgPool, symbol: &str) -> f64 {
    let base = symbol_base_lower(symbol);
    let coin = hl_perp_coin(symbol);
    let mut parts: Vec<(f64, f64)> = Vec::new();

    let taker_key = format!("binance_taker_{base}usdt");
    if let Some(j) = data_snapshot_json(pool, &taker_key).await {
        parts.push((score_for_source_key(&taker_key, &j), 0.26));
    }

    let prem_key = format!("binance_premium_{base}usdt");
    if let Some(j) = data_snapshot_json(pool, &prem_key).await {
        parts.push((score_for_source_key(&prem_key, &j), 0.28));
    }

    let oi_key = format!("binance_open_interest_{base}usdt");
    if let Some(j) = data_snapshot_json(pool, &oi_key).await {
        parts.push((score_for_source_key(&oi_key, &j), 0.10));
    }

    if let Some(j) = data_snapshot_json(pool, "hl_meta_asset_ctxs").await {
        let s = score_hl_meta_asset_ctxs_for_coin(&j, &coin);
        parts.push((s, 0.22));
    }

    if base == "btc" {
        if let Some(j) = data_snapshot_json(pool, "coinglass_netflow_btc").await {
            parts.push((score_coinglass_netflow_like(&j), 0.18));
        }
        if let Some(j) = data_snapshot_json(pool, "coinglass_liquidations_btc").await {
            parts.push((score_coinglass_liquidations_like(&j), 0.16));
        }
    }

    let wsum: f64 = parts.iter().map(|(_, w)| w).sum();
    if wsum <= 1e-12 {
        return 0.0;
    }
    parts.iter().map(|(s, w)| s * w).sum::<f64>() / wsum
}

/// Confluence JSON traceability — okunan `data_snapshots` anahtarları (PLAN §4.2 / §8).
fn build_data_sources_considered(symbol: &str) -> Vec<String> {
    let base = symbol_base_lower(symbol);
    let mut v = vec![
        NANSEN_TOKEN_SCREENER_DATA_KEY.to_string(),
        format!("binance_taker_{base}usdt"),
        format!("binance_premium_{base}usdt"),
        format!("binance_open_interest_{base}usdt"),
        "hl_meta_asset_ctxs".to_string(),
    ];
    if base == "btc" {
        v.push("coinglass_netflow_btc".to_string());
        v.push("coinglass_liquidations_btc".to_string());
    }
    v
}

fn smart_money_pillar_from_nansen(nansen_json: Option<&Value>) -> f64 {
    let Some(j) = nansen_json else {
        return 0.0;
    };
    let depth = score_nansen_smart_money_depth(j);
    let dex = score_nansen_dex_buy_sell_pressure(j);
    if dex.abs() < 1e-12 {
        depth
    } else {
        (0.55 * depth + 0.45 * dex).clamp(-1.0, 1.0)
    }
}

fn category_funding_oi(premium_json: Option<&Value>, oi_json: Option<&Value>) -> f64 {
    let mut parts: Vec<(f64, f64)> = Vec::new();
    if let Some(j) = premium_json {
        parts.push((score_binance_premium_funding(j), 0.65));
    }
    if let Some(j) = oi_json {
        parts.push((score_binance_open_interest_heat(j), 0.35));
    }
    let w: f64 = parts.iter().map(|(_, w)| w).sum();
    if w <= 1e-12 {
        return 0.0;
    }
    parts.iter().map(|(s, w)| s * w).sum::<f64>() / w
}

/// PLAN Phase B / pazar matrisi — ham kategori skorları (payload + `market_confluence_snapshots.scores_json`).
fn build_category_scores_json(
    smart_money_depth: f64,
    cex_flow: f64,
    dex_pressure: f64,
    hyperliquid: f64,
    funding_oi: f64,
    liquidations: f64,
    composite: f64,
) -> Value {
    json!({
        "smart_money": smart_money_depth,
        "cex_flow": cex_flow,
        "dex_pressure": dex_pressure,
        "hyperliquid": hyperliquid,
        "funding_oi": funding_oi,
        "liquidations": liquidations,
        "composite": composite
    })
}

pub async fn compute_and_persist(
    pool: &PgPool,
    t: &EngineSymbolRow,
    dash_payload: &Value,
    last_bar_open_time: DateTime<Utc>,
    bar_count: i32,
) -> Result<(), String> {
    if !confluence_engine_enabled() {
        return Ok(());
    }
    if dash_payload
        .get("reason")
        .and_then(|x| x.as_str())
        == Some("insufficient_bars")
    {
        return Ok(());
    }

    let market_mode_raw = effective_market_mode_label(dash_payload);
    let regime = map_market_mode_to_regime(&market_mode_raw);
    let (wt, wo, ws) = load_regime_weights(pool, regime).await;

    let technical = technical_pillar_score(dash_payload);
    let onchain = onchain_pillar_score(pool, &t.symbol).await;
    let base = symbol_base_lower(&t.symbol);
    let coin = hl_perp_coin(&t.symbol);

    let nansen_json = data_snapshot_json(pool, NANSEN_TOKEN_SCREENER_DATA_KEY).await;
    let nansen_ref = nansen_json.as_ref();
    let smart_money = smart_money_pillar_from_nansen(nansen_ref);
    let dex_pressure_cat = nansen_ref
        .map(score_nansen_dex_buy_sell_pressure)
        .unwrap_or(0.0);
    let smart_money_depth_cat = nansen_ref
        .map(score_nansen_smart_money_depth)
        .unwrap_or(0.0);

    let prem_json = data_snapshot_json(pool, &format!("binance_premium_{base}usdt")).await;
    let oi_json = data_snapshot_json(pool, &format!("binance_open_interest_{base}usdt")).await;
    let funding_oi_cat = category_funding_oi(prem_json.as_ref(), oi_json.as_ref());

    let hyperliquid_cat = if let Some(j) = data_snapshot_json(pool, "hl_meta_asset_ctxs").await {
        score_hl_meta_asset_ctxs_for_coin(&j, &coin)
    } else {
        0.0
    };

    let (cex_flow_cat, liquidations_cat) = if base == "btc" {
        let cf = data_snapshot_json(pool, "coinglass_netflow_btc")
            .await
            .map(|j| score_coinglass_netflow_like(&j))
            .unwrap_or(0.0);
        let lq = data_snapshot_json(pool, "coinglass_liquidations_btc")
            .await
            .map(|j| score_coinglass_liquidations_like(&j))
            .unwrap_or(0.0);
        (cf, lq)
    } else {
        (0.0, 0.0)
    };

    let data_sources_considered = build_data_sources_considered(&t.symbol);

    let composite_score =
        (wt * technical + wo * onchain + ws * smart_money).clamp(-1.0, 1.0);

    let mut conflicts: Vec<Value> = Vec::new();
    if technical > 0.25 && onchain < -0.25 {
        conflicts.push(json!({
            "code": "ta_long_vs_onchain_bearish",
            "severity": "warn"
        }));
    }
    if technical < -0.25 && onchain > 0.25 {
        conflicts.push(json!({
            "code": "ta_short_vs_onchain_bullish",
            "severity": "warn"
        }));
    }
    if regime == "breakout" && onchain.abs() > 0.32 {
        conflicts.push(json!({
            "code": "breakout_regime_strong_onchain_bias",
            "severity": "warn"
        }));
    }
    if technical.abs() > 0.35 && smart_money < 0.12 {
        conflicts.push(json!({
            "code": "strong_ta_thin_smart_money",
            "severity": "warn"
        }));
    }
    if technical * composite_score < -0.05
        && technical.abs() > 0.22
        && composite_score.abs() > 0.08
    {
        conflicts.push(json!({
            "code": "technical_vs_weighted_composite_opposed",
            "severity": "warn"
        }));
    }

    let mut confidence =
        (((composite_score + 1.0) / 2.0) * 100.0).round() as i32;
    confidence -= (conflicts.len() as i32) * 15;
    confidence = confidence.clamp(0, 100);

    let lot_scale_hint = lot_scale_hint_from_conflict_count(conflicts.len());

    let direction = direction_from_composite_score(composite_score);

    let weights_used = json!({
        "technical": wt,
        "onchain": wo,
        "smart_money": ws
    });

    let payload = json!({
        "schema_version": 2,
        "regime": regime,
        "market_mode_raw": market_mode_raw,
        "direction": direction,
        "pillar_scores": {
            "technical": technical,
            "onchain": onchain,
            "smart_money": smart_money
        },
        "weights_used": weights_used,
        "composite_score": composite_score,
        "confidence_0_100": confidence,
        "conflicts": conflicts,
        "lot_scale_hint": lot_scale_hint,
        "symbol": t.symbol,
        "exchange": t.exchange,
        "segment": t.segment,
        "interval": t.interval,
        "data_sources_considered": data_sources_considered,
        "category_scores": {
            "smart_money_depth": smart_money_depth_cat,
            "dex_pressure": dex_pressure_cat,
            "cex_flow": cex_flow_cat,
            "hyperliquid": hyperliquid_cat,
            "funding_oi": funding_oi_cat,
            "liquidations": liquidations_cat
        }
    });

    upsert_analysis_snapshot(
        pool,
        t.id,
        "confluence",
        &payload,
        Some(last_bar_open_time),
        Some(bar_count),
        None,
    )
    .await
    .map_err(|e| e.to_string())?;

    let scores_json = build_category_scores_json(
        smart_money_depth_cat,
        cex_flow_cat,
        dex_pressure_cat,
        hyperliquid_cat,
        funding_oi_cat,
        liquidations_cat,
        composite_score,
    );
    let conflicts_json = Value::Array(conflicts.clone());
    let hist = MarketConfluenceSnapshotInsert {
        engine_symbol_id: t.id,
        schema_version: 1,
        regime: Some(regime.to_string()),
        composite_score,
        confidence_0_100: confidence,
        scores_json,
        conflicts_json,
        confluence_payload_json: Some(payload.clone()),
    };
    if let Err(e) = insert_market_confluence_snapshot(pool, &hist).await {
        warn!(
            %e,
            symbol = %t.symbol,
            "market_confluence_snapshots insert failed (analysis_snapshots confluence still updated)"
        );
    }

    info!(
        symbol = %t.symbol,
        interval = %t.interval,
        regime,
        composite_score,
        confidence_0_100 = confidence,
        lot_scale_hint,
        conflicts = conflicts.len(),
        "confluence snapshot persisted"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn regime_maps_english_v2_market_modes() {
        assert_eq!(map_market_mode_to_regime("range"), "range");
        assert_eq!(map_market_mode_to_regime("BREAKOUT"), "breakout");
        assert_eq!(map_market_mode_to_regime("Trend"), "trend");
        assert_eq!(map_market_mode_to_regime("uncertain"), "uncertain");
    }

    #[test]
    fn regime_maps_turkish_legacy_modes() {
        assert_eq!(map_market_mode_to_regime("RANGE"), "range");
        assert_eq!(map_market_mode_to_regime("KOPUS"), "breakout");
        assert_eq!(map_market_mode_to_regime("BELIRSIZ"), "uncertain");
    }

    #[test]
    fn effective_market_mode_prefers_v2() {
        let dash = json!({
            "piyasa_modu": "RANGE",
            "signal_dashboard_v2": { "schema_version": 3, "market_mode": "trend" }
        });
        assert_eq!(effective_market_mode_label(&dash), "trend");
    }

    #[test]
    fn data_sources_considered_lists_concrete_taker_key() {
        let v = build_data_sources_considered("BTCUSDT");
        assert!(v.contains(&"nansen_token_screener".to_string()));
        assert!(v.contains(&"binance_taker_btcusdt".to_string()));
        assert!(v.contains(&"binance_premium_btcusdt".to_string()));
        assert!(v.contains(&"coinglass_netflow_btc".to_string()));
    }

    #[test]
    fn composite_maps_direction_labels() {
        assert_eq!(direction_from_composite_score(0.7), "strong_buy");
        assert_eq!(direction_from_composite_score(0.3), "buy");
        assert_eq!(direction_from_composite_score(0.0), "neutral");
        assert_eq!(direction_from_composite_score(-0.4), "sell");
        assert_eq!(direction_from_composite_score(-0.8), "strong_sell");
    }

    #[test]
    fn technical_pillar_prefers_v2_status() {
        let dash = json!({
            "durum": "NOTR",
            "signal_dashboard_v2": {
                "schema_version": 3,
                "status": "LONG",
                "position_strength_10": 10
            }
        });
        let s = technical_pillar_score(&dash);
        assert!(s > 0.5);
    }
}

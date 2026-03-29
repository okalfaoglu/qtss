//! Regime-weighted confluence → `analysis_snapshots` with `engine_kind = "confluence"`.
//! Reads `data_snapshots` + `app_config.confluence_weights_by_regime` (English keys).

use chrono::{DateTime, Utc};
use qtss_storage::{
    fetch_data_snapshot, upsert_analysis_snapshot, AppConfigRepository, EngineSymbolRow,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{info, warn};

use crate::data_sources::registry::NANSEN_TOKEN_SCREENER_DATA_KEY;

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

fn map_market_mode_to_regime(piyasa_modu: &str) -> &'static str {
    match piyasa_modu.trim().to_uppercase().as_str() {
        "RANGE" => "range",
        "KOPUS" => "breakout",
        "TREND" => "trend",
        "BELIRSIZ" | "BELİRSİZ" => "uncertain",
        _ => "uncertain",
    }
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
    let durum = dash.get("durum").and_then(|x| x.as_str()).unwrap_or("NOTR");
    let strength = dash
        .get("pozisyon_gucu_10")
        .and_then(|x| x.as_u64().or_else(|| x.as_i64().map(|i| i as u64)))
        .unwrap_or(5) as f64
        / 10.0;
    let base = match durum {
        "LONG" => 0.65,
        "SHORT" => -0.65,
        _ => 0.0,
    };
    (base * strength).clamp(-1.0, 1.0)
}

fn parse_binance_taker_bias(resp: &Value) -> Option<f64> {
    let arr = resp.as_array()?;
    let last = arr.last()?;
    let r: f64 = last
        .get("buySellRatio")
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse().ok())
        .or_else(|| last.get("buySellRatio").and_then(|x| x.as_f64()))?;
    if r > 1.06 {
        Some(0.45_f64.min((r - 1.0) * 1.5).clamp(0.0, 1.0))
    } else if r < 0.94 {
        Some(-0.45_f64.min((1.0 - r) * 1.5).clamp(-1.0, 0.0))
    } else {
        Some(0.0)
    }
}

async fn onchain_pillar_score(pool: &PgPool, symbol: &str) -> f64 {
    let sym = symbol.to_uppercase();
    let base = sym
        .strip_suffix("USDT")
        .unwrap_or(sym.as_str())
        .to_lowercase();
    let key = format!("binance_taker_{}usdt", base);
    let Ok(Some(row)) = fetch_data_snapshot(pool, &key).await else {
        return 0.0;
    };
    if row.error.is_some() {
        return 0.0;
    }
    let Some(rj) = row.response_json else {
        return 0.0;
    };
    parse_binance_taker_bias(&rj).unwrap_or(0.0)
}

async fn smart_money_pillar_score(pool: &PgPool) -> f64 {
    let Ok(Some(row)) = fetch_data_snapshot(pool, NANSEN_TOKEN_SCREENER_DATA_KEY).await else {
        return 0.0;
    };
    if row.error.is_some() {
        return 0.0;
    }
    let Some(rj) = row.response_json else {
        return 0.0;
    };
    let n = rj
        .get("data")
        .and_then(|d| d.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    if n >= 80 {
        0.55
    } else if n >= 20 {
        0.35
    } else if n > 0 {
        0.15
    } else {
        0.0
    }
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

    let piyasa = dash_payload
        .get("piyasa_modu")
        .and_then(|x| x.as_str())
        .unwrap_or("BELIRSIZ");
    let regime = map_market_mode_to_regime(piyasa);
    let (wt, wo, ws) = load_regime_weights(pool, regime).await;

    let technical = technical_pillar_score(dash_payload);
    let onchain = onchain_pillar_score(pool, &t.symbol).await;
    let smart_money = smart_money_pillar_score(pool).await;

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

    let weights_used = json!({
        "technical": wt,
        "onchain": wo,
        "smart_money": ws
    });

    let payload = json!({
        "schema_version": 2,
        "regime": regime,
        "market_mode_raw": piyasa,
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
        "data_sources_considered": [NANSEN_TOKEN_SCREENER_DATA_KEY, "binance_taker_*"]
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

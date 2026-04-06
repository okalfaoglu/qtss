//! Rule-based smart-money playbook sweeps → `intake_playbook_*` tables (gates Elliott / ACP / TBM / AI).
//!
//! Data: `data_snapshots` (`nansen_token_screener`, `nansen_netflows`, `nansen_flow_intelligence`,
//! `nansen_perp_trades`, `binance_premium_*`). Heuristics are best-effort; Nansen JSON shape may vary.
//!
//! Enable: `QTSS_INTAKE_PLAYBOOK_ENABLED=1` or `system_config` `worker` / `intake_playbook_loop_enabled` `{ "enabled": true }`.

use chrono::{Duration, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{info, warn};

use qtss_storage::{
    fetch_data_snapshot, insert_intake_playbook_candidates, insert_intake_playbook_run,
    resolve_worker_enabled_flag, resolve_worker_tick_secs, IntakePlaybookCandidateInsert,
    IntakePlaybookRunInsert,
};

use crate::data_sources::registry::{
    NANSEN_FLOW_INTELLIGENCE_DATA_KEY, NANSEN_NETFLOWS_DATA_KEY, NANSEN_PERP_TRADES_DATA_KEY,
    NANSEN_TOKEN_SCREENER_DATA_KEY,
};
use crate::signal_scorer::{
    score_coinglass_netflow_like, score_nansen_netflows, score_nansen_perp_direction,
};

pub const PLAYBOOK_MARKET_MODE: &str = "market_mode";
pub const PLAYBOOK_ELITE_SHORT: &str = "elite_short";
pub const PLAYBOOK_ELITE_LONG: &str = "elite_long";
pub const PLAYBOOK_TEN_X: &str = "ten_x_alert";
pub const PLAYBOOK_INSTITUTIONAL_EXIT: &str = "institutional_exit";
pub const PLAYBOOK_INSTITUTIONAL_ACCUM: &str = "institutional_accumulation";
pub const PLAYBOOK_EXPLOSIVE: &str = "explosive_high_risk";
pub const PLAYBOOK_EARLY_ACCUM: &str = "early_accumulation_24h";

const MAJORS_INFLOW_USD_LONG: f64 = 10_000_000.0;
const MAJORS_OUTFLOW_USD_SHORT: f64 = 10_000_000.0;

fn parse_json_value_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .or_else(|| v.as_i64().map(|i| i as f64))
}

fn parse_json_f64_opt(v: Option<&Value>) -> Option<f64> {
    v.and_then(parse_json_value_f64)
}

#[derive(Debug, Clone)]
struct ScreenerRow {
    symbol: String,
    chain: Option<String>,
    net_flow: f64,
    buy_vol: f64,
    sell_vol: f64,
    price_change_pct: f64,
    volume_usd: f64,
    liquidity_usd: f64,
    mcap_usd: f64,
    nof_traders: f64,
    token_age_days: f64,
    volume_change_pct: f64,
    raw: Value,
}

fn token_symbol_from_row(row: &Value) -> Option<String> {
    let s = row
        .get("token_symbol")
        .or_else(|| row.get("symbol"))
        .or_else(|| row.get("tokenSymbol"))
        .and_then(|x| x.as_str())?;
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    Some(t.to_uppercase())
}

fn chain_from_row(row: &Value) -> Option<String> {
    row.get("chain")
        .or_else(|| row.get("chain_name"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}

fn screener_rows(response: &Value) -> Vec<ScreenerRow> {
    let Some(arr) = response.get("data").and_then(|d| d.as_array()) else {
        return vec![];
    };
    let mut out = Vec::new();
    for row in arr.iter().take(500) {
        let Some(symbol) = token_symbol_from_row(row) else {
            continue;
        };
        let net_flow = parse_json_f64_opt(row.get("net_flow"))
            .or_else(|| parse_json_f64_opt(row.get("netFlow")))
            .or_else(|| parse_json_f64_opt(row.get("netflow")))
            .unwrap_or(0.0);
        let buy_vol = parse_json_f64_opt(row.get("buy_volume"))
            .or_else(|| parse_json_f64_opt(row.get("buyVolume")))
            .or_else(|| parse_json_f64_opt(row.get("dex_buy_volume")))
            .unwrap_or(0.0)
            .max(0.0);
        let sell_vol = parse_json_f64_opt(row.get("sell_volume"))
            .or_else(|| parse_json_f64_opt(row.get("sellVolume")))
            .or_else(|| parse_json_f64_opt(row.get("dex_sell_volume")))
            .unwrap_or(0.0)
            .max(0.0);
        let price_change_pct = parse_json_f64_opt(row.get("price_change_pct"))
            .or_else(|| parse_json_f64_opt(row.get("priceChangePct")))
            .or_else(|| parse_json_f64_opt(row.get("price_change_24h")))
            .or_else(|| parse_json_f64_opt(row.get("price_change")))
            .unwrap_or(0.0);
        let volume_usd = parse_json_f64_opt(row.get("volume"))
            .or_else(|| parse_json_f64_opt(row.get("volume_usd")))
            .or_else(|| parse_json_f64_opt(row.get("volumeUsd")))
            .unwrap_or(0.0)
            .max(0.0);
        let liquidity_usd = parse_json_f64_opt(row.get("liquidity"))
            .or_else(|| parse_json_f64_opt(row.get("liquidity_usd")))
            .unwrap_or(0.0)
            .max(0.0);
        let mcap_usd = parse_json_f64_opt(row.get("market_cap"))
            .or_else(|| parse_json_f64_opt(row.get("marketCap")))
            .or_else(|| parse_json_f64_opt(row.get("mcap")))
            .unwrap_or(0.0)
            .max(0.0);
        let nof_traders = parse_json_f64_opt(row.get("nof_traders"))
            .or_else(|| parse_json_f64_opt(row.get("nofTraders")))
            .or_else(|| parse_json_f64_opt(row.get("trader_count")))
            .unwrap_or(0.0);
        let token_age_days = parse_json_f64_opt(row.get("token_age_days"))
            .or_else(|| parse_json_f64_opt(row.get("tokenAgeDays")))
            .unwrap_or(999.0);
        let volume_change_pct = parse_json_f64_opt(row.get("volume_change_pct"))
            .or_else(|| parse_json_f64_opt(row.get("volumeChangePct")))
            .or_else(|| parse_json_f64_opt(row.get("volume_change")))
            .unwrap_or(0.0);

        out.push(ScreenerRow {
            symbol,
            chain: chain_from_row(row),
            net_flow,
            buy_vol,
            sell_vol,
            price_change_pct,
            volume_usd,
            liquidity_usd,
            mcap_usd,
            nof_traders,
            token_age_days,
            volume_change_pct,
            raw: row.clone(),
        });
    }
    out
}

/// Sum `net_flow` (USD) for BTC/ETH-like rows in Nansen netflows payload.
fn majors_netflow_usd(netflows_json: Option<&Value>) -> f64 {
    let Some(v) = netflows_json else {
        return 0.0;
    };
    let data = match v.get("data") {
        Some(d) => d,
        None => return 0.0,
    };
    let rows: Vec<&Value> = if let Some(a) = data.as_array() {
        a.iter().collect()
    } else {
        vec![data]
    };
    let mut sum = 0_f64;
    for row in rows.iter().take(2000) {
        let sym = row
            .get("symbol")
            .or_else(|| row.get("token_symbol"))
            .or_else(|| row.get("tokenSymbol"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_uppercase();
        if !(sym.contains("BTC") || sym.contains("ETH")) {
            continue;
        }
        let nf = parse_json_f64_opt(row.get("net_flow"))
            .or_else(|| parse_json_f64_opt(row.get("netFlow")))
            .unwrap_or(0.0);
        sum += nf;
    }
    sum
}

fn binance_funding_rate(resp: &Value) -> Option<f64> {
    let s = resp.get("lastFundingRate")?.as_str()?;
    s.parse::<f64>().ok()
}

async fn avg_btc_eth_funding_async(pool: &PgPool) -> Option<f64> {
    let mut rates = Vec::new();
    for base in ["btc", "eth"] {
        let key = format!("binance_premium_{base}usdt");
        if let Ok(Some(row)) = fetch_data_snapshot(pool, &key).await {
            if let Some(j) = row.response_json.as_ref() {
                if let Some(fr) = binance_funding_rate(j) {
                    rates.push(fr);
                }
            }
        }
    }
    if rates.is_empty() {
        return None;
    }
    Some(rates.iter().sum::<f64>() / rates.len() as f64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarketMode {
    Long,
    Short,
    Neutral,
}

fn decide_market_mode(
    majors_nf: f64,
    flow_intel_score: f64,
    netflow_score: f64,
    perp_dir: f64,
    funding_avg: Option<f64>,
) -> (MarketMode, i32, String, serde_json::Value) {
    let mut long_votes = 0_i32;
    let mut short_votes = 0_i32;
    let mut reasons = Vec::new();

    if majors_nf >= MAJORS_INFLOW_USD_LONG {
        long_votes += 2;
        reasons.push(format!("majors_netflow_usd>={MAJORS_INFLOW_USD_LONG:.0} ({majors_nf:.0})"));
    } else if majors_nf <= -MAJORS_OUTFLOW_USD_SHORT {
        short_votes += 2;
        reasons.push(format!(
            "majors_netflow_usd<=-{MAJORS_OUTFLOW_USD_SHORT:.0} ({majors_nf:.0})"
        ));
    }

    if flow_intel_score > 0.15 {
        long_votes += 1;
        reasons.push("flow_intelligence_accumulation_bias".into());
    } else if flow_intel_score < -0.15 {
        short_votes += 1;
        reasons.push("flow_intelligence_distribution_bias".into());
    }

    if netflow_score > 0.2 {
        long_votes += 1;
        reasons.push("nansen_netflows_breadth_bullish".into());
    } else if netflow_score < -0.2 {
        short_votes += 1;
        reasons.push("nansen_netflows_breadth_bearish".into());
    }

    if perp_dir > 0.2 {
        long_votes += 1;
        reasons.push("perp_smart_long_tilt".into());
    } else if perp_dir < -0.2 {
        short_votes += 1;
        reasons.push("perp_smart_short_tilt".into());
    }

    if let Some(fr) = funding_avg {
        if fr <= 0.000_05 {
            long_votes += 1;
            reasons.push(format!("funding_neutral_or_negative ({fr:.6})"));
        } else if fr >= 0.0003 {
            short_votes += 1;
            reasons.push(format!("funding_positive_crowded_long ({fr:.6})"));
        }
    }

    let mode = if long_votes >= 4 && long_votes > short_votes {
        MarketMode::Long
    } else if short_votes >= 4 && short_votes > long_votes {
        MarketMode::Short
    } else {
        MarketMode::Neutral
    };

    let max_v = long_votes.max(short_votes);
    let confidence = ((max_v * 100) / 7).min(100).max(if mode == MarketMode::Neutral { 35 } else { 45 });

    let key_reason = if reasons.is_empty() {
        "insufficient_data_snapshots".into()
    } else {
        reasons[0].clone()
    };

    let inputs = json!({
        "majors_netflow_usd": majors_nf,
        "flow_intel_score": flow_intel_score,
        "netflow_score": netflow_score,
        "perp_dir_score": perp_dir,
        "funding_avg_optional": funding_avg,
        "long_votes": long_votes,
        "short_votes": short_votes,
        "reason_flags": reasons,
    });

    (
        mode,
        confidence,
        key_reason,
        inputs,
    )
}

fn buy_ratio(row: &ScreenerRow) -> f64 {
    let t = row.buy_vol + row.sell_vol;
    if t < 1e-9 {
        return 0.5;
    }
    row.buy_vol / t
}

fn pick_long_candidates(rows: &[ScreenerRow], limit: usize) -> Vec<&ScreenerRow> {
    let mut v: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| {
            r.net_flow > 0.0
                && r.price_change_pct > -8.0
                && r.price_change_pct < 18.0
                && r.liquidity_usd >= 500_000.0
                && r.token_age_days >= 7.0
        })
        .collect();
    v.sort_by(|a, b| {
        let sa = a.net_flow + a.buy_vol * 0.0001;
        let sb = b.net_flow + b.buy_vol * 0.0001;
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(limit);
    v
}

fn pick_short_candidates(rows: &[ScreenerRow], limit: usize) -> Vec<&ScreenerRow> {
    let mut v: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| {
            r.net_flow < 0.0
                && r.price_change_pct > -5.0
                && r.liquidity_usd >= 500_000.0
                && r.token_age_days >= 7.0
        })
        .collect();
    v.sort_by(|a, b| {
        let sa = -a.net_flow + a.sell_vol * 0.0001;
        let sb = -b.net_flow + b.sell_vol * 0.0001;
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(limit);
    v
}

fn buy_to_sell_ratio(row: &ScreenerRow) -> f64 {
    if row.sell_vol < 1e-9 {
        return 999.0;
    }
    row.buy_vol / row.sell_vol
}

fn pick_elite_long(rows: &[ScreenerRow]) -> Vec<&ScreenerRow> {
    let mut v: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| {
            r.net_flow > 500_000.0
                && buy_to_sell_ratio(r) >= 2.0
                && r.price_change_pct >= -2.0
                && r.price_change_pct <= 10.0
                && r.volume_usd > 0.0
                && r.liquidity_usd >= 500_000.0
                && r.mcap_usd > 0.0
                && r.mcap_usd <= 120_000_000.0
        })
        .collect();
    v.sort_by(|a, b| b.net_flow.partial_cmp(&a.net_flow).unwrap_or(std::cmp::Ordering::Equal));
    v.truncate(3);
    v
}

fn pick_elite_short(rows: &[ScreenerRow]) -> Vec<&ScreenerRow> {
    let mut v: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| {
            r.net_flow < -500_000.0
                && buy_ratio(r) > 0.55
                && r.price_change_pct >= 3.0
                && r.liquidity_usd >= 400_000.0
        })
        .collect();
    v.sort_by(|a, b| {
        a.net_flow
            .partial_cmp(&b.net_flow)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(3);
    v
}

fn pick_ten_x(rows: &[ScreenerRow]) -> Vec<&ScreenerRow> {
    let mut v: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| {
            r.net_flow > 100_000.0
                && r.nof_traders >= 3.0
                && buy_ratio(r) > 0.65
                && r.mcap_usd > 0.0
                && r.mcap_usd < 30_000_000.0
                && r.liquidity_usd >= 300_000.0
                && r.liquidity_usd <= 5_000_000.0
                && r.price_change_pct <= 20.0
                && r.net_flow > 0.0
        })
        .collect();
    v.sort_by(|a, b| b.net_flow.partial_cmp(&a.net_flow).unwrap_or(std::cmp::Ordering::Equal));
    v.truncate(1);
    v
}

fn pick_explosive(rows: &[ScreenerRow]) -> Vec<&ScreenerRow> {
    let mut v: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| {
            r.volume_change_pct >= 200.0
                && r.volume_usd >= 1_000_000.0
                && r.liquidity_usd >= 500_000.0
        })
        .collect();
    v.sort_by(|a, b| {
        b.volume_change_pct
            .partial_cmp(&a.volume_change_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(3);
    v
}

fn pick_early_accumulation(rows: &[ScreenerRow]) -> Vec<&ScreenerRow> {
    let mut v: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| {
            r.net_flow > 0.0
                && r.price_change_pct > -3.0
                && r.price_change_pct < 10.0
                && r.volume_change_pct > 30.0
                && r.mcap_usd > 0.0
                && r.mcap_usd < 500_000_000.0
        })
        .collect();
    v.sort_by(|a, b| b.net_flow.partial_cmp(&a.net_flow).unwrap_or(std::cmp::Ordering::Equal));
    v.truncate(15);
    v
}

fn institutional_exit_like(rows: &[ScreenerRow]) -> Vec<&ScreenerRow> {
    let mut v: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| {
            r.net_flow < -300_000.0
                && r.price_change_pct >= -2.0
                && r.liquidity_usd >= 500_000.0
                && r.token_age_days >= 7.0
        })
        .collect();
    v.sort_by(|a, b| a.net_flow.partial_cmp(&b.net_flow).unwrap_or(std::cmp::Ordering::Equal));
    v.truncate(5);
    v
}

fn institutional_accum_like(rows: &[ScreenerRow]) -> Vec<&ScreenerRow> {
    let mut v: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| {
            r.net_flow > 300_000.0
                && r.price_change_pct < 35.0
                && r.liquidity_usd >= 500_000.0
                && r.volume_change_pct > 0.0
        })
        .collect();
    v.sort_by(|a, b| b.net_flow.partial_cmp(&a.net_flow).unwrap_or(std::cmp::Ordering::Equal));
    v.truncate(5);
    v
}

fn candidates_from_rows<'a>(
    rows: &[&'a ScreenerRow],
    direction: &str,
    tier: &str,
    conf_base: i32,
) -> Vec<IntakePlaybookCandidateInsert<'a>> {
    rows.iter()
        .enumerate()
        .map(|(i, r)| IntakePlaybookCandidateInsert {
            rank: (i + 1) as i32,
            symbol: r.symbol.as_str(),
            chain: r.chain.as_deref(),
            direction,
            intake_tier: tier,
            confidence_0_100: (conf_base + (30 - (i as i32 * 5))).clamp(20, 92),
            detail_json: &r.raw,
        })
        .collect()
}

async fn persist_playbook(
    pool: &PgPool,
    playbook_id: &str,
    market_mode: Option<&str>,
    confidence: i32,
    key_reason: &str,
    neutral: Option<&str>,
    summary: Value,
    inputs: Value,
    meta: Value,
    candidates: Vec<IntakePlaybookCandidateInsert<'_>>,
) -> Result<(), qtss_storage::StorageError> {
    let expires = Utc::now() + Duration::hours(24);
    let run_id = insert_intake_playbook_run(
        pool,
        &IntakePlaybookRunInsert {
            playbook_id,
            expires_at: Some(expires),
            market_mode,
            confidence_0_100: confidence,
            key_reason: Some(key_reason),
            neutral_guidance: neutral,
            summary_json: &summary,
            inputs_json: &inputs,
            meta_json: &meta,
        },
    )
    .await?;
    if !candidates.is_empty() {
        insert_intake_playbook_candidates(pool, run_id, &candidates).await?;
    }
    Ok(())
}

async fn run_sweep(pool: &PgPool) -> Result<(), qtss_storage::StorageError> {
    let screener_j = fetch_data_snapshot(pool, NANSEN_TOKEN_SCREENER_DATA_KEY)
        .await?
        .and_then(|r| r.response_json);
    let nf_j = fetch_data_snapshot(pool, NANSEN_NETFLOWS_DATA_KEY)
        .await?
        .and_then(|r| r.response_json);
    let fi_j = fetch_data_snapshot(pool, NANSEN_FLOW_INTELLIGENCE_DATA_KEY)
        .await?
        .and_then(|r| r.response_json);
    let perp_j = fetch_data_snapshot(pool, NANSEN_PERP_TRADES_DATA_KEY)
        .await?
        .and_then(|r| r.response_json);

    let rows = screener_j
        .as_ref()
        .map(|j| screener_rows(j))
        .unwrap_or_default();

    let majors_nf = majors_netflow_usd(nf_j.as_ref());
    let flow_intel = fi_j.as_ref().map(score_coinglass_netflow_like).unwrap_or(0.0);
    let netflow_score = nf_j.as_ref().map(score_nansen_netflows).unwrap_or(0.0);
    let perp_dir = perp_j.as_ref().map(score_nansen_perp_direction).unwrap_or(0.0);
    let funding_avg = avg_btc_eth_funding_async(pool).await;

    let (mode, conf, key_reason, mode_inputs) =
        decide_market_mode(majors_nf, flow_intel, netflow_score, perp_dir, funding_avg);

    let mode_str = match mode {
        MarketMode::Long => "LONG_MODE",
        MarketMode::Short => "SHORT_MODE",
        MarketMode::Neutral => "NEUTRAL",
    };

    let neutral_guidance = if mode == MarketMode::Neutral {
        Some("wait / scalp only")
    } else {
        None
    };

    let long_picks = pick_long_candidates(&rows, 10);
    let short_picks = pick_short_candidates(&rows, 3);

    let mode_candidates: Vec<IntakePlaybookCandidateInsert<'_>> = match mode {
        MarketMode::Long => candidates_from_rows(&long_picks, "LONG", "core", 55),
        MarketMode::Short => candidates_from_rows(&short_picks, "SHORT", "core", 55),
        MarketMode::Neutral => vec![],
    };

    let mode_summary = json!({
        "current_mode": mode_str,
        "confidence_0_100": conf,
        "key_reason": key_reason,
        "neutral_guidance": neutral_guidance,
        "long_candidate_target_pct": 10,
        "short_candidate_target_pct": 10,
        "note": "Heuristic from data_snapshots; confirm with Nansen UI / LLM.",
    });

    persist_playbook(
        pool,
        PLAYBOOK_MARKET_MODE,
        Some(mode_str),
        conf,
        &key_reason,
        neutral_guidance,
        mode_summary,
        mode_inputs.clone(),
        json!({ "data_keys_checked": [NANSEN_TOKEN_SCREENER_DATA_KEY, NANSEN_NETFLOWS_DATA_KEY, NANSEN_FLOW_INTELLIGENCE_DATA_KEY, NANSEN_PERP_TRADES_DATA_KEY, "binance_premium_btcusdt", "binance_premium_ethusdt"] }),
        mode_candidates,
    )
    .await?;

    // Elite short / long
    let elite_s = pick_elite_short(&rows);
    persist_playbook(
        pool,
        PLAYBOOK_ELITE_SHORT,
        None,
        50,
        "elite_short_heuristic",
        None,
        json!({ "goal": "pump_distribution_dump", "horizon_hours": [1,4] }),
        mode_inputs.clone(),
        json!({ "strict_usd": 500_000 }),
        candidates_from_rows(&elite_s, "SHORT", "apex", 48),
    )
    .await?;

    let elite_l = pick_elite_long(&rows);
    persist_playbook(
        pool,
        PLAYBOOK_ELITE_LONG,
        None,
        50,
        "elite_long_heuristic",
        None,
        json!({ "goal": "pre_pump", "horizon_hours": [1,6] }),
        mode_inputs.clone(),
        json!({ "strict_usd": 500_000 }),
        candidates_from_rows(&elite_l, "LONG", "apex", 50),
    )
    .await?;

    let ten = pick_ten_x(&rows);
    let triggered = !ten.is_empty();
    let ten_summary = json!({
        "triggered": triggered,
        "tp_pct_tiers": [25, 50, 100],
        "sl_pct_range": [-10, -15],
    });
    let ten_conf = if triggered { 72 } else { 0 };
    let ten_key = if triggered {
        "ten_x_thresholds_met"
    } else {
        "no_ten_x_candidate_this_sweep"
    };
    persist_playbook(
        pool,
        PLAYBOOK_TEN_X,
        None,
        ten_conf,
        ten_key,
        None,
        ten_summary,
        mode_inputs.clone(),
        json!({}),
        candidates_from_rows(&ten, "LONG", "apex", 70),
    )
    .await?;

    let ex = institutional_exit_like(&rows);
    persist_playbook(
        pool,
        PLAYBOOK_INSTITUTIONAL_EXIT,
        None,
        45,
        "institutional_exit_proxy_netflow",
        None,
        json!({ "note": "Labeled custody flows require Nansen TGM / manual LLM" }),
        mode_inputs.clone(),
        json!({}),
        candidates_from_rows(&ex, "SHORT", "scan", 42),
    )
    .await?;

    let acc = institutional_accum_like(&rows);
    persist_playbook(
        pool,
        PLAYBOOK_INSTITUTIONAL_ACCUM,
        None,
        45,
        "institutional_accumulation_proxy_netflow",
        None,
        json!({}),
        mode_inputs.clone(),
        json!({}),
        candidates_from_rows(&acc, "LONG", "scan", 42),
    )
    .await?;

    let exp = pick_explosive(&rows);
    persist_playbook(
        pool,
        PLAYBOOK_EXPLOSIVE,
        None,
        48,
        "volume_spike_explosive",
        None,
        json!({ "min_volume_change_pct": 200, "min_volume_usd": 1_000_000 }),
        mode_inputs.clone(),
        json!({}),
        candidates_from_rows(&exp, "LONG_OR_SHORT", "apex", 45),
    )
    .await?;

    let early = pick_early_accumulation(&rows);
    persist_playbook(
        pool,
        PLAYBOOK_EARLY_ACCUM,
        None,
        44,
        "early_accumulation_flat_price_rising_flow",
        None,
        json!({ "horizon_hours": [6, 24] }),
        mode_inputs.clone(),
        json!({}),
        candidates_from_rows(&early, "LONG", "scan", 40),
    )
    .await?;

    Ok(())
}

pub async fn intake_playbook_loop(pool: PgPool) {
    info!("intake_playbook_engine: loop started (off unless QTSS_INTAKE_PLAYBOOK_ENABLED or system_config)");
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "worker",
            "intake_playbook_loop_enabled",
            "QTSS_INTAKE_PLAYBOOK_ENABLED",
            false,
        )
        .await;
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "intake_playbook_tick_secs",
            "QTSS_INTAKE_PLAYBOOK_TICK_SECS",
            300,
            60,
        )
        .await;

        if enabled {
            match run_sweep(&pool).await {
                Ok(()) => info!("intake_playbook sweep ok"),
                Err(e) => warn!(%e, "intake_playbook sweep failed"),
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(tick)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn majors_netflow_sums_eth_btc() {
        let j = json!({
            "data": [
                { "symbol": "ETH", "net_flow": 6e6 },
                { "symbol": "BTC", "net_flow": 5e6 },
                { "symbol": "PEPE", "net_flow": 1e9 }
            ]
        });
        assert!((majors_netflow_usd(Some(&j)) - 11e6).abs() < 1.0);
    }

    #[test]
    fn decide_long_when_strong_signals() {
        let (m, _, _, _) = decide_market_mode(11e6, 0.2, 0.3, 0.25, Some(-0.0001));
        assert_eq!(m, MarketMode::Long);
    }
}

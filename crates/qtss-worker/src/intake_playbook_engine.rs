//! Enhanced rule-based smart-money playbook → `intake_playbook_*` tables (gates Elliott / ACP / TBM / AI).
//!
//! ## Data Sources (all from `data_snapshots`)
//!
//! | Key | Nansen API | Used by |
//! |-----|-----------|---------|
//! | `nansen_token_screener` | `POST /api/v1/token-screener` | All `pick_*` heuristics |
//! | `nansen_netflows` | `POST /api/v1/smart-money/netflow` | `decide_market_mode`, `majors_netflow_usd` |
//! | `nansen_flow_intelligence` | `POST /api/v1/tgm/flow-intelligence` | `decide_market_mode` flow intel score |
//! | `nansen_perp_trades` | `POST /api/v1/smart-money/perp-trades` | `decide_market_mode` perp direction |
//! | `nansen_holdings` | `POST /api/v1/smart-money/holdings` | `decide_market_mode` holdings signal |
//! | `nansen_who_bought_sold` | `POST /api/v1/tgm/who-bought-sold` | Entity labels, market maker detection |
//! | `nansen_smart_money_dex_trades` | `POST /api/v1/smart-money/dex-trades` | SM DEX aggression, entity labels |
//! | `nansen_whale_perp_aggregate` | Merged `profiler/perp-positions` | Whale net exposure |
//! | `nansen_perp_screener` | `POST /api/v1/perp-screener` | Perp OI/funding aggregate |
//! | `nansen_tgm_indicators` | `POST /api/v1/tgm/indicators` | Risk/reward per-token indicators |
//! | `binance_premium_btcusdt` | Binance FAPI `premiumIndex` | Funding rate (BTC) |
//! | `binance_premium_ethusdt` | Binance FAPI `premiumIndex` | Funding rate (ETH) |
//!
//! Enable: `system_config` `worker` / `intake_playbook_loop_enabled` → `{ "enabled": true }`.
//!
//! Notifications via `notify_outbox`; `intake_ten_x_alert` deduped per symbol.

use chrono::{Duration, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{debug, info, warn};

use qtss_storage::{
    fetch_data_snapshot, fetch_latest_intake_playbook_run, insert_intake_playbook_candidates,
    insert_intake_playbook_run, resolve_system_csv, resolve_worker_enabled_flag, resolve_worker_tick_secs,
    NotifyOutboxRepository, IntakePlaybookCandidateInsert, IntakePlaybookRunInsert,
};

use crate::data_sources::registry::{
    NANSEN_FLOW_INTELLIGENCE_DATA_KEY, NANSEN_HOLDINGS_DATA_KEY, NANSEN_NETFLOWS_DATA_KEY,
    NANSEN_PERP_SCREENER_DATA_KEY, NANSEN_PERP_TRADES_DATA_KEY, NANSEN_SMART_MONEY_DEX_TRADES_DATA_KEY,
    NANSEN_TOKEN_SCREENER_DATA_KEY, NANSEN_TGM_INDICATORS_DATA_KEY, NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY,
    NANSEN_WHO_BOUGHT_DATA_KEY,
};
use crate::signal_scorer::{
    count_unique_entities, extract_entity_labels, has_market_maker_activity,
    score_coinglass_netflow_like, score_nansen_holdings_signal, score_nansen_netflows,
    score_nansen_perp_direction, score_perp_screener_aggregate, score_smart_money_dex_trades,
    score_whale_perp_aggregate,
};

// -- Playbook IDs -----------------------------------------------------------

pub const PLAYBOOK_MARKET_MODE: &str = "market_mode";
pub const PLAYBOOK_ELITE_SHORT: &str = "elite_short";
pub const PLAYBOOK_ELITE_LONG: &str = "elite_long";
pub const PLAYBOOK_TEN_X: &str = "ten_x_alert";
pub const PLAYBOOK_INSTITUTIONAL_EXIT: &str = "institutional_exit";
pub const PLAYBOOK_INSTITUTIONAL_ACCUM: &str = "institutional_accumulation";
pub const PLAYBOOK_EXPLOSIVE: &str = "explosive_high_risk";
pub const PLAYBOOK_EARLY_ACCUM: &str = "early_accumulation_24h";
pub const PLAYBOOK_TOKEN_ANALYSIS: &str = "token_analysis";

// -- Thresholds (centralised) -----------------------------------------------

const MAJORS_INFLOW_USD_LONG: f64 = 10_000_000.0;
const MAJORS_OUTFLOW_USD_SHORT: f64 = 10_000_000.0;
const ELITE_FLOW_USD: f64 = 500_000.0;
const INSTITUTIONAL_FLOW_USD: f64 = 300_000.0;
const TEN_X_FLOW_USD: f64 = 100_000.0;
const TEN_X_MCAP_MAX: f64 = 30_000_000.0;
const TEN_X_LIQ_MIN: f64 = 300_000.0;
const TEN_X_LIQ_MAX: f64 = 5_000_000.0;

// -- JSON helpers -----------------------------------------------------------

fn parse_json_value_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .or_else(|| v.as_i64().map(|i| i as f64))
}

fn parse_json_f64_opt(v: Option<&Value>) -> Option<f64> {
    v.and_then(parse_json_value_f64)
}

// -- ScreenerRow (enriched) -------------------------------------------------

#[derive(Debug, Clone)]
struct ScreenerRow {
    symbol: String,
    chain: Option<String>,
    #[allow(dead_code)]
    token_address: Option<String>,
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
    fresh_wallets: f64,
    nof_buy_wallets: f64,
    nof_sell_wallets: f64,
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
        let fresh_wallets = parse_json_f64_opt(row.get("fresh_wallets"))
            .or_else(|| parse_json_f64_opt(row.get("freshWallets")))
            .or_else(|| parse_json_f64_opt(row.get("fresh_wallet_count")))
            .unwrap_or(0.0);
        let nof_buy_wallets = parse_json_f64_opt(row.get("nof_buy_wallets"))
            .or_else(|| parse_json_f64_opt(row.get("nofBuyWallets")))
            .or_else(|| parse_json_f64_opt(row.get("unique_buyers")))
            .unwrap_or(0.0);
        let nof_sell_wallets = parse_json_f64_opt(row.get("nof_sell_wallets"))
            .or_else(|| parse_json_f64_opt(row.get("nofSellWallets")))
            .or_else(|| parse_json_f64_opt(row.get("unique_sellers")))
            .unwrap_or(0.0);
        let token_address = row
            .get("token_address")
            .or_else(|| row.get("tokenAddress"))
            .or_else(|| row.get("address"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());

        out.push(ScreenerRow {
            symbol,
            chain: chain_from_row(row),
            token_address,
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
            fresh_wallets,
            nof_buy_wallets,
            nof_sell_wallets,
            raw: row.clone(),
        });
    }
    out
}

// -- Auxiliary data context (all extra snapshots) ---------------------------

struct AuxData {
    holdings_score: f64,
    whale_perp_score: f64,
    sm_dex_score: f64,
    perp_screener_score: f64,
    buy_entities: Vec<String>,
    sell_entities: Vec<String>,
    mm_buying: bool,
    mm_selling: bool,
    #[allow(dead_code)]
    indicators_json: Option<Value>,
}

async fn load_aux_data(pool: &PgPool) -> AuxData {
    let holdings_j = fetch_data_snapshot(pool, NANSEN_HOLDINGS_DATA_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.response_json);
    let whale_perp_j = fetch_data_snapshot(pool, NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.response_json);
    let sm_dex_j = fetch_data_snapshot(pool, NANSEN_SMART_MONEY_DEX_TRADES_DATA_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.response_json);
    let perp_scr_j = fetch_data_snapshot(pool, NANSEN_PERP_SCREENER_DATA_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.response_json);
    let who_j = fetch_data_snapshot(pool, NANSEN_WHO_BOUGHT_DATA_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.response_json);
    let indicators_j = fetch_data_snapshot(pool, NANSEN_TGM_INDICATORS_DATA_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.response_json);

    let holdings_score = holdings_j
        .as_ref()
        .map(score_nansen_holdings_signal)
        .unwrap_or(0.0);
    let whale_perp_score = whale_perp_j
        .as_ref()
        .map(score_whale_perp_aggregate)
        .unwrap_or(0.0);
    let sm_dex_score = sm_dex_j
        .as_ref()
        .map(score_smart_money_dex_trades)
        .unwrap_or(0.0);
    let perp_screener_score = perp_scr_j
        .as_ref()
        .map(score_perp_screener_aggregate)
        .unwrap_or(0.0);

    let (mut buy_ent, mut sell_ent) = who_j
        .as_ref()
        .map(extract_entity_labels)
        .unwrap_or_default();
    if let Some(dex) = sm_dex_j.as_ref() {
        let (b2, s2) = extract_entity_labels(dex);
        buy_ent.extend(b2);
        sell_ent.extend(s2);
    }

    let mm_buying = has_market_maker_activity(&buy_ent);
    let mm_selling = has_market_maker_activity(&sell_ent);

    AuxData {
        holdings_score,
        whale_perp_score,
        sm_dex_score,
        perp_screener_score,
        buy_entities: buy_ent,
        sell_entities: sell_ent,
        mm_buying,
        mm_selling,
        indicators_json: indicators_j,
    }
}

// -- Majors netflow ---------------------------------------------------------

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

/// Stablecoin flow from netflows: positive = stables leaving exchanges (bullish).
fn stablecoin_exchange_flow(netflows_json: Option<&Value>) -> f64 {
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
        if !(sym.contains("USDT") || sym.contains("USDC") || sym.contains("DAI") || sym.contains("BUSD")) {
            continue;
        }
        let nf = parse_json_f64_opt(row.get("net_flow"))
            .or_else(|| parse_json_f64_opt(row.get("netFlow")))
            .unwrap_or(0.0);
        sum += nf;
    }
    sum
}

// -- Funding ----------------------------------------------------------------

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

// -- Market mode (enhanced with 9 signals) ----------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarketMode {
    Long,
    Short,
    Neutral,
}

fn decide_market_mode(
    majors_nf: f64,
    stablecoin_flow: f64,
    flow_intel_score: f64,
    netflow_score: f64,
    perp_dir: f64,
    funding_avg: Option<f64>,
    aux: &AuxData,
) -> (MarketMode, i32, String, Value) {
    let mut long_votes = 0_i32;
    let mut short_votes = 0_i32;
    let mut reasons = Vec::new();

    // Signal 1: Majors net inflow (weight 2)
    if majors_nf >= MAJORS_INFLOW_USD_LONG {
        long_votes += 2;
        reasons.push(format!("majors_netflow_usd>={MAJORS_INFLOW_USD_LONG:.0} ({majors_nf:.0})"));
    } else if majors_nf <= -MAJORS_OUTFLOW_USD_SHORT {
        short_votes += 2;
        reasons.push(format!("majors_netflow_usd<=-{MAJORS_OUTFLOW_USD_SHORT:.0} ({majors_nf:.0})"));
    }

    // Signal 2: Stablecoin exchange flow — stables OUT of exchanges = bullish
    if stablecoin_flow < -1_000_000.0 {
        long_votes += 1;
        reasons.push(format!("stablecoins_leaving_exchanges ({stablecoin_flow:.0})"));
    } else if stablecoin_flow > 1_000_000.0 {
        short_votes += 1;
        reasons.push(format!("stablecoins_entering_exchanges ({stablecoin_flow:.0})"));
    }

    // Signal 3: Flow intelligence (exchange flows)
    if flow_intel_score > 0.15 {
        long_votes += 1;
        reasons.push("flow_intelligence_accumulation_bias".into());
    } else if flow_intel_score < -0.15 {
        short_votes += 1;
        reasons.push("flow_intelligence_distribution_bias".into());
    }

    // Signal 4: Nansen netflows breadth
    if netflow_score > 0.2 {
        long_votes += 1;
        reasons.push("nansen_netflows_breadth_bullish".into());
    } else if netflow_score < -0.2 {
        short_votes += 1;
        reasons.push("nansen_netflows_breadth_bearish".into());
    }

    // Signal 5: Perp direction (SM long/short tilt)
    if perp_dir > 0.2 {
        long_votes += 1;
        reasons.push("perp_smart_long_tilt".into());
    } else if perp_dir < -0.2 {
        short_votes += 1;
        reasons.push("perp_smart_short_tilt".into());
    }

    // Signal 6: Funding rate
    if let Some(fr) = funding_avg {
        if fr <= 0.000_05 {
            long_votes += 1;
            reasons.push(format!("funding_neutral_or_negative ({fr:.6})"));
        } else if fr >= 0.0003 {
            short_votes += 1;
            reasons.push(format!("funding_positive_crowded_long ({fr:.6})"));
        }
    }

    // Signal 7: Whale perp aggregate
    if aux.whale_perp_score > 0.3 {
        long_votes += 1;
        reasons.push("whales_net_long_perps".into());
    } else if aux.whale_perp_score < -0.3 {
        short_votes += 1;
        reasons.push("whales_net_short_perps".into());
    }

    // Signal 8: SM DEX trades aggression
    if aux.sm_dex_score > 0.25 {
        long_votes += 1;
        reasons.push("smart_money_dex_buying".into());
    } else if aux.sm_dex_score < -0.25 {
        short_votes += 1;
        reasons.push("smart_money_dex_selling".into());
    }

    // Signal 9: Holdings accumulation signal
    if aux.holdings_score > 0.3 {
        long_votes += 1;
        reasons.push("holdings_accumulation_detected".into());
    } else if aux.holdings_score < -0.3 {
        short_votes += 1;
        reasons.push("holdings_distribution_detected".into());
    }

    // Total max votes: 2 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 = 10
    let total_weight = 10_i32;

    let mode = if long_votes >= 5 && long_votes > short_votes {
        MarketMode::Long
    } else if short_votes >= 5 && short_votes > long_votes {
        MarketMode::Short
    } else {
        MarketMode::Neutral
    };

    let max_v = long_votes.max(short_votes);
    let confidence = ((max_v * 100) / total_weight)
        .min(100)
        .max(if mode == MarketMode::Neutral { 35 } else { 45 });

    let key_reason = if reasons.is_empty() {
        "insufficient_data_snapshots".into()
    } else {
        reasons[0].clone()
    };

    let inputs = json!({
        "majors_netflow_usd": majors_nf,
        "stablecoin_exchange_flow": stablecoin_flow,
        "flow_intel_score": flow_intel_score,
        "netflow_score": netflow_score,
        "perp_dir_score": perp_dir,
        "funding_avg_optional": funding_avg,
        "whale_perp_score": aux.whale_perp_score,
        "sm_dex_score": aux.sm_dex_score,
        "holdings_score": aux.holdings_score,
        "perp_screener_score": aux.perp_screener_score,
        "long_votes": long_votes,
        "short_votes": short_votes,
        "total_weight": total_weight,
        "reason_flags": reasons,
        "mm_buying": aux.mm_buying,
        "mm_selling": aux.mm_selling,
    });

    (mode, confidence, key_reason, inputs)
}

// -- Ratio helpers ----------------------------------------------------------

fn buy_ratio(row: &ScreenerRow) -> f64 {
    let t = row.buy_vol + row.sell_vol;
    if t < 1e-9 {
        return 0.5;
    }
    row.buy_vol / t
}

fn buy_to_sell_ratio(row: &ScreenerRow) -> f64 {
    if row.sell_vol < 1e-9 {
        return 999.0;
    }
    row.buy_vol / row.sell_vol
}

fn candidates_from_rows<'a>(
    rows: &[&'a ScreenerRow],
    direction: &'static str,
    tier: &'static str,
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

// -- Pick functions (enhanced) ----------------------------------------------

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
        let sa = a.net_flow + a.buy_vol * 0.0001 + a.fresh_wallets * 1000.0;
        let sb = b.net_flow + b.buy_vol * 0.0001 + b.fresh_wallets * 1000.0;
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
        let sa = -a.net_flow + a.sell_vol * 0.0001 + a.nof_sell_wallets * 1000.0;
        let sb = -b.net_flow + b.sell_vol * 0.0001 + b.nof_sell_wallets * 1000.0;
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(limit);
    v
}

/// Elite long: SM inflow >$500K, buy/sell ≥2.0, multiple wallets, early stage (<10% move), MCap ≤$120M.
fn pick_elite_long<'a>(rows: &'a [ScreenerRow], aux: &AuxData) -> Vec<&'a ScreenerRow> {
    let mut v: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| {
            r.net_flow > ELITE_FLOW_USD
                && buy_to_sell_ratio(r) >= 2.0
                && r.price_change_pct >= -2.0
                && r.price_change_pct <= 10.0
                && r.volume_usd > 0.0
                && r.liquidity_usd >= 500_000.0
                && r.mcap_usd > 0.0
                && r.mcap_usd <= 120_000_000.0
        })
        .collect();
    v.sort_by(|a, b| {
        let bonus_a = if a.nof_buy_wallets >= 3.0 { 100_000.0 } else { 0.0 }
            + if a.volume_change_pct >= 200.0 { 50_000.0 } else { 0.0 }
            + if aux.mm_buying { 200_000.0 } else { 0.0 };
        let bonus_b = if b.nof_buy_wallets >= 3.0 { 100_000.0 } else { 0.0 }
            + if b.volume_change_pct >= 200.0 { 50_000.0 } else { 0.0 }
            + if aux.mm_buying { 200_000.0 } else { 0.0 };
        let sa = a.net_flow + bonus_a;
        let sb = b.net_flow + bonus_b;
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(3);
    v
}

/// Elite short: SM outflow >$500K, retail still buying (buy_ratio>0.55), price pumped (≥3%),
/// MM distributing is a bonus signal.
fn pick_elite_short<'a>(rows: &'a [ScreenerRow], funding_avg: Option<f64>, aux: &AuxData) -> Vec<&'a ScreenerRow> {
    let funding_positive = funding_avg.map_or(false, |f| f >= 0.0003);
    let mut v: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| {
            r.net_flow < -ELITE_FLOW_USD
                && buy_ratio(r) > 0.55
                && r.price_change_pct >= 3.0
                && r.liquidity_usd >= 400_000.0
        })
        .collect();
    v.sort_by(|a, b| {
        let bonus_a = if a.nof_sell_wallets >= 2.0 { 100_000.0 } else { 0.0 }
            + if funding_positive { 50_000.0 } else { 0.0 }
            + if aux.mm_selling { 200_000.0 } else { 0.0 };
        let bonus_b = if b.nof_sell_wallets >= 2.0 { 100_000.0 } else { 0.0 }
            + if funding_positive { 50_000.0 } else { 0.0 }
            + if aux.mm_selling { 200_000.0 } else { 0.0 };
        let sa = -a.net_flow + bonus_a;
        let sb = -b.net_flow + bonus_b;
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(3);
    v
}

/// 10x alert: SM inflow >$100K, fresh wallets, ≥3 SM wallets buying, MCap <$30M, low liq.
fn pick_ten_x(rows: &[ScreenerRow]) -> Vec<&ScreenerRow> {
    let mut v: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| {
            r.net_flow > TEN_X_FLOW_USD
                && r.nof_traders >= 3.0
                && buy_ratio(r) > 0.65
                && r.mcap_usd > 0.0
                && r.mcap_usd < TEN_X_MCAP_MAX
                && r.liquidity_usd >= TEN_X_LIQ_MIN
                && r.liquidity_usd <= TEN_X_LIQ_MAX
                && r.price_change_pct <= 20.0
                && r.net_flow > 0.0
        })
        .collect();
    v.sort_by(|a, b| {
        let bonus_a = if a.fresh_wallets >= 1.0 { 50_000.0 } else { 0.0 }
            + if a.nof_buy_wallets >= 3.0 { 30_000.0 } else { 0.0 }
            + if a.volume_change_pct > 100.0 { 20_000.0 } else { 0.0 };
        let bonus_b = if b.fresh_wallets >= 1.0 { 50_000.0 } else { 0.0 }
            + if b.nof_buy_wallets >= 3.0 { 30_000.0 } else { 0.0 }
            + if b.volume_change_pct > 100.0 { 20_000.0 } else { 0.0 };
        let sa = a.net_flow + bonus_a;
        let sb = b.net_flow + bonus_b;
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(1);
    v
}

/// Explosive setups with direction detection.
fn pick_explosive<'a>(rows: &'a [ScreenerRow], aux: &AuxData) -> Vec<(&'a ScreenerRow, &'static str)> {
    let mut v: Vec<(&ScreenerRow, &'static str)> = rows
        .iter()
        .filter(|r| {
            r.volume_change_pct >= 200.0
                && r.volume_usd >= 1_000_000.0
                && r.liquidity_usd >= 500_000.0
        })
        .map(|r| {
            let dir = if r.net_flow > 0.0 && r.price_change_pct >= 3.0 && r.price_change_pct <= 12.0 {
                "LONG_HIGH_RISK"
            } else if r.price_change_pct >= 20.0 && r.net_flow < 0.0 {
                "SHORT_HIGH_RISK"
            } else if r.net_flow < 0.0 && r.price_change_pct >= 0.0 && aux.whale_perp_score > 0.2 {
                "SQUEEZE_HIGH_RISK"
            } else if r.net_flow > 0.0 {
                "LONG_HIGH_RISK"
            } else {
                "SHORT_HIGH_RISK"
            };
            (r, dir)
        })
        .collect();
    v.sort_by(|a, b| {
        b.0.volume_change_pct
            .partial_cmp(&a.0.volume_change_pct)
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
    v.sort_by(|a, b| {
        let sa = a.net_flow + a.fresh_wallets * 5000.0;
        let sb = b.net_flow + b.fresh_wallets * 5000.0;
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(15);
    v
}

/// Institutional exit: large outflow while price flat/up. Enriched with entity detection.
fn institutional_exit_like<'a>(rows: &'a [ScreenerRow], aux: &AuxData) -> Vec<&'a ScreenerRow> {
    let mut v: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| {
            r.net_flow < -INSTITUTIONAL_FLOW_USD
                && r.price_change_pct >= -2.0
                && r.liquidity_usd >= 500_000.0
                && r.token_age_days >= 7.0
        })
        .collect();
    v.sort_by(|a, b| {
        let bonus_a = if aux.mm_selling { 200_000.0 } else { 0.0 }
            + if a.nof_sell_wallets >= 2.0 { 100_000.0 } else { 0.0 };
        let bonus_b = if aux.mm_selling { 200_000.0 } else { 0.0 }
            + if b.nof_sell_wallets >= 2.0 { 100_000.0 } else { 0.0 };
        let sa = -a.net_flow + bonus_a;
        let sb = -b.net_flow + bonus_b;
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(5);
    v
}

/// Institutional accumulation: large inflow + volume rising. Enriched with entity detection.
fn institutional_accum_like<'a>(rows: &'a [ScreenerRow], aux: &AuxData) -> Vec<&'a ScreenerRow> {
    let mut v: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| {
            r.net_flow > INSTITUTIONAL_FLOW_USD
                && r.price_change_pct < 35.0
                && r.liquidity_usd >= 500_000.0
                && r.volume_change_pct > 0.0
        })
        .collect();
    v.sort_by(|a, b| {
        let bonus_a = if aux.mm_buying { 200_000.0 } else { 0.0 }
            + if a.nof_buy_wallets >= 3.0 { 100_000.0 } else { 0.0 };
        let bonus_b = if aux.mm_buying { 200_000.0 } else { 0.0 }
            + if b.nof_buy_wallets >= 3.0 { 100_000.0 } else { 0.0 };
        let sa = a.net_flow + bonus_a;
        let sb = b.net_flow + bonus_b;
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(5);
    v
}

/// Per-token deep analysis: flow context, narrative, market phase, risk, trade plan.
fn pick_token_analysis(rows: &[ScreenerRow], aux: &AuxData) -> Vec<Value> {
    let mut top: Vec<&ScreenerRow> = rows
        .iter()
        .filter(|r| r.volume_usd >= 100_000.0 && r.liquidity_usd >= 200_000.0)
        .collect();
    top.sort_by(|a, b| {
        b.net_flow
            .abs()
            .partial_cmp(&a.net_flow.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top.truncate(10);

    let mut out = Vec::new();
    for r in top {
        let br = buy_ratio(r);
        let sm_direction = if r.net_flow > 100_000.0 && br > 0.6 {
            "accumulating"
        } else if r.net_flow < -100_000.0 && br < 0.45 {
            "distributing"
        } else {
            "mixed"
        };

        let market_phase = if r.price_change_pct < 5.0 && r.net_flow > 0.0 && r.volume_change_pct > 30.0 {
            "accumulation"
        } else if r.price_change_pct > 15.0 && r.volume_change_pct > 100.0 {
            "breakout"
        } else if r.net_flow < -200_000.0 && r.price_change_pct > 10.0 {
            "distribution"
        } else {
            "consolidation"
        };

        let risk = if r.mcap_usd < 10_000_000.0 || r.liquidity_usd < 500_000.0 {
            "high"
        } else if r.mcap_usd < 100_000_000.0 {
            "medium"
        } else {
            "low"
        };

        let verdict = if sm_direction == "accumulating" && market_phase == "accumulation" {
            "BUY"
        } else if sm_direction == "distributing" {
            "SHORT"
        } else {
            "WAIT"
        };

        let unique_buy_ent = count_unique_entities(&aux.buy_entities);
        let unique_sell_ent = count_unique_entities(&aux.sell_entities);

        out.push(json!({
            "symbol": r.symbol,
            "chain": r.chain,
            "smart_money": {
                "net_flow_usd": r.net_flow,
                "direction": sm_direction,
                "nof_buy_wallets": r.nof_buy_wallets,
                "nof_sell_wallets": r.nof_sell_wallets,
                "fresh_wallets": r.fresh_wallets,
                "buy_ratio": br,
                "unique_buy_entities": unique_buy_ent,
                "unique_sell_entities": unique_sell_ent,
                "mm_buying": aux.mm_buying,
                "mm_selling": aux.mm_selling,
            },
            "flow_context": {
                "volume_usd": r.volume_usd,
                "volume_change_pct": r.volume_change_pct,
                "liquidity_usd": r.liquidity_usd,
            },
            "market_phase": market_phase,
            "risk": risk,
            "verdict": verdict,
            "price_change_pct": r.price_change_pct,
            "mcap_usd": r.mcap_usd,
        }));
    }
    out
}

// -- Notification helpers ---------------------------------------------------

async fn intake_notify_enabled(pool: &PgPool) -> bool {
    resolve_worker_enabled_flag(
        pool,
        "worker",
        "intake_playbook_notify_enabled",
        "QTSS_INTAKE_PLAYBOOK_NOTIFY_ENABLED",
        false,
    )
    .await
}

async fn intake_notify_channels(pool: &PgPool) -> Vec<String> {
    resolve_system_csv(
        pool,
        "worker",
        "intake_playbook_notify_channels",
        "QTSS_INTAKE_PLAYBOOK_NOTIFY_CHANNELS",
        "telegram",
    )
    .await
    .into_iter()
    .map(|s| s.trim().to_lowercase())
    .filter(|s| !s.is_empty())
    .collect()
}

async fn try_enqueue_intake_notification(
    pool: &PgPool,
    notify_on: bool,
    event_key: &'static str,
    title: &str,
    body: &str,
    symbol: Option<&str>,
) {
    if !notify_on {
        return;
    }
    let channels = intake_notify_channels(pool).await;
    if channels.is_empty() {
        warn!("intake notify: intake_playbook_notify_channels empty");
        return;
    }
    let repo = NotifyOutboxRepository::new(pool.clone());
    let sym_meta = symbol.map(|s| s.trim()).filter(|s| !s.is_empty());
    match repo
        .enqueue_with_meta(
            None,
            Some(event_key),
            "info",
            None,
            None,
            sym_meta,
            title,
            body,
            channels,
        )
        .await
    {
        Ok(row) => info!(%event_key, outbox_id = %row.id, "intake notification enqueued"),
        Err(e) => warn!(%e, %event_key, "intake notification enqueue failed"),
    }
}

// -- Persist ----------------------------------------------------------------

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

// -- All data_snapshots keys consumed by this engine ------------------------

const ALL_DATA_KEYS: &[&str] = &[
    "nansen_token_screener",
    "nansen_netflows",
    "nansen_flow_intelligence",
    "nansen_perp_trades",
    "nansen_holdings",
    "nansen_who_bought_sold",
    "nansen_smart_money_dex_trades",
    "nansen_whale_perp_aggregate",
    "nansen_perp_screener",
    "nansen_tgm_indicators",
    "binance_premium_btcusdt",
    "binance_premium_ethusdt",
];

// -- Main sweep (all playbooks) ---------------------------------------------

async fn run_sweep(pool: &PgPool) -> Result<(), qtss_storage::StorageError> {
    let notify_on = intake_notify_enabled(pool).await;

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

    let aux = load_aux_data(pool).await;

    let majors_nf = majors_netflow_usd(nf_j.as_ref());
    let stable_flow = stablecoin_exchange_flow(nf_j.as_ref());
    let flow_intel = fi_j.as_ref().map(score_coinglass_netflow_like).unwrap_or(0.0);
    let netflow_score = nf_j.as_ref().map(score_nansen_netflows).unwrap_or(0.0);
    let perp_dir = perp_j.as_ref().map(score_nansen_perp_direction).unwrap_or(0.0);
    let funding_avg = avg_btc_eth_funding_async(pool).await;

    // === Playbook 1: Market Mode ===
    let (mode, conf, key_reason, mode_inputs) =
        decide_market_mode(majors_nf, stable_flow, flow_intel, netflow_score, perp_dir, funding_avg, &aux);

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
        "stablecoin_exchange_flow_usd": stable_flow,
        "whale_perp_exposure": aux.whale_perp_score,
        "mm_buying": aux.mm_buying,
        "mm_selling": aux.mm_selling,
        "note": "Enhanced 9-signal heuristic from data_snapshots; confirm with Nansen UI / LLM.",
    });

    let prev_market = fetch_latest_intake_playbook_run(pool, PLAYBOOK_MARKET_MODE).await?;
    let prev_mode = prev_market.as_ref().and_then(|r| r.market_mode.as_deref());

    persist_playbook(
        pool,
        PLAYBOOK_MARKET_MODE,
        Some(mode_str),
        conf,
        &key_reason,
        neutral_guidance,
        mode_summary,
        mode_inputs.clone(),
        json!({ "data_keys_checked": ALL_DATA_KEYS }),
        mode_candidates,
    )
    .await?;

    if prev_mode != Some(mode_str) {
        let title = format!("Intake: market mode -> {mode_str}");
        let body = format!(
            "Previous: {}\nConfidence: {}%\nReason: {}\nStablecoin flow: {stable_flow:.0}\nWhale perp: {:.2}\nMM buy: {} / sell: {}",
            prev_mode.unwrap_or("(none)"),
            conf,
            key_reason,
            aux.whale_perp_score,
            aux.mm_buying,
            aux.mm_selling,
        );
        try_enqueue_intake_notification(pool, notify_on, "intake_market_mode", &title, &body, None).await;
    }

    // === Playbook 2: Elite Short (pump->distribution->dump) ===
    let elite_s = pick_elite_short(&rows, funding_avg, &aux);
    let elite_s_summary = json!({
        "goal": "pump_distribution_dump",
        "horizon_hours": [1, 4],
        "target_pct": [-20, -30],
        "stop_loss_pct": 5,
        "funding_positive": funding_avg.map_or(false, |f| f >= 0.0003),
        "mm_distributing": aux.mm_selling,
        "sell_entity_count": count_unique_entities(&aux.sell_entities),
    });
    persist_playbook(
        pool,
        PLAYBOOK_ELITE_SHORT,
        None,
        50,
        "elite_short_enhanced",
        None,
        elite_s_summary,
        mode_inputs.clone(),
        json!({ "strict_usd": ELITE_FLOW_USD }),
        candidates_from_rows(&elite_s, "SHORT", "apex", 48),
    )
    .await?;

    // === Playbook 3: Elite Long (pre-pump breakout) ===
    let elite_l = pick_elite_long(&rows, &aux);
    let elite_l_summary = json!({
        "goal": "pre_pump_breakout",
        "horizon_hours": [1, 6],
        "target_pct": [30],
        "stop_loss_pct": -10,
        "mm_accumulating": aux.mm_buying,
        "buy_entity_count": count_unique_entities(&aux.buy_entities),
    });
    persist_playbook(
        pool,
        PLAYBOOK_ELITE_LONG,
        None,
        50,
        "elite_long_enhanced",
        None,
        elite_l_summary,
        mode_inputs.clone(),
        json!({ "strict_usd": ELITE_FLOW_USD }),
        candidates_from_rows(&elite_l, "LONG", "apex", 50),
    )
    .await?;

    // === Playbook 4: 10x Alert ===
    let ten = pick_ten_x(&rows);
    let triggered = !ten.is_empty();
    let ten_summary = json!({
        "triggered": triggered,
        "tp_pct_tiers": [25, 50, 100],
        "sl_pct_range": [-10, -15],
        "requires_fresh_wallets": true,
        "requires_volume_confirmation": true,
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

    if triggered {
        if let Some(r) = ten.first() {
            let sym_key = r.symbol.trim().to_uppercase();
            let dedupe_secs = resolve_worker_tick_secs(
                pool,
                "worker",
                "intake_playbook_notify_ten_x_dedupe_secs",
                "QTSS_INTAKE_PLAYBOOK_NOTIFY_TEN_X_DEDUPE_SECS",
                86_400,
                0,
            )
            .await;
            let outbox = NotifyOutboxRepository::new(pool.clone());
            let within_dedupe_window = dedupe_secs > 0
                && outbox
                    .exists_recent_global_event_symbol(
                        "intake_ten_x_alert",
                        &sym_key,
                        dedupe_secs.min(i64::MAX as u64) as i64,
                    )
                    .await
                    .unwrap_or(false);
            if within_dedupe_window {
                debug!(symbol = %sym_key, secs = dedupe_secs, "intake ten_x notify skipped (dedupe)");
            } else {
                let title = format!("10x Alert: {}", r.symbol);
                let body = format!(
                    "Symbol: {}\nNet flow: ${:.0}\nMCap: ${:.0}\nLiquidity: ${:.0}\nFresh wallets: {}\nBuy wallets: {}\nPrice: {:.2}%\nBuy ratio: {:.2}",
                    r.symbol, r.net_flow, r.mcap_usd, r.liquidity_usd,
                    r.fresh_wallets, r.nof_buy_wallets, r.price_change_pct, buy_ratio(r),
                );
                try_enqueue_intake_notification(
                    pool,
                    notify_on,
                    "intake_ten_x_alert",
                    &title,
                    &body,
                    Some(sym_key.as_str()),
                )
                .await;
            }
        }
    }

    // === Playbook 5: Institutional Exit (entity-enriched) ===
    let ex = institutional_exit_like(&rows, &aux);
    let ex_summary = json!({
        "direction": "SHORT",
        "target_pct": [-15, -25],
        "stop_loss_pct": 5,
        "mm_distributing": aux.mm_selling,
        "known_sell_entities": count_unique_entities(&aux.sell_entities),
        "note": "Entity labels from nansen_who_bought_sold + smart_money_dex_trades",
    });
    persist_playbook(
        pool,
        PLAYBOOK_INSTITUTIONAL_EXIT,
        None,
        45,
        "institutional_exit_entity_enriched",
        None,
        ex_summary,
        mode_inputs.clone(),
        json!({}),
        candidates_from_rows(&ex, "SHORT", "scan", 42),
    )
    .await?;

    // === Playbook 6: Institutional Accumulation (entity-enriched) ===
    let acc = institutional_accum_like(&rows, &aux);
    let acc_summary = json!({
        "direction": "LONG",
        "target_pct": [15, 30],
        "stop_loss_pct": -8,
        "mm_accumulating": aux.mm_buying,
        "known_buy_entities": count_unique_entities(&aux.buy_entities),
    });
    persist_playbook(
        pool,
        PLAYBOOK_INSTITUTIONAL_ACCUM,
        None,
        45,
        "institutional_accumulation_entity_enriched",
        None,
        acc_summary,
        mode_inputs.clone(),
        json!({}),
        candidates_from_rows(&acc, "LONG", "scan", 42),
    )
    .await?;

    // === Playbook 7: Explosive with direction detection ===
    let exp = pick_explosive(&rows, &aux);
    let exp_directions: Vec<&str> = exp.iter().map(|(_, d)| *d).collect();
    let exp_summary = json!({
        "min_volume_change_pct": 200,
        "min_volume_usd": 1_000_000,
        "detected_directions": exp_directions,
        "whale_perp_context": aux.whale_perp_score,
    });
    let exp_cands: Vec<IntakePlaybookCandidateInsert<'_>> = exp
        .iter()
        .enumerate()
        .map(|(i, (r, dir))| IntakePlaybookCandidateInsert {
            rank: (i + 1) as i32,
            symbol: r.symbol.as_str(),
            chain: r.chain.as_deref(),
            direction: dir,
            intake_tier: "apex",
            confidence_0_100: (45 + (30 - (i as i32 * 5))).clamp(20, 92),
            detail_json: &r.raw,
        })
        .collect();
    persist_playbook(
        pool,
        PLAYBOOK_EXPLOSIVE,
        None,
        48,
        "volume_spike_direction_detected",
        None,
        exp_summary,
        mode_inputs.clone(),
        json!({}),
        exp_cands,
    )
    .await?;

    // === Playbook 8: Early Accumulation ===
    let early = pick_early_accumulation(&rows);
    let early_summary = json!({
        "horizon_hours": [6, 24],
        "criteria": "flat_price_rising_flow_fresh_wallets",
    });
    persist_playbook(
        pool,
        PLAYBOOK_EARLY_ACCUM,
        None,
        44,
        "early_accumulation_flat_price_rising_flow",
        None,
        early_summary,
        mode_inputs.clone(),
        json!({}),
        candidates_from_rows(&early, "LONG", "scan", 40),
    )
    .await?;

    // === Playbook 9: Token Analysis (deep per-token verdict) ===
    let analyses = pick_token_analysis(&rows, &aux);
    let analysis_summary = json!({
        "count": analyses.len(),
        "tokens": analyses,
    });
    persist_playbook(
        pool,
        PLAYBOOK_TOKEN_ANALYSIS,
        None,
        50,
        "per_token_flow_analysis",
        None,
        analysis_summary,
        mode_inputs,
        json!({ "data_keys": ALL_DATA_KEYS }),
        vec![],
    )
    .await?;

    Ok(())
}

// -- Worker loop ------------------------------------------------------------

pub async fn intake_playbook_loop(pool: PgPool) {
    info!("intake_playbook_engine: enhanced loop started");
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
                Ok(()) => info!("intake_playbook sweep ok (9 playbooks)"),
                Err(e) => warn!(%e, "intake_playbook sweep failed"),
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(tick)).await;
    }
}

// -- Tests ------------------------------------------------------------------

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
    fn stablecoin_flow_tracks_usdt_usdc() {
        let j = json!({
            "data": [
                { "symbol": "USDT", "net_flow": -500_000.0 },
                { "symbol": "USDC", "net_flow": -300_000.0 },
                { "symbol": "ETH", "net_flow": 5e6 },
            ]
        });
        let flow = stablecoin_exchange_flow(Some(&j));
        assert!((flow - (-800_000.0)).abs() < 1.0, "expected -800K got {flow}");
    }

    #[test]
    fn decide_long_when_strong_signals() {
        let aux = AuxData {
            holdings_score: 0.5,
            whale_perp_score: 0.5,
            sm_dex_score: 0.4,
            perp_screener_score: 0.0,
            buy_entities: vec![],
            sell_entities: vec![],
            mm_buying: false,
            mm_selling: false,
            indicators_json: None,
        };
        let (m, conf, _, _) = decide_market_mode(11e6, -2e6, 0.2, 0.3, 0.25, Some(-0.0001), &aux);
        assert_eq!(m, MarketMode::Long);
        assert!(conf >= 50, "confidence {conf}");
    }

    #[test]
    fn decide_short_when_distribution() {
        let aux = AuxData {
            holdings_score: -0.5,
            whale_perp_score: -0.5,
            sm_dex_score: -0.4,
            perp_screener_score: 0.0,
            buy_entities: vec![],
            sell_entities: vec![],
            mm_buying: false,
            mm_selling: true,
            indicators_json: None,
        };
        let (m, _, _, _) = decide_market_mode(-11e6, 2e6, -0.2, -0.3, -0.25, Some(0.0005), &aux);
        assert_eq!(m, MarketMode::Short);
    }

    #[test]
    fn screener_row_parses_new_fields() {
        let j = json!({
            "data": [{
                "token_symbol": "PEPE",
                "net_flow": 150_000.0,
                "buy_volume": 100.0,
                "sell_volume": 50.0,
                "volume": 500_000.0,
                "liquidity": 1_000_000.0,
                "market_cap": 5_000_000.0,
                "fresh_wallets": 12,
                "nof_buy_wallets": 8,
                "nof_sell_wallets": 3,
                "token_address": "0xabc123",
                "volume_change_pct": 250.0,
            }]
        });
        let rows = screener_rows(&j);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fresh_wallets, 12.0);
        assert_eq!(rows[0].nof_buy_wallets, 8.0);
        assert_eq!(rows[0].token_address.as_deref(), Some("0xabc123"));
    }

    #[test]
    fn pick_ten_x_prefers_fresh_wallets() {
        let base = || ScreenerRow {
            symbol: "X".into(),
            chain: None,
            token_address: None,
            net_flow: 200_000.0,
            buy_vol: 80.0,
            sell_vol: 20.0,
            price_change_pct: 5.0,
            volume_usd: 100_000.0,
            liquidity_usd: 1_000_000.0,
            mcap_usd: 10_000_000.0,
            nof_traders: 5.0,
            token_age_days: 30.0,
            volume_change_pct: 150.0,
            fresh_wallets: 0.0,
            nof_buy_wallets: 4.0,
            nof_sell_wallets: 1.0,
            raw: json!({}),
        };
        let mut a = base();
        a.symbol = "A".into();
        a.fresh_wallets = 5.0;
        a.net_flow = 150_000.0;
        let mut b = base();
        b.symbol = "B".into();
        b.fresh_wallets = 0.0;
        b.net_flow = 200_000.0;
        let rows = vec![a, b];
        let picks = pick_ten_x(&rows);
        assert_eq!(picks.len(), 1);
        assert_eq!(picks[0].symbol, "A");
    }
}

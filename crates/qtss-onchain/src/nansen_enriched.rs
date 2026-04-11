//! Enriched Nansen analyzers — derive deeper signals from raw Nansen
//! snapshot data stored in `data_snapshots`.
//!
//! Three signal types:
//! 1. **Cross-chain flow** — same token moving on multiple chains
//! 2. **DEX volume spike** — smart-money aggressive buy/sell burst
//! 3. **Whale concentration** — top-N wallet balance shift
//!
//! Each analyzer reads the same snapshots that [`super::nansen`] reads,
//! produces an [`EnrichedSignal`], and optionally fires a notification.
//! Results are persisted to `nansen_enriched_signals` and folded into
//! the chain pillar blend.

use serde_json::{json, Value};
use std::collections::HashMap;

/// Output of an enriched analyzer.
#[derive(Debug, Clone)]
pub struct EnrichedSignal {
    pub signal_type: &'static str,
    pub score: f64,
    pub direction: &'static str,
    pub confidence: f64,
    pub chain_breakdown: Option<Value>,
    pub details: Option<Value>,
}

/// Config for enriched analyzers, read from `system_config`.
#[derive(Debug, Clone)]
pub struct EnrichedConfig {
    pub enabled: bool,
    pub cross_chain_min_chains: usize,
    pub cross_chain_agreement_boost: f64,
    pub dex_spike_threshold_x: f64,
    pub dex_spike_min_value_usd: f64,
    pub whale_top_n: usize,
    pub whale_delta_threshold: f64,
    pub w_cross_chain: f64,
    pub w_dex_spike: f64,
    pub w_whale_conc: f64,
}

impl Default for EnrichedConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cross_chain_min_chains: 2,
            cross_chain_agreement_boost: 0.3,
            dex_spike_threshold_x: 3.0,
            dex_spike_min_value_usd: 50_000.0,
            whale_top_n: 10,
            whale_delta_threshold: 0.05,
            w_cross_chain: 0.15,
            w_dex_spike: 0.10,
            w_whale_conc: 0.10,
        }
    }
}

/// Resolved chain-level key for one symbol across multiple chains.
#[derive(Debug, Clone)]
pub struct MultiChainKey {
    pub engine_symbol: String,
    pub chains: Vec<ChainEntry>,
}

#[derive(Debug, Clone)]
pub struct ChainEntry {
    pub chain: String,
    pub address: Option<String>,
    pub symbol: Option<String>,
}

/// Parse the expanded symbol_map format. Supports both:
/// - Old: `{ "BTCUSDT": { "chain": "ethereum", "address": "0x...", "symbol": "WBTC" } }`
/// - New: `{ "BTCUSDT": { "chains": { "ethereum": { "address": "0x...", "symbol": "WBTC" }, "bnb": { ... } } } }`
pub fn parse_multi_chain_keys(symbol_map: &Value, engine_symbol: &str) -> Option<MultiChainKey> {
    let stripped = engine_symbol
        .strip_suffix("USDT")
        .or_else(|| engine_symbol.strip_suffix("USD"))
        .or_else(|| engine_symbol.strip_suffix("BUSD"))
        .unwrap_or(engine_symbol);

    let entry = symbol_map
        .get(engine_symbol)
        .or_else(|| symbol_map.get(stripped))?;

    if !entry.is_object() {
        return None;
    }

    // New multi-chain format
    if let Some(chains_obj) = entry.get("chains").and_then(|v| v.as_object()) {
        let chains: Vec<ChainEntry> = chains_obj
            .iter()
            .filter_map(|(chain, v)| {
                let address = v.get("address").and_then(|a| a.as_str()).map(String::from);
                let symbol = v.get("symbol").and_then(|s| s.as_str()).map(String::from);
                if address.is_none() && symbol.is_none() {
                    return None;
                }
                Some(ChainEntry {
                    chain: chain.clone(),
                    address,
                    symbol,
                })
            })
            .collect();
        if chains.is_empty() {
            return None;
        }
        return Some(MultiChainKey {
            engine_symbol: engine_symbol.to_string(),
            chains,
        });
    }

    // Old single-chain format — wrap into multi-chain
    let chain = entry.get("chain").and_then(|v| v.as_str()).map(String::from);
    let address = entry
        .get("address")
        .and_then(|v| v.as_str())
        .map(String::from);
    let symbol = entry
        .get("symbol")
        .and_then(|v| v.as_str())
        .map(String::from);
    if address.is_none() && symbol.is_none() {
        return None;
    }
    Some(MultiChainKey {
        engine_symbol: engine_symbol.to_string(),
        chains: vec![ChainEntry {
            chain: chain.unwrap_or_else(|| "unknown".to_string()),
            address,
            symbol,
        }],
    })
}

fn json_f64(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn rows_of(resp: &Value) -> Vec<&Value> {
    match resp.get("data") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(other) => vec![other],
        None => Vec::new(),
    }
}

fn row_matches_chain_entry(row: &Value, entry: &ChainEntry) -> bool {
    // Check chain match first (if row has chain field)
    if let Some(row_chain) = row
        .get("chain")
        .or_else(|| row.get("blockchain"))
        .and_then(|v| v.as_str())
    {
        if !entry.chain.eq_ignore_ascii_case(row_chain) && entry.chain != "unknown" {
            return false;
        }
    }

    // Address match
    let addr_field = row
        .get("token_address")
        .or_else(|| row.get("tokenAddress"))
        .or_else(|| row.get("address"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_lowercase());
    if let (Some(want), Some(got)) = (
        entry.address.as_deref().map(str::to_ascii_lowercase),
        addr_field,
    ) {
        if want == got {
            return true;
        }
    }

    // Symbol match
    let sym_field = row
        .get("token_symbol")
        .or_else(|| row.get("symbol"))
        .or_else(|| row.get("token"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_uppercase());
    if let (Some(want), Some(got)) = (
        entry.symbol.as_deref().map(str::to_ascii_uppercase),
        sym_field,
    ) {
        if want == got {
            return true;
        }
    }

    false
}

fn sum_netflow_for_entry(rows: &[&Value], entry: &ChainEntry) -> (f64, f64, u32) {
    let mut net = 0.0_f64;
    let mut abs = 0.0_f64;
    let mut hits = 0u32;
    for row in rows {
        if row_matches_chain_entry(row, entry) {
            let v = json_f64(row.get("net_flow_24h_usd"))
                .or_else(|| json_f64(row.get("net_flow")))
                .or_else(|| json_f64(row.get("netFlow")))
                .or_else(|| json_f64(row.get("net_volume")))
                .unwrap_or(0.0);
            net += v;
            abs += v.abs();
            hits += 1;
        }
    }
    (net, abs, hits)
}

// ── Analyzer 1: Cross-Chain Flow ───────────────────────────────────

/// For each chain in the key, compute netflow direction. When multiple
/// chains agree → stronger signal.
pub fn analyze_cross_chain_flow(
    netflow_resp: Option<&Value>,
    key: &MultiChainKey,
    cfg: &EnrichedConfig,
) -> Option<EnrichedSignal> {
    let resp = netflow_resp?;
    let all_rows = rows_of(resp);
    if all_rows.is_empty() {
        return None;
    }

    let mut chain_scores: HashMap<String, f64> = HashMap::new();
    let mut method = "token_specific";

    // ── Phase 1: try token-specific match ──────────────────────────
    if key.chains.len() >= 2 {
        for entry in &key.chains {
            let (net, abs, hits) = sum_netflow_for_entry(&all_rows, entry);
            if hits > 0 && abs > 1e-12 {
                chain_scores.insert(entry.chain.clone(), (net / abs).clamp(-1.0, 1.0));
            }
        }
    }
    if chain_scores.is_empty() {
        // Auto-discover: same symbol/address across any chain
        let ref_entry = &key.chains[0];
        for row in &all_rows {
            let row_chain = row
                .get("chain")
                .or_else(|| row.get("blockchain"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let sym_match = match (
                ref_entry.symbol.as_deref().map(str::to_ascii_uppercase),
                row.get("token_symbol")
                    .or_else(|| row.get("symbol"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_ascii_uppercase()),
            ) {
                (Some(want), Some(got)) => want == got,
                _ => false,
            };
            let addr_match = match (
                ref_entry.address.as_deref().map(str::to_ascii_lowercase),
                row.get("token_address")
                    .or_else(|| row.get("address"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_ascii_lowercase()),
            ) {
                (Some(want), Some(got)) => want == got,
                _ => false,
            };
            if !sym_match && !addr_match {
                continue;
            }
            let v = netflow_value(row);
            let e = chain_scores.entry(row_chain.to_string()).or_insert(0.0);
            *e += v;
        }
        if !chain_scores.is_empty() {
            normalize_scores(&mut chain_scores);
        }
    }

    // ── Phase 2: fallback — chain-aggregate sentiment ──────────────
    // Token not found or found on fewer chains than min_chains.
    // Aggregate ALL smart-money netflows per chain. Gives "ethereum
    // chain net sentiment = inflow/outflow" rather than nothing.
    if chain_scores.len() < cfg.cross_chain_min_chains {
        chain_scores.clear();
        method = "chain_aggregate";
        let target_chains: Vec<String> = key
            .chains
            .iter()
            .map(|c| c.chain.to_ascii_lowercase())
            .collect();

        // Also include well-known chains so we can still detect
        // cross-chain agreement even for single-chain symbols.
        let scan_chains = if target_chains.len() < 2 {
            vec![
                "ethereum".to_string(),
                "solana".to_string(),
                "bnb".to_string(),
                "arbitrum".to_string(),
                "base".to_string(),
            ]
        } else {
            target_chains
        };

        for row in &all_rows {
            let row_chain = row
                .get("chain")
                .or_else(|| row.get("blockchain"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            if !scan_chains.iter().any(|c| *c == row_chain) {
                continue;
            }
            let v = netflow_value(row);
            let e = chain_scores.entry(row_chain).or_insert(0.0);
            *e += v;
        }
        normalize_scores(&mut chain_scores);
    }

    if chain_scores.len() < cfg.cross_chain_min_chains {
        return None;
    }

    // ── Consensus detection ────────────────────────────────────────
    let bullish = chain_scores.values().filter(|s| **s > 0.05).count();
    let bearish = chain_scores.values().filter(|s| **s < -0.05).count();
    let neutral = chain_scores.len() - bullish - bearish;
    let total = chain_scores.len();

    let (agreement_ratio, dominant_dir) = if bullish > bearish && bullish > neutral {
        (bullish as f64 / total as f64, "long")
    } else if bearish > bullish && bearish > neutral {
        (bearish as f64 / total as f64, "short")
    } else {
        // Mixed / neutral — still produce a low-confidence signal
        let avg: f64 = chain_scores.values().sum::<f64>() / total as f64;
        let dir = if avg > 0.01 { "long" } else if avg < -0.01 { "short" } else { "neutral" };
        let ratio = (bullish.max(bearish)) as f64 / total as f64;
        (ratio, dir)
    };

    let avg_score: f64 = chain_scores.values().sum::<f64>() / chain_scores.len() as f64;
    // Chain-aggregate has lower confidence than token-specific.
    // Scale confidence by agreement_ratio — mixed signals = low confidence.
    let base_conf = if method == "chain_aggregate" { 0.2 } else { 0.4 };
    let agreement_mult = agreement_ratio.clamp(0.3, 1.0);
    let confidence_boost = if agreement_ratio >= 0.75 {
        cfg.cross_chain_agreement_boost
    } else {
        cfg.cross_chain_agreement_boost * agreement_ratio
    };
    let confidence = ((base_conf + confidence_boost) * agreement_mult).clamp(0.1, 1.0);

    let breakdown: Value = chain_scores
        .iter()
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect::<serde_json::Map<String, Value>>()
        .into();

    Some(EnrichedSignal {
        signal_type: "cross_chain_flow",
        score: avg_score.clamp(-1.0, 1.0),
        direction: dominant_dir,
        confidence,
        chain_breakdown: Some(breakdown),
        details: Some(json!({
            "method": method,
            "chains_agreeing": if dominant_dir == "long" { bullish } else { bearish },
            "chains_total": total,
            "agreement_ratio": agreement_ratio,
        })),
    })
}

fn netflow_value(row: &Value) -> f64 {
    json_f64(row.get("net_flow_24h_usd"))
        .or_else(|| json_f64(row.get("net_flow")))
        .or_else(|| json_f64(row.get("netFlow")))
        .or_else(|| json_f64(row.get("net_volume")))
        .unwrap_or(0.0)
}

fn normalize_scores(scores: &mut HashMap<String, f64>) {
    let max_abs = scores.values().map(|v| v.abs()).fold(0.0_f64, f64::max);
    if max_abs > 1e-12 {
        for v in scores.values_mut() {
            *v = (*v / max_abs).clamp(-1.0, 1.0);
        }
    }
}

// ── Analyzer 2: DEX Volume Spike ───────────────────────────────────

/// Detect sudden buy/sell volume spike from smart-money DEX trades.
pub fn analyze_dex_volume_spike(
    dex_resp: Option<&Value>,
    key: &MultiChainKey,
    cfg: &EnrichedConfig,
    prev_volume: Option<f64>,
) -> Option<EnrichedSignal> {
    let resp = dex_resp?;
    let all_rows = rows_of(resp);

    // ── Phase 1: token-specific match ──────────────────────────────
    let (mut buy_vol, mut sell_vol, mut method) = (0.0_f64, 0.0_f64, "token_specific");
    for row in &all_rows {
        let matches = key.chains.iter().any(|e| row_matches_chain_entry(row, e));
        if !matches {
            continue;
        }
        accumulate_dex_row(row, &mut buy_vol, &mut sell_vol);
    }

    // ── Phase 2: fallback — chain-aggregate DEX activity ───────────
    // Token not in snapshot → aggregate ALL smart-money DEX trades on
    // the chains this symbol lives on.
    if (buy_vol + sell_vol) < 1.0 {
        method = "chain_aggregate";
        buy_vol = 0.0;
        sell_vol = 0.0;
        let target_chains: Vec<String> = key
            .chains
            .iter()
            .map(|c| c.chain.to_ascii_lowercase())
            .collect();
        for row in &all_rows {
            let row_chain = row
                .get("chain")
                .or_else(|| row.get("blockchain"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            if !target_chains.iter().any(|c| *c == row_chain) {
                continue;
            }
            accumulate_dex_row(row, &mut buy_vol, &mut sell_vol);
        }
    }

    let total_vol = buy_vol + sell_vol;
    if total_vol < 1.0 {
        return None;
    }

    let is_spike = if let Some(baseline) = prev_volume {
        baseline > 0.0 && total_vol > baseline * cfg.dex_spike_threshold_x
    } else {
        false
    };

    let direction_score = ((buy_vol - sell_vol) / total_vol).clamp(-1.0, 1.0);
    let direction = if direction_score > 0.1 {
        "long"
    } else if direction_score < -0.1 {
        "short"
    } else {
        "neutral"
    };

    // Chain-aggregate has lower base confidence
    let base_conf = if method == "chain_aggregate" { 0.15 } else { 0.4 };
    let confidence = if is_spike {
        0.85
    } else if total_vol >= cfg.dex_spike_min_value_usd {
        base_conf + 0.2
    } else {
        base_conf
    };

    Some(EnrichedSignal {
        signal_type: "dex_volume_spike",
        score: direction_score,
        direction,
        confidence,
        chain_breakdown: None,
        details: Some(json!({
            "method": method,
            "buy_volume_usd": buy_vol,
            "sell_volume_usd": sell_vol,
            "total_volume_usd": total_vol,
            "baseline_volume_usd": prev_volume,
            "is_spike": is_spike,
            "spike_ratio": prev_volume.map(|b| if b > 0.0 { total_vol / b } else { 0.0 }),
        })),
    })
}

fn accumulate_dex_row(row: &Value, buy: &mut f64, sell: &mut f64) {
    let value = json_f64(row.get("trade_value_usd"))
        .or_else(|| json_f64(row.get("value_usd")))
        .or_else(|| json_f64(row.get("usd_value")))
        .unwrap_or(0.0)
        .max(0.0);
    if value <= 0.0 {
        return;
    }

    // Format 1: explicit action/side field
    let action = row
        .get("action")
        .or_else(|| row.get("side"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if action.contains("buy") {
        *buy += value;
        return;
    }
    if action.contains("sell") {
        *sell += value;
        return;
    }

    // Format 2: swap format (token_bought/token_sold).
    // Stablecoin sold → buying crypto (buy), stablecoin bought → selling crypto (sell).
    let sold_sym = row
        .get("token_sold_symbol")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    let bought_sym = row
        .get("token_bought_symbol")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    let is_stable = |s: &str| matches!(s, "USDC" | "USDT" | "DAI" | "BUSD" | "TUSD" | "USDP" | "FDUSD");

    if is_stable(&sold_sym) && !is_stable(&bought_sym) {
        // Sold stablecoin, bought token → net buy
        *buy += value;
    } else if !is_stable(&sold_sym) && is_stable(&bought_sym) {
        // Sold token, bought stablecoin → net sell
        *sell += value;
    }
    // Both non-stable (token-to-token swap): skip — direction unclear
}

// ── Analyzer 3: Whale Concentration ────────────────────────────────

/// Track top-N wallet balance changes. Large concentration shifts →
/// whale accumulation/distribution signal.
pub fn analyze_whale_concentration(
    holdings_resp: Option<&Value>,
    key: &MultiChainKey,
    cfg: &EnrichedConfig,
    prev_concentration: Option<f64>,
) -> Option<EnrichedSignal> {
    let resp = holdings_resp?;
    let all_rows = rows_of(resp);

    // Collect balance change % for matching rows
    let mut changes: Vec<f64> = Vec::new();
    for row in &all_rows {
        let matches = key.chains.iter().any(|e| row_matches_chain_entry(row, e));
        if !matches {
            continue;
        }
        if let Some(pct) = json_f64(row.get("balance_24h_percent_change"))
            .or_else(|| json_f64(row.get("balance_change_24h_pct")))
            .or_else(|| json_f64(row.get("balance_change_pct_24h")))
            .or_else(|| json_f64(row.get("balance_change_24h")))
        {
            changes.push(pct);
        }
    }

    if changes.is_empty() {
        return None;
    }

    // Sort by magnitude, take top-N
    changes.sort_by(|a, b| b.abs().partial_cmp(&a.abs()).unwrap_or(std::cmp::Ordering::Equal));
    let top_n = changes.iter().take(cfg.whale_top_n).copied().collect::<Vec<_>>();

    // Average concentration change
    let avg_change = top_n.iter().sum::<f64>() / top_n.len() as f64;

    // Detect significant delta from previous
    let delta = prev_concentration
        .map(|prev| (avg_change - prev).abs())
        .unwrap_or(avg_change.abs());

    let is_significant = delta >= cfg.whale_delta_threshold;

    let score = (avg_change / 10.0).clamp(-1.0, 1.0); // ±10% = saturation
    let direction = if avg_change > cfg.whale_delta_threshold {
        "long" // whales accumulating
    } else if avg_change < -cfg.whale_delta_threshold {
        "short" // whales distributing
    } else {
        "neutral"
    };

    let confidence = if is_significant {
        (delta / cfg.whale_delta_threshold * 0.3).clamp(0.3, 0.9)
    } else {
        0.15 // low-confidence baseline reading
    };

    Some(EnrichedSignal {
        signal_type: "whale_concentration",
        score,
        direction,
        confidence,
        chain_breakdown: None,
        details: Some(json!({
            "top_n_avg_change_pct": avg_change,
            "top_n_count": top_n.len(),
            "delta_from_prev": delta,
            "prev_concentration": prev_concentration,
        })),
    })
}

// ── Blend enriched signals into a composite score ──────────────────

/// Combine enriched signals into a single score [-1, +1] with
/// confidence, ready to fold into the chain pillar.
pub fn blend_enriched(
    signals: &[EnrichedSignal],
    cfg: &EnrichedConfig,
) -> Option<(f64, f64)> {
    if signals.is_empty() {
        return None;
    }

    let mut weighted_sum = 0.0_f64;
    let mut weight_sum = 0.0_f64;
    let mut conf_sum = 0.0_f64;

    for s in signals {
        let w = match s.signal_type {
            "cross_chain_flow" => cfg.w_cross_chain,
            "dex_volume_spike" => cfg.w_dex_spike,
            "whale_concentration" => cfg.w_whale_conc,
            _ => 0.0,
        };
        let ew = w * s.confidence;
        if ew <= 0.0 {
            continue;
        }
        weighted_sum += s.score * ew;
        weight_sum += ew;
        conf_sum += s.confidence * w;
    }

    if weight_sum < 1e-12 {
        return None;
    }

    let score = (weighted_sum / weight_sum).clamp(-1.0, 1.0);
    let max_w = cfg.w_cross_chain + cfg.w_dex_spike + cfg.w_whale_conc;
    let confidence = if max_w > 0.0 {
        (conf_sum / max_w).clamp(0.0, 1.0)
    } else {
        0.0
    };

    Some((score, confidence))
}

// ── Raw flow extraction helpers ────────────────────────────────────

/// Extract structured rows from a Nansen API response for persistence
/// to `nansen_raw_flows`. Returns (source_type, chain, token_symbol,
/// token_address, direction, value_usd, balance_pct, raw_row) tuples.
pub struct RawFlowRow {
    pub source_type: String,
    pub chain: Option<String>,
    pub token_symbol: Option<String>,
    pub token_address: Option<String>,
    pub direction: Option<String>,
    pub value_usd: Option<f64>,
    pub balance_pct_change: Option<f64>,
    pub raw_row: Value,
}

pub fn extract_raw_flow_rows(source_type: &str, resp: &Value) -> Vec<RawFlowRow> {
    let all_rows = rows_of(resp);
    let mut out = Vec::with_capacity(all_rows.len());

    for row in all_rows {
        let chain = row
            .get("chain")
            .or_else(|| row.get("blockchain"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let token_symbol = row
            .get("token_symbol")
            .or_else(|| row.get("symbol"))
            .or_else(|| row.get("token"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_ascii_uppercase());
        let token_address = row
            .get("token_address")
            .or_else(|| row.get("tokenAddress"))
            .or_else(|| row.get("address"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_ascii_lowercase());

        let (direction, value_usd, balance_pct) = match source_type {
            "netflow" | "flow_intel" => {
                let v = json_f64(row.get("net_flow_24h_usd"))
                    .or_else(|| json_f64(row.get("net_flow")))
                    .or_else(|| json_f64(row.get("netFlow")))
                    .or_else(|| json_f64(row.get("net_volume")));
                let dir = v.map(|f| {
                    if f > 0.0 {
                        "inflow"
                    } else {
                        "outflow"
                    }
                });
                (dir.map(String::from), v.map(|f| f.abs()), None)
            }
            "dex_trades" => {
                let v = json_f64(row.get("trade_value_usd"))
                    .or_else(|| json_f64(row.get("value_usd")));
                // Format 1: explicit action/side
                let action = row.get("action").or_else(|| row.get("side"))
                    .and_then(|v| v.as_str()).unwrap_or("").to_ascii_lowercase();
                let dir = if action.contains("buy") {
                    Some("buy".to_string())
                } else if action.contains("sell") {
                    Some("sell".to_string())
                } else {
                    // Format 2: swap (stablecoin in/out)
                    let sold = row.get("token_sold_symbol").and_then(|v| v.as_str())
                        .unwrap_or("").to_ascii_uppercase();
                    let bought = row.get("token_bought_symbol").and_then(|v| v.as_str())
                        .unwrap_or("").to_ascii_uppercase();
                    let is_stable = |s: &str| matches!(s, "USDC" | "USDT" | "DAI" | "BUSD" | "TUSD" | "USDP" | "FDUSD");
                    if is_stable(&sold) && !is_stable(&bought) {
                        Some("buy".to_string())
                    } else if !is_stable(&sold) && is_stable(&bought) {
                        Some("sell".to_string())
                    } else {
                        None
                    }
                };
                (dir, v, None)
            }
            "holdings" => {
                let pct = json_f64(row.get("balance_24h_percent_change"))
                    .or_else(|| json_f64(row.get("balance_change_24h_pct")))
                    .or_else(|| json_f64(row.get("balance_change_pct_24h")))
                    .or_else(|| json_f64(row.get("balance_change_24h")));
                let dir = pct.map(|p| {
                    if p > 0.0 {
                        "accumulating"
                    } else {
                        "distributing"
                    }
                });
                (dir.map(String::from), None, pct)
            }
            _ => (None, None, None),
        };

        out.push(RawFlowRow {
            source_type: source_type.to_string(),
            chain,
            token_symbol,
            token_address,
            direction,
            value_usd,
            balance_pct_change: balance_pct,
            raw_row: row.clone(),
        });
    }

    out
}

//! Nansen-derived Chain category fetcher.
//!
//! Unlike [`crate::glassnode`] / [`crate::cryptoquant`] this fetcher
//! does **not** call the Nansen API directly. The `qtss-nansen` worker
//! crate already maintains nine background loops that hydrate the
//! `data_snapshots` table with global Nansen payloads (smart-money
//! netflow, holdings, flow-intelligence, smart-money DEX trades…).
//! Re-fetching here would burn API credits for no reason.
//!
//! Instead this fetcher reads those snapshots from PostgreSQL, filters
//! the rows down to the requested `symbol` via the
//! `onchain.nansen.symbol_map` config (`{ "BTCUSDT": { "chain":
//! "ethereum", "address": "0x...", "symbol": "WBTC" } }`) and produces
//! a single [`CategoryReading`] in `[-1, +1]`.
//!
//! Four blend components, all weights and the staleness budget are
//! config-driven (CLAUDE.md #2):
//! - **netflow**     — `Σ net_flow / Σ |net_flow|` from
//!   `nansen_netflows` rows for the symbol
//! - **flow_intel**  — same shape from `nansen_flow_intelligence`
//! - **dex_trades**  — `(buy − sell) / (buy + sell)` from
//!   `nansen_smart_money_dex_trades`
//! - **holdings**    — average 24h balance change % from
//!   `nansen_holdings`
//!
//! A snapshot older than `staleness_s` is dropped (component skipped,
//! confidence falls). When no symbol_map entry exists the fetcher
//! returns `UnsupportedSymbol` so the worker just logs at debug level
//! and moves on — exactly like Glassnode for non-BTC/ETH symbols.

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use sqlx::PgPool;

use crate::types::{
    CategoryKind, CategoryReading, FetcherError, OnchainCategoryFetcher, OnchainDirection,
};

const KEY_NETFLOWS: &str = "nansen_netflows";
const KEY_FLOW_INTEL: &str = "nansen_flow_intelligence";
const KEY_DEX_TRADES: &str = "nansen_smart_money_dex_trades";
const KEY_HOLDINGS: &str = "nansen_holdings";

#[derive(Debug, Clone)]
pub struct NansenTuning {
    pub staleness_s: i64,
    pub w_netflow: f64,
    pub w_flow_intel: f64,
    pub w_dex_trades: f64,
    pub w_holdings: f64,
    /// JSON object: { "BTCUSDT": { "chain": "...", "address": "0x...", "symbol": "WBTC" } }
    pub symbol_map: Value,
}

impl Default for NansenTuning {
    fn default() -> Self {
        Self {
            staleness_s: 7200,
            w_netflow: 0.40,
            w_flow_intel: 0.25,
            w_dex_trades: 0.20,
            w_holdings: 0.15,
            symbol_map: Value::Object(Default::default()),
        }
    }
}

pub struct NansenFetcher {
    pool: PgPool,
    tuning: NansenTuning,
}

impl NansenFetcher {
    pub fn new(pool: PgPool, tuning: NansenTuning) -> Self {
        Self { pool, tuning }
    }
}

/// Resolved Nansen identifiers for a single engine symbol.
#[derive(Debug, Clone)]
struct SymbolKey {
    /// Reserved for future chain-scoped filtering (e.g. multi-chain
    /// Nansen rows where the same address exists on Ethereum and BSC).
    /// Currently address+symbol matching is enough so this is unused.
    #[allow(dead_code)]
    chain: Option<String>,
    address: Option<String>,
    nansen_symbol: Option<String>,
}

impl SymbolKey {
    fn matches_row(&self, row: &Value) -> bool {
        // Address match wins (chain-agnostic — Nansen rows usually
        // include `token_address`). Falls back to symbol/name match.
        let addr_field = row
            .get("token_address")
            .or_else(|| row.get("tokenAddress"))
            .or_else(|| row.get("address"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_ascii_lowercase());
        if let (Some(want), Some(got)) =
            (self.address.as_deref().map(str::to_ascii_lowercase), addr_field)
        {
            if want == got {
                return true;
            }
        }
        let sym_field = row
            .get("token_symbol")
            .or_else(|| row.get("symbol"))
            .or_else(|| row.get("token"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_ascii_uppercase());
        if let (Some(want), Some(got)) =
            (self.nansen_symbol.as_deref().map(str::to_ascii_uppercase), sym_field)
        {
            if want == got {
                return true;
            }
        }
        false
    }
}

fn resolve_symbol(map: &Value, symbol: &str) -> Option<SymbolKey> {
    let entry = map.get(symbol).or_else(|| {
        let stripped = symbol
            .strip_suffix("USDT")
            .or_else(|| symbol.strip_suffix("USD"))
            .or_else(|| symbol.strip_suffix("BUSD"))
            .unwrap_or(symbol);
        map.get(stripped)
    })?;
    if !entry.is_object() {
        return None;
    }

    // New multi-chain format: { "chains": { "ethereum": { "symbol": "WETH", "address": "0x..." } } }
    if let Some(chains_obj) = entry.get("chains").and_then(|v| v.as_object()) {
        // Use the first chain entry for primary matching; SymbolKey.matches_row
        // is chain-agnostic (matches by address or symbol across all rows).
        let first = chains_obj.values().next()?;
        let chain = chains_obj.keys().next().map(String::from);
        let address = first.get("address").and_then(|v| v.as_str()).map(String::from);
        let nansen_symbol = first.get("symbol").and_then(|v| v.as_str()).map(String::from);
        if address.is_none() && nansen_symbol.is_none() {
            return None;
        }
        return Some(SymbolKey { chain, address, nansen_symbol });
    }

    // Old single-chain format
    let chain = entry.get("chain").and_then(|v| v.as_str()).map(String::from);
    let address = entry
        .get("address")
        .and_then(|v| v.as_str())
        .map(String::from);
    let nansen_symbol = entry
        .get("symbol")
        .and_then(|v| v.as_str())
        .map(String::from);
    if address.is_none() && nansen_symbol.is_none() {
        return None;
    }
    Some(SymbolKey { chain, address, nansen_symbol })
}

fn json_f64(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    if let Some(f) = v.as_f64() {
        return Some(f);
    }
    if let Some(s) = v.as_str() {
        return s.parse().ok();
    }
    None
}

/// Pull `data` array out of a Nansen response envelope.
fn rows_of(resp: &Value) -> Vec<&Value> {
    match resp.get("data") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(other) => vec![other],
        None => Vec::new(),
    }
}

/// Σ net_flow / Σ |net_flow| → [-1, +1] over rows matching `key`.
fn score_netflow_like(resp: &Value, key: &SymbolKey, field_aliases: &[&str]) -> Option<f64> {
    let rows = rows_of(resp);
    let mut net = 0.0_f64;
    let mut abs = 0.0_f64;
    let mut hits = 0u32;
    for row in rows {
        if !key.matches_row(row) {
            continue;
        }
        let v = field_aliases
            .iter()
            .find_map(|f| json_f64(row.get(*f)))
            .unwrap_or(0.0);
        net += v;
        abs += v.abs();
        hits += 1;
    }
    if hits == 0 || abs < 1e-12 {
        return None;
    }
    Some((net / abs).clamp(-1.0, 1.0))
}

fn is_stablecoin(sym: &str) -> bool {
    matches!(
        sym,
        "USDC" | "USDT" | "DAI" | "BUSD" | "TUSD" | "USDP" | "FDUSD"
    )
}

fn classify_dex_row(row: &Value) -> Option<(&'static str, f64)> {
    let value = json_f64(row.get("trade_value_usd"))
        .or_else(|| json_f64(row.get("value_usd")))
        .or_else(|| json_f64(row.get("usd_value")))
        .unwrap_or(0.0)
        .max(0.0);
    if value <= 0.0 {
        return None;
    }
    // Format 1: explicit action/side
    let action = row
        .get("action")
        .or_else(|| row.get("side"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if action.contains("buy") {
        return Some(("buy", value));
    }
    if action.contains("sell") {
        return Some(("sell", value));
    }
    // Format 2: swap (token_bought/token_sold)
    let sold = row.get("token_sold_symbol").and_then(|v| v.as_str()).unwrap_or("").to_ascii_uppercase();
    let bought = row.get("token_bought_symbol").and_then(|v| v.as_str()).unwrap_or("").to_ascii_uppercase();
    if is_stablecoin(&sold) && !is_stablecoin(&bought) {
        return Some(("buy", value));
    }
    if !is_stablecoin(&sold) && is_stablecoin(&bought) {
        return Some(("sell", value));
    }
    None
}

fn score_dex_trades(resp: &Value, key: &SymbolKey) -> Option<f64> {
    let rows = rows_of(resp);
    let mut buy = 0.0_f64;
    let mut sell = 0.0_f64;
    let mut hits = 0u32;
    for row in rows {
        if !key.matches_row(row) {
            continue;
        }
        if let Some((side, value)) = classify_dex_row(row) {
            if side == "buy" {
                buy += value;
            } else {
                sell += value;
            }
            hits += 1;
        }
    }
    let t = buy + sell;
    if hits == 0 || t < 1e-12 {
        return None;
    }
    Some(((buy - sell) / t).clamp(-1.0, 1.0))
}

fn score_holdings(resp: &Value, key: &SymbolKey) -> Option<f64> {
    let rows = rows_of(resp);
    let mut sum_pct = 0.0_f64;
    let mut hits = 0u32;
    for row in rows {
        if !key.matches_row(row) {
            continue;
        }
        if let Some(p) = json_f64(row.get("balance_24h_percent_change"))
            .or_else(|| json_f64(row.get("balance_change_24h_pct")))
            .or_else(|| json_f64(row.get("balance_change_pct_24h")))
            .or_else(|| json_f64(row.get("balance_change_24h")))
        {
            sum_pct += p;
            hits += 1;
        }
    }
    if hits == 0 {
        return None;
    }
    // ±10% sustained 24h delta = ±1.0 saturation.
    Some(((sum_pct / hits as f64) / 10.0).clamp(-1.0, 1.0))
}

async fn load_snapshot(
    pool: &PgPool,
    key: &str,
    staleness_s: i64,
) -> Option<Value> {
    let row = qtss_storage::data_snapshots::fetch_data_snapshot(pool, key)
        .await
        .ok()
        .flatten()?;
    if row.error.is_some() {
        return None;
    }
    let age = Utc::now().signed_duration_since(row.computed_at).num_seconds();
    if age > staleness_s {
        return None;
    }
    row.response_json
}

#[async_trait]
impl OnchainCategoryFetcher for NansenFetcher {
    fn name(&self) -> &'static str {
        "nansen"
    }

    fn category(&self) -> CategoryKind {
        CategoryKind::Chain
    }

    async fn fetch(&self, symbol: &str) -> Result<CategoryReading, FetcherError> {
        let Some(key) = resolve_symbol(&self.tuning.symbol_map, symbol) else {
            return Err(FetcherError::UnsupportedSymbol(symbol.to_string()));
        };

        // Pull all four snapshots once. Stale or missing → None.
        let netflow_resp = load_snapshot(&self.pool, KEY_NETFLOWS, self.tuning.staleness_s).await;
        let flow_intel_resp =
            load_snapshot(&self.pool, KEY_FLOW_INTEL, self.tuning.staleness_s).await;
        let dex_trades_resp =
            load_snapshot(&self.pool, KEY_DEX_TRADES, self.tuning.staleness_s).await;
        let holdings_resp =
            load_snapshot(&self.pool, KEY_HOLDINGS, self.tuning.staleness_s).await;

        let netflow = netflow_resp.as_ref().and_then(|r| {
            score_netflow_like(r, &key, &["net_flow", "netFlow", "net_volume", "net_flow_24h_usd"])
        });
        let flow_intel = flow_intel_resp.as_ref().and_then(|r| {
            score_netflow_like(r, &key, &["net_flow_24h_usd", "net_flow", "netFlow", "net_flow_usd"])
        });
        let dex = dex_trades_resp.as_ref().and_then(|r| score_dex_trades(r, &key));
        let holdings = holdings_resp.as_ref().and_then(|r| score_holdings(r, &key));

        let reading = blend(
            netflow,
            flow_intel,
            dex,
            holdings,
            &self.tuning,
            symbol,
        );
        // When all snapshots are stale or missing (confidence=0) return
        // an error so the aggregator skips the phantom zero reading.
        if reading.confidence <= 0.0 {
            return Err(FetcherError::NoData(format!(
                "nansen: all snapshots stale/missing for {symbol}"
            )));
        }
        Ok(reading)
    }
}

fn blend(
    netflow: Option<f64>,
    flow_intel: Option<f64>,
    dex: Option<f64>,
    holdings: Option<f64>,
    t: &NansenTuning,
    symbol: &str,
) -> CategoryReading {
    let mut weighted = 0.0_f64;
    let mut wsum = 0.0_f64;
    let mut wmax = 0.0_f64;
    let mut details = Vec::new();

    let mut push = |label: &str, val: Option<f64>, w: f64| {
        wmax += w;
        if let Some(v) = val {
            weighted += v * w;
            wsum += w;
            details.push(format!("[NS] {label} {v:+.2} (w={w:.2})"));
        }
    };

    push("netflow", netflow, t.w_netflow);
    push("flow_intel", flow_intel, t.w_flow_intel);
    push("dex_trades", dex, t.w_dex_trades);
    push("holdings", holdings, t.w_holdings);

    let score = if wsum < 1e-9 {
        0.0
    } else {
        (weighted / wsum).clamp(-1.0, 1.0)
    };
    let confidence = if wmax < 1e-9 { 0.0 } else { (wsum / wmax).clamp(0.0, 1.0) };
    let direction = if score > 0.05 {
        OnchainDirection::Long
    } else if score < -0.05 {
        OnchainDirection::Short
    } else {
        OnchainDirection::Neutral
    };

    if details.is_empty() {
        details.push(format!("[NS] {symbol}: no fresh snapshots"));
    }

    CategoryReading {
        category: CategoryKind::Chain,
        score,
        confidence,
        direction: Some(direction),
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key_btc() -> SymbolKey {
        SymbolKey {
            chain: Some("ethereum".into()),
            address: Some("0xWbTc".into()),
            nansen_symbol: Some("WBTC".into()),
        }
    }

    #[test]
    fn symbol_map_strip_suffix() {
        let map = json!({ "BTC": { "address": "0xabc", "symbol": "WBTC" } });
        let k = resolve_symbol(&map, "BTCUSDT").unwrap();
        assert_eq!(k.address.as_deref(), Some("0xabc"));
    }

    #[test]
    fn netflow_score_filters_by_address() {
        let resp = json!({
            "data": [
                { "token_address": "0xWBTC", "net_flow": 1_000_000.0 },
                { "token_address": "0xWBTC", "net_flow":   500_000.0 },
                { "token_address": "0xOTHER", "net_flow": -9_999_999.0 },
            ]
        });
        let s = score_netflow_like(&resp, &key_btc(), &["net_flow"]).unwrap();
        assert!(s > 0.99);
    }

    #[test]
    fn dex_trades_buy_dominant() {
        let resp = json!({
            "data": [
                { "token_symbol": "WBTC", "action": "BUY",  "trade_value_usd": 200_000.0 },
                { "token_symbol": "WBTC", "action": "SELL", "trade_value_usd":  50_000.0 },
            ]
        });
        let s = score_dex_trades(&resp, &key_btc()).unwrap();
        assert!(s > 0.5);
    }

    #[test]
    fn holdings_accumulation() {
        let resp = json!({
            "data": [
                { "token_symbol": "WBTC", "balance_change_24h_pct": 8.0 },
                { "token_symbol": "WBTC", "balance_change_24h_pct": 4.0 },
            ]
        });
        let s = score_holdings(&resp, &key_btc()).unwrap();
        assert!(s > 0.5);
    }

    #[test]
    fn blend_no_data_neutral() {
        let r = blend(None, None, None, None, &NansenTuning::default(), "BTCUSDT");
        assert_eq!(r.score, 0.0);
        assert_eq!(r.confidence, 0.0);
    }

    #[test]
    fn blend_full_bull() {
        let r = blend(Some(0.9), Some(0.8), Some(0.7), Some(0.6), &NansenTuning::default(), "BTC");
        assert!(r.score > 0.7);
        assert!((r.confidence - 1.0).abs() < 1e-9);
    }
}

//! PLAN §3 — `SignalScorer`: `data_snapshots.source_key` → skor fonksiyonu.
//!
//! **Vendor policy:** `docs/PLAN_CONFLUENCE_AND_MARKET_DATA.md` §2 — CryptoQuant / Whale Alert **free**
//! otomasyon için dışarıda; birincil gerçek zamanlı: Binance FAPI, Hyperliquid, Coinglass (+ API key),
//! Nansen (smart money). DeFi Llama yalnızca TVL tamamlayıcı; DEX baskısı için Graph / DEX API + yeni dal.
//!
//! Kapsam: Nansen (derinlik + DEX buy/sell), Binance (taker, premium funding, OI), HL `metaAndAssetCtxs`,
//! Coinglass (netflow / likidasyon — esnek JSON ayrıştırıcı).

use serde_json::Value;

fn parse_json_value_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .or_else(|| v.as_i64().map(|i| i as f64))
}

fn parse_json_f64(v: Option<&Value>) -> Option<f64> {
    v.and_then(parse_json_value_f64)
}

/// Bilinen `source_key` için skor; çoğu [-1, 1] civarı.
#[must_use]
pub fn score_for_source_key(source_key: &str, response: &Value) -> f64 {
    if source_key == "nansen_token_screener" {
        return score_nansen_dex_buy_sell_pressure(response);
    }
    if source_key == "nansen_netflows" {
        return score_nansen_netflows(response);
    }
    if source_key == "nansen_perp_trades" {
        return score_nansen_perp_direction(response);
    }
    if source_key == "nansen_flow_intelligence" {
        return score_nansen_flow_intelligence(response);
    }
    if source_key == "nansen_who_bought_sold" {
        return score_nansen_buyer_quality(response);
    }
    if source_key.starts_with("binance_taker_") && source_key.ends_with("usdt") {
        return score_binance_taker_ratio(response).unwrap_or(0.0);
    }
    if source_key.starts_with("binance_ls_ratio_") && source_key.ends_with("usdt") {
        return score_binance_global_long_short_account_ratio(response);
    }
    if source_key.starts_with("binance_premium_") && source_key.ends_with("usdt") {
        return score_binance_premium_funding(response);
    }
    if source_key.starts_with("binance_open_interest_") && source_key.ends_with("usdt") {
        return score_binance_open_interest_heat(response);
    }
    if source_key == "hl_meta_asset_ctxs" {
        return 0.0;
    }
    if source_key.starts_with("coinglass_netflow") {
        return score_coinglass_netflow_like(response);
    }
    if source_key.contains("liquidation") && source_key.contains("coinglass") {
        return score_coinglass_liquidations_like(response);
    }
    0.0
}

/// Nansen: API response row count → coverage only (not directional smart money). For diagnostics;
/// do not blend into aggregate on-chain score (see dev guide §3.1 / §3.9).
#[must_use]
pub fn score_nansen_response_coverage(response: &Value) -> f64 {
    let n = response
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

/// smart-money/netflows: sum `net_flow` / sum |net_flow| → [-1, 1].
#[must_use]
pub fn score_nansen_netflows(response: &Value) -> f64 {
    let data = match response.get("data") {
        Some(d) => d,
        None => return 0.0,
    };
    let rows: Vec<&Value> = if let Some(a) = data.as_array() {
        a.iter().collect()
    } else {
        vec![data]
    };
    let mut net_sum = 0_f64;
    let mut abs_sum = 0_f64;
    for row in rows.iter().take(2000) {
        let nf = parse_json_f64(row.get("net_flow"))
            .or_else(|| parse_json_f64(row.get("netFlow")))
            .or_else(|| parse_json_f64(row.get("net_volume")))
            .unwrap_or(0.0);
        net_sum += nf;
        abs_sum += nf.abs();
    }
    if abs_sum < 1e-12 {
        return 0.0;
    }
    (net_sum / abs_sum).clamp(-1.0, 1.0)
}

/// smart-money/perp-trades: long vs short notional share → [-1, 1] (0.5-centered).
#[must_use]
pub fn score_nansen_perp_direction(response: &Value) -> f64 {
    let Some(rows) = response.get("data").and_then(|d| d.as_array()) else {
        return 0.0;
    };
    let mut long_n = 0_f64;
    let mut short_n = 0_f64;
    for row in rows.iter().take(2000) {
        let side = row
            .get("side")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let n = parse_json_f64(row.get("notional_usd"))
            .or_else(|| parse_json_f64(row.get("notionalUsd")))
            .or_else(|| parse_json_f64(row.get("position_value_usd")))
            .unwrap_or(0.0)
            .max(0.0);
        if side.contains("long") {
            long_n += n;
        } else if side.contains("short") {
            short_n += n;
        }
    }
    let t = long_n + short_n;
    if t < 1e-12 {
        return 0.0;
    }
    let ratio = long_n / t;
    ((ratio - 0.5) * 2.0).clamp(-1.0, 1.0)
}

/// tgm/flow-intelligence: same normalization idea as Coinglass netflow (exchange flow).
#[must_use]
pub fn score_nansen_flow_intelligence(response: &Value) -> f64 {
    score_coinglass_netflow_like(response)
}

/// tgm/who-bought-sold: wallet label quality ratio → [-1, 1].
#[must_use]
pub fn score_nansen_buyer_quality(response: &Value) -> f64 {
    let Some(rows) = response.get("data").and_then(|d| d.as_array()) else {
        return 0.0;
    };
    let mut good = 0_usize;
    let mut bad = 0_usize;
    for row in rows.iter().take(500) {
        let label = row
            .get("wallet_label")
            .or_else(|| row.get("label"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if label.contains("smart")
            || label.contains("fund")
            || label.contains("vc")
            || label.contains("institution")
        {
            good += 1;
        } else if label.contains("bot")
            || label.contains("mev")
            || label.contains("wash")
        {
            bad += 1;
        }
    }
    let denom = good + bad;
    if denom == 0 {
        return 0.0;
    }
    let ratio = good as f64 / denom as f64;
    (ratio * 2.0 - 1.0).clamp(-1.0, 1.0)
}

/// Nansen token screener satırlarında DEX alım/satım dengesi (buy vs sell volume alanları).
#[must_use]
pub fn score_nansen_dex_buy_sell_pressure(response: &Value) -> f64 {
    let Some(rows) = response.get("data").and_then(|d| d.as_array()) else {
        return 0.0;
    };
    let mut buy = 0_f64;
    let mut sell = 0_f64;
    for row in rows.iter().take(500) {
        let bv = row
            .get("buy_volume")
            .or_else(|| row.get("buyVolume"))
            .or_else(|| row.get("dex_buy_volume"))
            .and_then(parse_json_value_f64)
            .unwrap_or(0.0_f64)
            .max(0.0_f64);
        let sv = row
            .get("sell_volume")
            .or_else(|| row.get("sellVolume"))
            .or_else(|| row.get("dex_sell_volume"))
            .and_then(parse_json_value_f64)
            .unwrap_or(0.0_f64)
            .max(0.0_f64);
        buy += bv;
        sell += sv;
    }
    let t = buy + sell;
    if t < 1e-12 {
        return 0.0;
    }
    ((buy - sell) / t).clamp(-1.0, 1.0)
}

/// `GET /futures/data/globalLongShortAccountRatio` — dizi, son eleman; SPEC §4.1 `binance_ls_ratio`.
#[must_use]
pub fn score_binance_global_long_short_account_ratio(resp: &Value) -> f64 {
    let Some(arr) = resp.as_array() else {
        return 0.0;
    };
    let Some(last) = arr.last() else {
        return 0.0;
    };
    let ratio = last
        .get("longShortRatio")
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| last.get("longShortRatio").and_then(|x| x.as_f64()))
        .unwrap_or(1.0);
    if ratio > 2.0 {
        -0.6
    } else if ratio < 0.5 {
        0.6
    } else if (0.8..=1.2).contains(&ratio) {
        0.0
    } else if ratio > 1.2 {
        -0.3
    } else {
        0.3
    }
}

#[must_use]
pub fn score_binance_taker_ratio(resp: &Value) -> Option<f64> {
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

/// `GET /fapi/v1/premiumIndex` tek sembol — `lastFundingRate` pozitif → long’lar öder (kalabalık long).
#[must_use]
pub fn score_binance_premium_funding(resp: &Value) -> f64 {
    let Some(fr_s) = resp.get("lastFundingRate").and_then(|x| x.as_str()) else {
        return 0.0;
    };
    let Ok(fr) = fr_s.parse::<f64>() else {
        return 0.0;
    };
    (fr * 4000.0).clamp(-1.0, 1.0)
}

/// `GET /fapi/v1/openInterest` — seviye tek başına yön vermez; “kaldıraç hacmi” için zayıf pozitif ısı.
#[must_use]
pub fn score_binance_open_interest_heat(resp: &Value) -> f64 {
    let oi = resp
        .get("openInterest")
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| resp.get("openInterest").and_then(|x| x.as_f64()))
        .unwrap_or(0.0);
    if oi <= 0.0 {
        return 0.0;
    }
    let x = oi.ln() - 8.0;
    (x / 6.0).clamp(0.0, 1.0) * 0.12
}

/// Hyperliquid `info` `metaAndAssetCtxs`: `[ { universe: [...] }, [ ctx per coin, ... ] ]`
#[must_use]
pub fn score_hl_meta_asset_ctxs_for_coin(response: &Value, coin: &str) -> f64 {
    let Some(top) = response.as_array() else {
        return 0.0;
    };
    if top.len() < 2 {
        return 0.0;
    }
    let Some(universe) = top[0].get("universe").and_then(|u| u.as_array()) else {
        return 0.0;
    };
    let Some(ctxs) = top[1].as_array() else {
        return 0.0;
    };
    let want = coin.trim().to_uppercase();
    for (i, u) in universe.iter().enumerate() {
        let name = u
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_uppercase();
        if name != want {
            continue;
        }
        let Some(ctx) = ctxs.get(i) else {
            return 0.0;
        };
        let funding = parse_json_f64(ctx.get("funding")).unwrap_or(0.0);
        return (funding * 4000.0).clamp(-1.0, 1.0);
    }
    0.0
}

fn sum_numeric_array_path(v: &Value, path: &[&str]) -> f64 {
    let mut cur = v;
    for p in path {
        cur = match cur.get(*p) {
            Some(x) => x,
            None => return 0.0,
        };
    }
    let Some(arr) = cur.as_array() else {
        return 0.0;
    };
    arr.iter()
        .filter_map(|x| parse_json_f64(Some(x)))
        .sum()
}

/// Coinglass / benzeri: netflow, exchange flow — yaygın `data` dizisi veya `inflow`/`outflow` alanları.
#[must_use]
pub fn score_coinglass_netflow_like(v: &Value) -> f64 {
    if let (Some(inf), Some(out)) = (
        parse_json_f64(v.get("inflow")),
        parse_json_f64(v.get("outflow")),
    ) {
        let t = inf.abs() + out.abs();
        if t > 1e-12 {
            return ((out - inf) / t).clamp(-1.0, 1.0);
        }
    }
    if let Some(data) = v.get("data") {
        if let Some(obj) = data.as_object() {
            if let (Some(inf), Some(out)) = (
                parse_json_f64(obj.get("inflow")),
                parse_json_f64(obj.get("outflow")),
            ) {
                let t = inf.abs() + out.abs();
                if t > 1e-12 {
                    return ((out - inf) / t).clamp(-1.0, 1.0);
                }
            }
        }
        let net = sum_numeric_array_path(v, &["data"]);
        if net.abs() > 1e-12 {
            return (net / (net.abs() + 1.0)).clamp(-1.0, 1.0);
        }
    }
    0.0
}

/// Likidasyon yönü: `longVol`/`shortVol`, `buyVol`/`sellVol`, `side` + `usd` listeleri.
#[must_use]
pub fn score_coinglass_liquidations_like(v: &Value) -> f64 {
    let long_v = parse_json_f64(v.get("longVol"))
        .or_else(|| parse_json_f64(v.get("long_vol")))
        .unwrap_or(0.0)
        + parse_json_f64(v.get("longLiquidationUsd")).unwrap_or(0.0);
    let short_v = parse_json_f64(v.get("shortVol"))
        .or_else(|| parse_json_f64(v.get("short_vol")))
        .unwrap_or(0.0)
        + parse_json_f64(v.get("shortLiquidationUsd")).unwrap_or(0.0);
    let t = long_v + short_v;
    if t > 1e-12 {
        return ((short_v - long_v) / t).clamp(-1.0, 1.0);
    }
    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
        let mut long_usd = 0_f64;
        let mut short_usd = 0_f64;
        for row in arr.iter().take(2000) {
            let side = row
                .get("side")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let usd = parse_json_f64(row.get("usd"))
                .or_else(|| parse_json_f64(row.get("liquidationUsd")))
                .unwrap_or(0.0);
            if side.contains("long") || side == "l" {
                long_usd += usd;
            } else if side.contains("short") || side == "s" {
                short_usd += usd;
            }
        }
        let tt = long_usd + short_usd;
        if tt > 1e-12 {
            return ((short_usd - long_usd) / tt).clamp(-1.0, 1.0);
        }
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dispatch_nansen_token_screener_uses_dex_pressure() {
        let v = json!({
            "data": [
                { "buy_volume": 100.0, "sell_volume": 0.0 }
            ]
        });
        let s = score_for_source_key("nansen_token_screener", &v);
        assert!((s - 1.0).abs() < 1e-9);
    }

    #[test]
    fn nansen_response_coverage_rows() {
        let v = json!({ "data": (0..25).map(|_| json!({})).collect::<Vec<_>>() });
        assert!((score_nansen_response_coverage(&v) - 0.35).abs() < 1e-9);
    }

    #[test]
    fn nansen_netflows_signed() {
        let v = json!({
            "data": [
                { "net_flow": 30.0 },
                { "net_flow": -10.0 }
            ]
        });
        let s = score_nansen_netflows(&v);
        assert!(s > 0.0);
    }

    #[test]
    fn nansen_netflows_bullish_dev_guide() {
        let v = json!({
            "data": [
                { "net_flow": 1_000_000.0, "symbol": "ETH" },
                { "net_flow": 500_000.0, "symbol": "BTC" }
            ]
        });
        let s = score_nansen_netflows(&v);
        assert!(s > 0.0, "positive netflow sum should be bullish");
        assert!(s <= 1.0);
    }

    #[test]
    fn nansen_perp_direction_heavy_long_dev_guide() {
        let v = json!({
            "data": [
                { "side": "long", "notional_usd": 8_000_000.0 },
                { "side": "short", "notional_usd": 2_000_000.0 }
            ]
        });
        let s = score_nansen_perp_direction(&v);
        assert!(s > 0.3, "heavy long notional should skew positive");
    }

    #[test]
    fn dispatch_binance_taker_key() {
        let v = json!([{ "buySellRatio": "1.1" }]);
        let s = score_for_source_key("binance_taker_btcusdt", &v);
        assert!(s > 0.0);
    }

    #[test]
    fn nansen_dex_pressure() {
        let v = json!({
            "data": [
                { "buy_volume": 100.0, "sell_volume": 50.0 },
                { "buyVolume": 10.0, "sellVolume": 40.0 }
            ]
        });
        let s = score_nansen_dex_buy_sell_pressure(&v);
        assert!(s > 0.0);
    }

    #[test]
    fn hl_funding_for_btc() {
        let v = json!([
            { "universe": [ { "name": "BTC" }, { "name": "ETH" } ] },
            [
                { "funding": "0.0001" },
                { "funding": "-0.00005" }
            ]
        ]);
        let s = score_hl_meta_asset_ctxs_for_coin(&v, "BTC");
        assert!(s > 0.0);
    }

    #[test]
    fn premium_funding() {
        let v = json!({ "lastFundingRate": "-0.0002" });
        let s = score_binance_premium_funding(&v);
        assert!(s < 0.0);
    }
}

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
        return score_nansen_smart_money_depth(response);
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

/// Nansen: screener satır sayısı → smart-money bağlam derinliği (mevcut heuristik).
#[must_use]
pub fn score_nansen_smart_money_depth(response: &Value) -> f64 {
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
    fn dispatch_nansen_key() {
        let v = json!({ "data": (0..25).map(|_| json!({})).collect::<Vec<_>>() });
        let s = score_for_source_key("nansen_token_screener", &v);
        assert!((s - 0.35).abs() < 1e-9);
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

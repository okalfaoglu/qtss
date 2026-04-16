//! Derivatives ConfluenceSource — Binance public endpoint snapshot'larından
//! (`binance_*` keys in `data_snapshots`) feature üretir.
//!
//! v1 feature set:
//!   * oi_value                (open interest, float)
//!   * funding_rate            (premiumIndex.lastFundingRate)
//!   * mark_price              (premiumIndex.markPrice)
//!   * ls_ratio_long_pct       (globalLongShortAccountRatio last row)
//!   * taker_buy_sell_ratio    (takerlongshortRatio last row)
//!   * liq_count_1h            (forceOrder rolling window count)
//!   * liq_long_qty_1h         (BUY-initiated liquidations → short forced close → bullish pressure)
//!   * liq_short_qty_1h        (SELL-initiated → long forced close → bearish pressure)
//!   * cvd_last                (last running CVD)
//!   * cvd_slope_5             (linear slope of last 5 buckets)
//!   * funding_history_mean_24 (last 24 intervals mean)
//!
//! Tüm okumalar `SourceQuery::data_snapshot(key)` üzerinden; key format
//! `binance_<metric>_<pair_lower>`.

use async_trait::async_trait;
use qtss_confluence::{ConfluenceSource, FeatureSnapshot, SourceContext, SourceQuery};
use serde_json::Value;

pub struct DerivativesSource;

const SPEC_VERSION: i32 = 1;

fn pair_lower(symbol: &str) -> String {
    symbol.to_lowercase()
}

fn f64_from(v: &Value) -> Option<f64> {
    if let Some(x) = v.as_f64() {
        return Some(x);
    }
    if let Some(s) = v.as_str() {
        return s.parse::<f64>().ok();
    }
    None
}

fn last_obj<'a>(arr: &'a Value) -> Option<&'a Value> {
    arr.as_array().and_then(|a| a.last())
}

fn linear_slope(buckets: &[(i64, f64)]) -> Option<f64> {
    if buckets.len() < 2 {
        return None;
    }
    let n = buckets.len() as f64;
    let mean_x = buckets.iter().map(|b| b.0 as f64).sum::<f64>() / n;
    let mean_y = buckets.iter().map(|b| b.1).sum::<f64>() / n;
    let mut num = 0.0;
    let mut den = 0.0;
    for (x, y) in buckets {
        let dx = (*x as f64) - mean_x;
        num += dx * (y - mean_y);
        den += dx * dx;
    }
    if den.abs() < 1e-12 {
        None
    } else {
        Some(num / den)
    }
}

#[async_trait]
impl ConfluenceSource for DerivativesSource {
    fn key(&self) -> &'static str {
        "derivatives"
    }

    async fn extract(
        &self,
        ctx: &SourceContext<'_>,
        query: &dyn SourceQuery,
    ) -> Option<FeatureSnapshot> {
        let p = pair_lower(ctx.symbol);
        let mut snap = FeatureSnapshot::new("derivatives", SPEC_VERSION);

        if let Some(oi) = query.data_snapshot(&format!("binance_open_interest_{p}")).await {
            if let Some(v) = oi.get("openInterest").and_then(f64_from) {
                snap.insert_f64("oi_value", v);
            }
        }

        if let Some(prem) = query.data_snapshot(&format!("binance_premium_{p}")).await {
            if let Some(fr) = prem.get("lastFundingRate").and_then(f64_from) {
                snap.insert_f64("funding_rate", fr);
            }
            if let Some(mp) = prem.get("markPrice").and_then(f64_from) {
                snap.insert_f64("mark_price", mp);
            }
        }

        if let Some(ls) = query.data_snapshot(&format!("binance_ls_ratio_{p}")).await {
            if let Some(last) = last_obj(&ls) {
                if let Some(v) = last.get("longAccount").and_then(f64_from) {
                    snap.insert_f64("ls_ratio_long_pct", v);
                }
                if let Some(v) = last.get("longShortRatio").and_then(f64_from) {
                    snap.insert_f64("ls_ratio", v);
                }
            }
        }

        if let Some(tk) = query.data_snapshot(&format!("binance_taker_{p}")).await {
            if let Some(last) = last_obj(&tk) {
                if let Some(v) = last.get("buySellRatio").and_then(f64_from) {
                    snap.insert_f64("taker_buy_sell_ratio", v);
                }
            }
        }

        if let Some(fh) = query
            .data_snapshot(&format!("binance_funding_rate_{p}"))
            .await
        {
            if let Some(arr) = fh.as_array() {
                let vals: Vec<f64> = arr
                    .iter()
                    .filter_map(|r| r.get("fundingRate").and_then(f64_from))
                    .collect();
                if !vals.is_empty() {
                    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
                    snap.insert_f64("funding_history_mean_24", mean);
                    snap.insert_i64("funding_history_n", vals.len() as i64);
                }
            }
        }

        if let Some(liq) = query
            .data_snapshot(&format!("binance_liquidations_{p}"))
            .await
        {
            if let Some(events) = liq.get("events").and_then(|v| v.as_array()) {
                let mut long_qty = 0.0;
                let mut short_qty = 0.0;
                for e in events {
                    let q = e.get("qty").and_then(f64_from).unwrap_or(0.0);
                    let side = e.get("side").and_then(|v| v.as_str()).unwrap_or("");
                    // Binance @forceOrder "side": order side of the liquidation.
                    //   SELL → long position forced to close → bearish pressure event
                    //   BUY  → short position forced to close → bullish pressure event
                    if side.eq_ignore_ascii_case("BUY") {
                        short_qty += q;
                    } else if side.eq_ignore_ascii_case("SELL") {
                        long_qty += q;
                    }
                }
                snap.insert_f64("liq_long_qty_1h", long_qty);
                snap.insert_f64("liq_short_qty_1h", short_qty);
                snap.insert_i64("liq_count_1h", events.len() as i64);
            }
        }

        if let Some(cvd) = query.data_snapshot(&format!("binance_cvd_{p}")).await {
            if let Some(arr) = cvd.get("buckets").and_then(|v| v.as_array()) {
                if let Some(last) = arr.last() {
                    if let Some(v) = last.get("cvd").and_then(f64_from) {
                        snap.insert_f64("cvd_last", v);
                    }
                }
                let tail: Vec<(i64, f64)> = arr
                    .iter()
                    .rev()
                    .take(5)
                    .filter_map(|b| {
                        let ts = b.get("bucket_ts_ms").and_then(|x| x.as_i64())?;
                        let c = b.get("cvd").and_then(f64_from)?;
                        Some((ts, c))
                    })
                    .collect();
                if let Some(slope) = linear_slope(&tail) {
                    snap.insert_f64("cvd_slope_5", slope);
                }
            }
        }

        if snap.features.is_empty() {
            return None;
        }
        Some(snap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::HashMap;

    struct MapQ(HashMap<String, Value>);
    #[async_trait]
    impl SourceQuery for MapQ {
        async fn data_snapshot(&self, key: &str) -> Option<Value> {
            self.0.get(key).cloned()
        }
        async fn latest_regime(&self, _: &str, _: &str, _: &str) -> Option<Value> {
            None
        }
        async fn latest_tbm(&self, _: &str, _: &str, _: &str) -> Option<Value> {
            None
        }
    }

    #[tokio::test]
    async fn extracts_full_derivatives_vector() {
        let mut m = HashMap::new();
        m.insert("binance_open_interest_btcusdt".into(), json!({"openInterest": "12345.5"}));
        m.insert(
            "binance_premium_btcusdt".into(),
            json!({"lastFundingRate": "0.00012", "markPrice": "65000.0"}),
        );
        m.insert(
            "binance_ls_ratio_btcusdt".into(),
            json!([{"longAccount":"0.55","longShortRatio":"1.22"}]),
        );
        m.insert(
            "binance_liquidations_btcusdt".into(),
            json!({"events":[{"side":"SELL","qty":1.5},{"side":"BUY","qty":0.4}]}),
        );
        m.insert(
            "binance_cvd_btcusdt".into(),
            json!({"buckets":[
                {"bucket_ts_ms":1000,"cvd":1.0},
                {"bucket_ts_ms":2000,"cvd":2.0},
                {"bucket_ts_ms":3000,"cvd":3.0}
            ]}),
        );
        let ctx = SourceContext {
            exchange: "binance",
            symbol: "BTCUSDT",
            timeframe: "15m",
            detection_id: None,
            setup_id: None,
            event_bar_ms: None,
            raw_detection: &Value::Null,
        };
        let s = DerivativesSource.extract(&ctx, &MapQ(m)).await.unwrap();
        assert!((s.features["oi_value"].as_f64().unwrap() - 12345.5).abs() < 1e-6);
        assert!((s.features["funding_rate"].as_f64().unwrap() - 0.00012).abs() < 1e-9);
        assert!((s.features["liq_long_qty_1h"].as_f64().unwrap() - 1.5).abs() < 1e-9);
        assert!((s.features["liq_short_qty_1h"].as_f64().unwrap() - 0.4).abs() < 1e-9);
        assert!((s.features["cvd_last"].as_f64().unwrap() - 3.0).abs() < 1e-9);
        assert!(s.features.contains_key("cvd_slope_5"));
    }

    #[tokio::test]
    async fn returns_none_when_no_snapshots() {
        let ctx = SourceContext {
            exchange: "binance",
            symbol: "BTCUSDT",
            timeframe: "15m",
            detection_id: None,
            setup_id: None,
            event_bar_ms: None,
            raw_detection: &Value::Null,
        };
        assert!(DerivativesSource
            .extract(&ctx, &MapQ(HashMap::new()))
            .await
            .is_none());
    }
}

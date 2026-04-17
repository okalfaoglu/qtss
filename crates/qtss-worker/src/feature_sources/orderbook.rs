//! Faz 9.6 — Orderbook ConfluenceSource.
//!
//! Reads `data_snapshots(binance_depth_<pair>)` written by the
//! `depth_stream_loop` and produces orderbook-derived features for the
//! feature store.

use async_trait::async_trait;
use qtss_confluence::{ConfluenceSource, FeatureSnapshot, SourceContext, SourceQuery};
use serde_json::Value;

pub struct OrderbookSource;

const SPEC_VERSION: i32 = 1;

fn f64_from(v: &Value) -> Option<f64> {
    if let Some(x) = v.as_f64() {
        return Some(x);
    }
    if let Some(s) = v.as_str() {
        return s.parse::<f64>().ok();
    }
    None
}

#[async_trait]
impl ConfluenceSource for OrderbookSource {
    fn key(&self) -> &'static str {
        "orderbook"
    }

    async fn extract(
        &self,
        ctx: &SourceContext<'_>,
        query: &dyn SourceQuery,
    ) -> Option<FeatureSnapshot> {
        let p = ctx.symbol.to_lowercase();
        let data = query
            .data_snapshot(&format!("binance_depth_{p}"))
            .await?;

        let mut snap = FeatureSnapshot::new("orderbook", SPEC_VERSION);

        if let Some(v) = data.get("spread_bps").and_then(f64_from) {
            snap.insert_f64("ob_spread_bps", v);
        }
        if let Some(v) = data.get("imbalance_top_n").and_then(f64_from) {
            snap.insert_f64("ob_imbalance_top_n", v);
        }
        if let Some(bid) = data.get("bid_depth_pct").and_then(f64_from) {
            snap.insert_f64("ob_bid_depth_pct", bid);
            if let Some(ask) = data.get("ask_depth_pct").and_then(f64_from) {
                snap.insert_f64("ob_ask_depth_pct", ask);
                if ask > 0.0 {
                    snap.insert_f64("ob_depth_ratio", bid / ask);
                }
            }
        }
        if let Some(bw) = data.get("bid_wall").and_then(f64_from) {
            snap.insert_f64("ob_bid_wall", bw);
            if let Some(aw) = data.get("ask_wall").and_then(f64_from) {
                snap.insert_f64("ob_ask_wall", aw);
                if aw > 0.0 {
                    snap.insert_f64("ob_wall_ratio", bw / aw);
                }
            }
        }

        // VWAP skew: (mid - vwap_bid_5) / (vwap_ask_5 - mid)
        if let (Some(mid), Some(vb), Some(va)) = (
            data.get("mid_price").and_then(f64_from),
            data.get("vwap_bid_5").and_then(f64_from),
            data.get("vwap_ask_5").and_then(f64_from),
        ) {
            let denom = va - mid;
            if denom.abs() > 1e-12 {
                snap.insert_f64("ob_vwap_skew", (mid - vb) / denom);
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
    async fn extracts_full_orderbook_vector() {
        let mut m = HashMap::new();
        m.insert(
            "binance_depth_btcusdt".into(),
            json!({
                "mid_price": 65000.0,
                "spread_bps": 1.54,
                "bid_depth_pct": 120.5,
                "ask_depth_pct": 95.3,
                "imbalance_top_n": 0.12,
                "bid_wall": 50.0,
                "ask_wall": 30.0,
                "vwap_bid_5": 64990.0,
                "vwap_ask_5": 65012.0,
                "ts_ms": 1700000000000_i64,
            }),
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
        let s = OrderbookSource.extract(&ctx, &MapQ(m)).await.unwrap();
        assert!((s.features["ob_spread_bps"].as_f64().unwrap() - 1.54).abs() < 1e-6);
        assert!((s.features["ob_depth_ratio"].as_f64().unwrap() - 120.5 / 95.3).abs() < 1e-6);
        assert!((s.features["ob_wall_ratio"].as_f64().unwrap() - 50.0 / 30.0).abs() < 1e-6);
        assert!(s.features.contains_key("ob_vwap_skew"));
        assert!(s.features.contains_key("ob_imbalance_top_n"));
    }

    #[tokio::test]
    async fn returns_none_when_no_snapshot() {
        let ctx = SourceContext {
            exchange: "binance",
            symbol: "BTCUSDT",
            timeframe: "15m",
            detection_id: None,
            setup_id: None,
            event_bar_ms: None,
            raw_detection: &Value::Null,
        };
        assert!(OrderbookSource
            .extract(&ctx, &MapQ(HashMap::new()))
            .await
            .is_none());
    }
}

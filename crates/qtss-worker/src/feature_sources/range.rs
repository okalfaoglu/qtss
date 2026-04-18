//! Range ConfluenceSource — FVG, Order Blocks, Liquidity Pools,
//! Equal Highs/Lows, Range Regime. Faz 9.8.AI-Yol2.
//!
//! v1 feature set:
//!   * direction               +1 bull / -1 bear / 0 neutral
//!   * sub_detector_ordinal    0 fvg, 1 ob, 2 liquidity_pool,
//!                             3 equal_levels, 4 range_regime,
//!                             5 long_setup, 9 other
//!   * pole                    +1 high-side (equal_highs, lp_high),
//!                             -1 low-side (equal_lows, lp_low), 0 other
//!   * structural_score
//!   * anchors_count / wave_span_bars / price_range_pct
//!   * zone_width_pct          |zone_high − zone_low| / mid, from raw_meta

use async_trait::async_trait;
use qtss_confluence::{ConfluenceSource, FeatureSnapshot, SourceContext, SourceQuery};
use serde_json::Value;

use super::util::{anchors_geometry, direction_from_subkind, subkind_ordinal};

pub struct RangeSource;
const SPEC_VERSION: i32 = 1;

const SUB_DETECTOR_TABLE: &[(&str, f64)] = &[
    ("fvg", 0.0),
    ("ob", 1.0),
    ("liquidity_pool", 2.0),
    ("equal_", 3.0),
    ("range_regime", 4.0),
    ("long_setup", 5.0),
];

fn pole_code(subkind: &str) -> f64 {
    if subkind.contains("_high") || subkind.contains("highs") {
        1.0
    } else if subkind.contains("_low") || subkind.contains("lows") {
        -1.0
    } else {
        0.0
    }
}

fn num_from(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
}

fn zone_width_pct(raw_meta: &Value) -> Option<f64> {
    let hi = raw_meta.get("zone_high").and_then(num_from)?;
    let lo = raw_meta.get("zone_low").and_then(num_from)?;
    let mid = (hi + lo) / 2.0;
    if mid <= 0.0 {
        return None;
    }
    Some((hi - lo).abs() / mid)
}

#[async_trait]
impl ConfluenceSource for RangeSource {
    fn key(&self) -> &'static str {
        "range"
    }

    async fn extract(
        &self,
        ctx: &SourceContext<'_>,
        _query: &dyn SourceQuery,
    ) -> Option<FeatureSnapshot> {
        let raw = ctx.raw_detection;
        let family = raw.get("family").and_then(|v| v.as_str()).unwrap_or("");
        if !family.eq_ignore_ascii_case("range") {
            return None;
        }
        let subkind = raw.get("subkind").and_then(|v| v.as_str()).unwrap_or("");

        let mut snap = FeatureSnapshot::new("range", SPEC_VERSION);
        // subkind prefixes like "bullish_fvg" / "bearish_ob" use bullish_/bearish_
        // rather than _bull/_bear, so the direction helper needs a hand.
        let direction = if subkind.starts_with("bullish") {
            1.0
        } else if subkind.starts_with("bearish") {
            -1.0
        } else {
            direction_from_subkind(subkind)
        };
        snap.insert_f64("direction", direction);
        snap.insert_f64(
            "sub_detector_ordinal",
            subkind_ordinal(subkind, SUB_DETECTOR_TABLE, 9.0),
        );
        snap.insert_f64("pole", pole_code(subkind));
        snap.insert_str("subkind", subkind);

        if let Some(score) = raw.get("structural_score").and_then(|v| v.as_f64()) {
            snap.insert_f64("structural_score", score);
        }
        if let Some(anchors) = raw.get("anchors") {
            if let Some((pct, span, n)) = anchors_geometry(anchors) {
                snap.insert_f64("price_range_pct", pct);
                snap.insert_i64("wave_span_bars", span);
                snap.insert_i64("anchors_count", n);
            }
        }
        if let Some(rm) = raw.get("raw_meta") {
            if let Some(zwp) = zone_width_pct(rm) {
                snap.insert_f64("zone_width_pct", zwp);
            }
        }
        Some(snap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct NoopQ;
    #[async_trait]
    impl SourceQuery for NoopQ {
        async fn data_snapshot(&self, _: &str) -> Option<Value> {
            None
        }
        async fn latest_regime(&self, _: &str, _: &str, _: &str) -> Option<Value> {
            None
        }
        async fn latest_tbm(&self, _: &str, _: &str, _: &str) -> Option<Value> {
            None
        }
    }

    fn ctx<'a>(raw: &'a Value) -> SourceContext<'a> {
        SourceContext {
            exchange: "binance",
            symbol: "BTCUSDT",
            timeframe: "15m",
            detection_id: None,
            setup_id: None,
            event_bar_ms: None,
            raw_detection: raw,
        }
    }

    #[tokio::test]
    async fn bullish_fvg_parses() {
        let raw = json!({
            "family": "range",
            "subkind": "bullish_fvg",
            "structural_score": 0.6,
            "raw_meta": {"zone_high": "105.0", "zone_low": "100.0"}
        });
        let s = RangeSource.extract(&ctx(&raw), &NoopQ).await.unwrap();
        assert_eq!(s.features["direction"].as_f64().unwrap(), 1.0);
        assert_eq!(s.features["sub_detector_ordinal"].as_f64().unwrap(), 0.0);
        let zwp = s.features["zone_width_pct"].as_f64().unwrap();
        assert!((zwp - 5.0 / 102.5).abs() < 1e-9);
    }

    #[tokio::test]
    async fn equal_highs_pole() {
        let raw = json!({"family":"range","subkind":"equal_highs"});
        let s = RangeSource.extract(&ctx(&raw), &NoopQ).await.unwrap();
        assert_eq!(s.features["pole"].as_f64().unwrap(), 1.0);
        assert_eq!(s.features["sub_detector_ordinal"].as_f64().unwrap(), 3.0);
    }

    #[tokio::test]
    async fn liquidity_pool_low() {
        let raw = json!({"family":"range","subkind":"liquidity_pool_low"});
        let s = RangeSource.extract(&ctx(&raw), &NoopQ).await.unwrap();
        assert_eq!(s.features["sub_detector_ordinal"].as_f64().unwrap(), 2.0);
        assert_eq!(s.features["pole"].as_f64().unwrap(), -1.0);
    }
}

//! Harmonic ConfluenceSource — XABCD / Gartley / Bat / Butterfly / Crab /
//! Cypher / Shark pattern features. Faz 9.8.AI-Yol2.
//!
//! v1 feature set:
//!   * direction              +1 bull / -1 bear
//!   * pattern_kind_ordinal   0 gartley, 1 bat, 2 alt_bat, 3 butterfly,
//!                            4 crab, 5 deep_crab, 6 cypher, 7 shark, 9 other
//!   * structural_score       detector 0..1
//!   * anchors_count          pivot count (XABCD → 5)
//!   * wave_span_bars         max_bar_index − min_bar_index
//!   * price_range_pct        (max − min price) / min
//!   * r_ab, r_bc, r_cd, r_ad Fibonacci leg ratios (|B-A|/|X-A| etc.)
//!                            present only when 5+ anchors available
//!
//! Anchors convention from qtss-harmonic: `[X, A, B, C, D]` in order.

use async_trait::async_trait;
use qtss_confluence::{ConfluenceSource, FeatureSnapshot, SourceContext, SourceQuery};
use serde_json::Value;

use super::util::{anchors_geometry, direction_from_subkind, subkind_ordinal};

pub struct HarmonicSource;
const SPEC_VERSION: i32 = 1;

const PATTERN_TABLE: &[(&str, f64)] = &[
    ("gartley", 0.0),
    ("alt_bat", 2.0),
    ("deep_crab", 5.0),
    ("bat", 1.0),
    ("butterfly", 3.0),
    ("crab", 4.0),
    ("cypher", 6.0),
    ("shark", 7.0),
];

/// Price of an anchor tolerating both string and f64 JSON.
fn anchor_price(a: &Value) -> Option<f64> {
    a.get("price").and_then(|v| {
        v.as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| v.as_f64())
    })
}

/// XABCD leg ratios computed from the first 5 anchors in canonical order.
fn xabcd_ratios(anchors: &Value) -> Option<(f64, f64, f64, f64)> {
    let arr = anchors.as_array()?;
    if arr.len() < 5 {
        return None;
    }
    let prices: Vec<f64> = arr.iter().take(5).filter_map(anchor_price).collect();
    if prices.len() < 5 {
        return None;
    }
    let (x, a, b, c, d) = (prices[0], prices[1], prices[2], prices[3], prices[4]);
    let xa = (a - x).abs();
    let ab = (b - a).abs();
    let bc = (c - b).abs();
    let cd = (d - c).abs();
    let ad = (d - a).abs();
    if xa == 0.0 || ab == 0.0 || bc == 0.0 {
        return None;
    }
    // Standard harmonic ratio definitions:
    //   r_ab = AB/XA, r_bc = BC/AB, r_cd = CD/BC, r_ad = AD/XA
    Some((ab / xa, bc / ab, cd / bc, ad / xa))
}

#[async_trait]
impl ConfluenceSource for HarmonicSource {
    fn key(&self) -> &'static str {
        "harmonic"
    }

    async fn extract(
        &self,
        ctx: &SourceContext<'_>,
        _query: &dyn SourceQuery,
    ) -> Option<FeatureSnapshot> {
        let raw = ctx.raw_detection;
        let family = raw.get("family").and_then(|v| v.as_str()).unwrap_or("");
        if !family.eq_ignore_ascii_case("harmonic") {
            return None;
        }
        let subkind = raw.get("subkind").and_then(|v| v.as_str()).unwrap_or("");

        let mut snap = FeatureSnapshot::new("harmonic", SPEC_VERSION);
        snap.insert_f64("direction", direction_from_subkind(subkind));
        snap.insert_f64(
            "pattern_kind_ordinal",
            subkind_ordinal(subkind, PATTERN_TABLE, 9.0),
        );
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
            if let Some((r_ab, r_bc, r_cd, r_ad)) = xabcd_ratios(anchors) {
                snap.insert_f64("r_ab", r_ab);
                snap.insert_f64("r_bc", r_bc);
                snap.insert_f64("r_cd", r_cd);
                snap.insert_f64("r_ad", r_ad);
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
            timeframe: "1h",
            detection_id: None,
            setup_id: None,
            event_bar_ms: None,
            raw_detection: raw,
        }
    }

    #[tokio::test]
    async fn deep_crab_ordered_before_crab() {
        let raw = json!({"family":"harmonic","subkind":"deep_crab_bull"});
        let s = HarmonicSource.extract(&ctx(&raw), &NoopQ).await.unwrap();
        assert_eq!(s.features["pattern_kind_ordinal"].as_f64().unwrap(), 5.0);
    }

    #[tokio::test]
    async fn xabcd_ratios_computed() {
        let raw = json!({
            "family": "harmonic",
            "subkind": "gartley_bull",
            "structural_score": 0.9,
            "anchors": [
                {"bar_index": 0,  "price": "100.0"},  // X
                {"bar_index": 10, "price": "150.0"},  // A: XA = 50
                {"bar_index": 20, "price": "120.0"},  // B: AB = 30 → r_ab=0.6
                {"bar_index": 30, "price": "138.0"},  // C: BC = 18 → r_bc=0.6
                {"bar_index": 40, "price": "109.0"}   // D: CD = 29 → r_cd≈1.611
            ]
        });
        let s = HarmonicSource.extract(&ctx(&raw), &NoopQ).await.unwrap();
        assert!((s.features["r_ab"].as_f64().unwrap() - 0.6).abs() < 1e-9);
        assert!((s.features["r_bc"].as_f64().unwrap() - 0.6).abs() < 1e-9);
        assert!((s.features["r_cd"].as_f64().unwrap() - 29.0 / 18.0).abs() < 1e-9);
        assert_eq!(s.features["anchors_count"].as_i64().unwrap(), 5);
    }

    #[tokio::test]
    async fn ignores_non_harmonic() {
        let raw = json!({"family": "elliott", "subkind": "impulse_5_bull"});
        assert!(HarmonicSource.extract(&ctx(&raw), &NoopQ).await.is_none());
    }
}

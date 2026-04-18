//! Elliott ConfluenceSource — structural features for the `elliott` family.
//!
//! Faz 9.8.AI-Yol2. Feeds the LightGBM meta-model with the detector's
//! own evidence so AI stops being blind to the wave count that produced
//! the setup. Dispatch is pure data (subkind → numeric codes), no
//! if/else tree (CLAUDE.md #1).
//!
//! v1 feature set:
//!   * direction              +1 bull / -1 bear / 0 neutral
//!   * pattern_kind_ordinal   0=impulse, 1=diagonal, 2=flat, 3=zigzag,
//!                            4=triangle, 5=combination, 6=other
//!   * extended_wave          0 none / 1 w1 / 3 w3 / 5 w5 / -1 truncated
//!   * anchors_count          i64 pivot count in the wave
//!   * structural_score       detector 0..1
//!   * wave_span_bars         last_anchor_idx − first_anchor_idx (i64)
//!   * price_range_pct        (max − min anchor price) / min
//!   * projected_count        len(raw_meta.projected_anchors)
//!   * sub_wave_count         len(raw_meta.sub_wave_anchors)

use async_trait::async_trait;
use qtss_confluence::{ConfluenceSource, FeatureSnapshot, SourceContext, SourceQuery};
use serde_json::Value;

pub struct ElliottSource;

const SPEC_VERSION: i32 = 1;

/// Dispatch table: pattern family by subkind substring.
/// Kept as data so new Elliott shapes only need a row here.
const PATTERN_TABLE: &[(&str, f64)] = &[
    ("impulse", 0.0),
    ("ending_diagonal", 1.0),
    ("leading_diagonal", 1.0),
    ("flat", 2.0),
    ("zigzag", 3.0),
    ("triangle", 4.0),
    ("combination", 5.0),
];

fn pattern_ordinal(subkind: &str) -> f64 {
    PATTERN_TABLE
        .iter()
        .find(|(needle, _)| subkind.contains(needle))
        .map(|(_, ord)| *ord)
        .unwrap_or(6.0)
}

fn direction_code(subkind: &str) -> f64 {
    if subkind.ends_with("_bull") {
        1.0
    } else if subkind.ends_with("_bear") {
        -1.0
    } else {
        0.0
    }
}

/// -1 truncated, 1/3/5 extension target wave, 0 otherwise.
fn extended_wave(subkind: &str) -> f64 {
    const TABLE: &[(&str, f64)] = &[
        ("truncated", -1.0),
        ("w1_extended", 1.0),
        ("w3_extended", 3.0),
        ("w5_extended", 5.0),
    ];
    TABLE
        .iter()
        .find(|(needle, _)| subkind.contains(needle))
        .map(|(_, v)| *v)
        .unwrap_or(0.0)
}

fn anchors_price_stats(anchors: &Value) -> Option<(f64, i64, i64)> {
    let arr = anchors.as_array()?;
    if arr.is_empty() {
        return None;
    }
    let mut min_price = f64::INFINITY;
    let mut max_price = f64::NEG_INFINITY;
    let mut min_idx = i64::MAX;
    let mut max_idx = i64::MIN;
    for a in arr {
        let price = a
            .get("price")
            .and_then(|v| v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64()));
        if let Some(p) = price {
            if p < min_price {
                min_price = p;
            }
            if p > max_price {
                max_price = p;
            }
        }
        if let Some(idx) = a.get("bar_index").and_then(|v| v.as_i64()) {
            if idx < min_idx {
                min_idx = idx;
            }
            if idx > max_idx {
                max_idx = idx;
            }
        }
    }
    if !min_price.is_finite() || !max_price.is_finite() || min_price == 0.0 {
        return None;
    }
    let span = if max_idx > min_idx { max_idx - min_idx } else { 0 };
    let range_pct = (max_price - min_price) / min_price;
    Some((range_pct, span, arr.len() as i64))
}

#[async_trait]
impl ConfluenceSource for ElliottSource {
    fn key(&self) -> &'static str {
        "elliott"
    }

    async fn extract(
        &self,
        ctx: &SourceContext<'_>,
        _query: &dyn SourceQuery,
    ) -> Option<FeatureSnapshot> {
        let raw = ctx.raw_detection;
        let family = raw.get("family").and_then(|v| v.as_str()).unwrap_or("");
        if !family.eq_ignore_ascii_case("elliott") {
            return None;
        }
        let subkind = raw.get("subkind").and_then(|v| v.as_str()).unwrap_or("");
        let raw_meta = raw.get("raw_meta").cloned().unwrap_or(Value::Null);

        let mut snap = FeatureSnapshot::new("elliott", SPEC_VERSION);
        snap.insert_f64("direction", direction_code(subkind));
        snap.insert_f64("pattern_kind_ordinal", pattern_ordinal(subkind));
        snap.insert_f64("extended_wave", extended_wave(subkind));
        snap.insert_str("subkind", subkind);

        if let Some(score) = raw.get("structural_score").and_then(|v| v.as_f64()) {
            snap.insert_f64("structural_score", score);
        }

        if let Some(anchors) = raw.get("anchors") {
            if let Some((range_pct, span, n)) = anchors_price_stats(anchors) {
                snap.insert_f64("price_range_pct", range_pct);
                snap.insert_i64("wave_span_bars", span);
                snap.insert_i64("anchors_count", n);
            }
        }

        if let Some(proj) = raw_meta.get("projected_anchors").and_then(|v| v.as_array()) {
            snap.insert_i64("projected_count", proj.len() as i64);
        }
        if let Some(subw) = raw_meta.get("sub_wave_anchors").and_then(|v| v.as_array()) {
            snap.insert_i64("sub_wave_count", subw.len() as i64);
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
    async fn ignores_non_elliott() {
        let raw = json!({"family": "wyckoff", "subkind": "phase_c"});
        assert!(ElliottSource.extract(&ctx(&raw), &NoopQ).await.is_none());
    }

    #[tokio::test]
    async fn impulse_w3_extended_bull() {
        let raw = json!({
            "family": "elliott",
            "subkind": "impulse_w3_extended_bull",
            "structural_score": 0.82,
            "anchors": [
                {"bar_index": 100, "price": "100.0"},
                {"bar_index": 110, "price": "120.0"},
                {"bar_index": 120, "price": "108.0"},
                {"bar_index": 140, "price": "150.0"}
            ],
            "raw_meta": {
                "projected_anchors": [{"price": "160.0"}, {"price": "170.0"}],
                "sub_wave_anchors": [{}, {}, {}]
            }
        });
        let s = ElliottSource.extract(&ctx(&raw), &NoopQ).await.unwrap();
        assert_eq!(s.source, "elliott");
        assert_eq!(s.features["direction"].as_f64().unwrap(), 1.0);
        assert_eq!(s.features["pattern_kind_ordinal"].as_f64().unwrap(), 0.0);
        assert_eq!(s.features["extended_wave"].as_f64().unwrap(), 3.0);
        assert_eq!(s.features["anchors_count"].as_i64().unwrap(), 4);
        assert_eq!(s.features["wave_span_bars"].as_i64().unwrap(), 40);
        assert_eq!(s.features["projected_count"].as_i64().unwrap(), 2);
        assert_eq!(s.features["sub_wave_count"].as_i64().unwrap(), 3);
        let range_pct = s.features["price_range_pct"].as_f64().unwrap();
        assert!((range_pct - 0.5).abs() < 1e-6);
    }

    #[tokio::test]
    async fn zigzag_bear_classifies() {
        let raw = json!({
            "family": "elliott",
            "subkind": "zigzag_abc_bear",
            "structural_score": 0.55
        });
        let s = ElliottSource.extract(&ctx(&raw), &NoopQ).await.unwrap();
        assert_eq!(s.features["direction"].as_f64().unwrap(), -1.0);
        assert_eq!(s.features["pattern_kind_ordinal"].as_f64().unwrap(), 3.0);
    }
}

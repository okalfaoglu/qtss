//! Classical ConfluenceSource — H&S, double top/bottom, wedges,
//! channels, triangles, rectangles, diamonds, rounding tops.
//! Faz 9.8.AI-Yol2.
//!
//! v1 feature set:
//!   * direction              +1 bull / -1 bear / 0 neutral
//!   * shape_kind_ordinal     0 triangle, 1 wedge, 2 channel,
//!                            3 h_and_s, 4 inv_h_and_s, 5 double_top,
//!                            6 double_bottom, 7 diamond, 8 rectangle,
//!                            9 rounding, 10 other
//!   * structural_score
//!   * anchors_count / wave_span_bars / price_range_pct
//!
//! Shape dispatch is a data table — add a row, no code change
//! (CLAUDE.md #1).

use async_trait::async_trait;
use qtss_confluence::{ConfluenceSource, FeatureSnapshot, SourceContext, SourceQuery};

use super::util::{anchors_geometry, direction_from_subkind, subkind_ordinal};

pub struct ClassicalSource;
const SPEC_VERSION: i32 = 1;

// Order matters: more specific substrings first ("inverse_head_and_shoulders"
// before "head_and_shoulders", "double_top" before "top").
const SHAPE_TABLE: &[(&str, f64)] = &[
    ("inverse_head_and_shoulders", 4.0),
    ("head_and_shoulders", 3.0),
    ("double_top", 5.0),
    ("double_bottom", 6.0),
    ("ascending_triangle", 0.0),
    ("descending_triangle", 0.0),
    ("symmetrical_triangle", 0.0),
    ("rising_wedge", 1.0),
    ("falling_wedge", 1.0),
    ("ascending_channel", 2.0),
    ("descending_channel", 2.0),
    ("diamond", 7.0),
    ("rectangle", 8.0),
    ("rounding", 9.0),
    // Faz 10 Aşama 1 — yeni formasyon ordinals.
    ("triple_top", 11.0),
    ("triple_bottom", 12.0),
    ("broadening_top", 13.0),
    ("broadening_bottom", 14.0),
    ("broadening_triangle", 15.0),
    ("v_top", 16.0),
    ("v_bottom", 17.0),
    ("measured_move_abcd", 18.0),
];

#[async_trait]
impl ConfluenceSource for ClassicalSource {
    fn key(&self) -> &'static str {
        "classical"
    }

    async fn extract(
        &self,
        ctx: &SourceContext<'_>,
        _query: &dyn SourceQuery,
    ) -> Option<FeatureSnapshot> {
        let raw = ctx.raw_detection;
        let family = raw.get("family").and_then(|v| v.as_str()).unwrap_or("");
        if !family.eq_ignore_ascii_case("classical") {
            return None;
        }
        let subkind = raw.get("subkind").and_then(|v| v.as_str()).unwrap_or("");

        let mut snap = FeatureSnapshot::new("classical", SPEC_VERSION);
        snap.insert_f64("direction", direction_from_subkind(subkind));
        snap.insert_f64(
            "shape_kind_ordinal",
            subkind_ordinal(subkind, SHAPE_TABLE, 10.0),
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
        }
        Some(snap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

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
            timeframe: "4h",
            detection_id: None,
            setup_id: None,
            event_bar_ms: None,
            raw_detection: raw,
        }
    }

    #[tokio::test]
    async fn inverse_head_and_shoulders_before_head_and_shoulders() {
        let raw = json!({"family":"classical","subkind":"inverse_head_and_shoulders_bull"});
        let s = ClassicalSource.extract(&ctx(&raw), &NoopQ).await.unwrap();
        // 4.0 (inverse H&S), not 3.0 (H&S)
        assert_eq!(s.features["shape_kind_ordinal"].as_f64().unwrap(), 4.0);
        assert_eq!(s.features["direction"].as_f64().unwrap(), 1.0);
    }

    #[tokio::test]
    async fn rectangle_neutral_direction() {
        let raw = json!({"family":"classical","subkind":"rectangle_neutral"});
        let s = ClassicalSource.extract(&ctx(&raw), &NoopQ).await.unwrap();
        assert_eq!(s.features["direction"].as_f64().unwrap(), 0.0);
        assert_eq!(s.features["shape_kind_ordinal"].as_f64().unwrap(), 8.0);
    }

    #[tokio::test]
    async fn ignores_non_classical() {
        let raw = json!({"family":"harmonic","subkind":"gartley_bull"});
        assert!(ClassicalSource.extract(&ctx(&raw), &NoopQ).await.is_none());
    }
}

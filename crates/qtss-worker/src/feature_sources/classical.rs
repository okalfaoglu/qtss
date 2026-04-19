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
use qtss_setup_engine::types::Direction;
use serde_json::Value;

use super::util::{anchors_geometry, direction_from_subkind, subkind_ordinal};
use crate::v2_setup_loop::compute_structural_targets_raw;

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

        // Faz 10 — structural target features. LightGBM sees the
        // normalised TP geometry the setup engine will use, so the
        // model can learn "formations whose first target is ~1R away
        // + has a secondary 1.6R target" as a pattern in its own
        // right. Raw prices would leak symbol scale; we emit counts +
        // R-multiples only.
        append_target_features(&mut snap, raw, subkind);
        Some(snap)
    }
}

/// Fold target geometry into the feature snapshot. Pure — only reads
/// JSON and the shared `compute_structural_targets_raw` helper.
fn append_target_features(snap: &mut FeatureSnapshot, raw: &Value, subkind: &str) {
    // Direction comes from the same keyword dispatch the scorer uses.
    let dir_sign = direction_from_subkind(subkind);
    let direction = if dir_sign > 0.0 {
        Direction::Long
    } else if dir_sign < 0.0 {
        Direction::Short
    } else {
        Direction::Neutral
    };
    if matches!(direction, Direction::Neutral) {
        snap.insert_f64("target_count", 0.0);
        snap.insert_f64("has_structural_targets", 0.0);
        return;
    }
    let family = raw.get("family").and_then(|v| v.as_str()).unwrap_or("");
    let inv_price = raw
        .get("invalidation_price")
        .and_then(|v| v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64()))
        .unwrap_or(0.0);
    let anchors: Vec<Value> = raw
        .get("anchors")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    let empty_meta = Value::Null;
    let raw_meta = raw.get("raw_meta").unwrap_or(&empty_meta);

    let targets = compute_structural_targets_raw(
        &anchors, subkind, family, direction, inv_price, raw_meta,
    );
    snap.insert_f64("target_count", targets.len() as f64);
    snap.insert_f64(
        "has_structural_targets",
        if targets.is_empty() { 0.0 } else { 1.0 },
    );

    // R-multiple distances relative to |entry - SL|. Use
    // invalidation_price as SL proxy and the price of the earliest
    // anchor as the "entry reference" — matches the setup engine's
    // structural guard semantics. Emits up to the first two targets
    // (TP1, TP2) because the GUI only ratchets through two levels.
    if inv_price > 0.0 && !targets.is_empty() {
        let entry_ref = anchors
            .last()
            .and_then(|a| {
                a.get("price")
                    .and_then(|v| v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64()))
            })
            .unwrap_or(inv_price);
        let r = (entry_ref - inv_price).abs();
        if r > 0.0 {
            if let Some(t1) = targets.first() {
                snap.insert_f64("target_1_r", (t1.price - entry_ref).abs() / r);
                snap.insert_f64("target_1_weight", t1.weight);
            }
            if let Some(t2) = targets.get(1) {
                snap.insert_f64("target_2_r", (t2.price - entry_ref).abs() / r);
                snap.insert_f64("target_2_weight", t2.weight);
            }
        }
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
    async fn emits_target_features_for_double_bottom() {
        // Double bottom: 3 anchors [low1, neck, low2]. Expect
        // compute_structural_targets to produce 2 targets, and the
        // feature snapshot to carry target_count / has_structural_targets
        // + target_1_r relative to R = |entry - inv|.
        let raw = json!({
            "family": "classical",
            "subkind": "double_bottom_bull",
            "invalidation_price": "100.0",
            "anchors": [
                {"price": "100.0"},  // low 1
                {"price": "110.0"},  // neckline
                {"price": "100.0"}   // low 2 (also entry_ref via last())
            ],
            "raw_meta": {}
        });
        let s = ClassicalSource.extract(&ctx(&raw), &NoopQ).await.unwrap();
        assert_eq!(s.features["target_count"].as_f64().unwrap(), 2.0);
        assert_eq!(s.features["has_structural_targets"].as_f64().unwrap(), 1.0);
        // entry_ref = last anchor = 100, inv = 100 → R=0 → target_1_r omitted.
        // Adjust entry_ref by using a different last anchor to test R math.
        let raw2 = json!({
            "family": "classical",
            "subkind": "double_bottom_bull",
            "invalidation_price": "95.0",
            "anchors": [
                {"price": "100.0"},  // low 1
                {"price": "110.0"},  // neckline
                {"price": "105.0"}   // entry_ref (last)
            ],
            "raw_meta": {}
        });
        let s2 = ClassicalSource.extract(&ctx(&raw2), &NoopQ).await.unwrap();
        // R = |105 - 95| = 10. Target1 = neck + sign * height = 110 + 1*10 = 120.
        // target_1_r = |120 - 105| / 10 = 1.5.
        assert!((s2.features["target_1_r"].as_f64().unwrap() - 1.5).abs() < 1e-6);
    }

    #[tokio::test]
    async fn triple_top_emits_targets() {
        // Previously: triple_top was not in CLASSICAL_HEIGHT_PREFIXES
        // → empty target vec → ATR fallback. Now it has a dedicated
        // neckline-MM branch.
        let raw = json!({
            "family": "classical",
            "subkind": "triple_top_bear",
            "invalidation_price": "105.0",
            "anchors": [
                {"price": "110.0"},  // P1
                {"price": "100.0"},  // V1
                {"price": "110.0"},  // P2
                {"price": "100.0"},  // V2
                {"price": "108.0"}   // P3 (entry_ref)
            ],
            "raw_meta": {}
        });
        let s = ClassicalSource.extract(&ctx(&raw), &NoopQ).await.unwrap();
        assert_eq!(s.features["target_count"].as_f64().unwrap(), 2.0);
        assert_eq!(s.features["has_structural_targets"].as_f64().unwrap(), 1.0);
    }

    #[tokio::test]
    async fn ignores_non_classical() {
        let raw = json!({"family":"harmonic","subkind":"gartley_bull"});
        assert!(ClassicalSource.extract(&ctx(&raw), &NoopQ).await.is_none());
    }
}

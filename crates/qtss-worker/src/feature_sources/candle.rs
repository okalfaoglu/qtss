//! Candle ConfluenceSource — Japanese candlestick patterns. Faz 10 Aşama 3.
//!
//! v1 feature set:
//!   * direction              +1 bull / -1 bear / 0 neutral
//!   * candle_kind_ordinal    stable id per candle family
//!   * structural_score
//!
//! Family dispatch is a data table — add a row, no code change (CLAUDE.md #1).

use async_trait::async_trait;
use qtss_confluence::{ConfluenceSource, FeatureSnapshot, SourceContext, SourceQuery};

use super::util::{direction_from_subkind, subkind_ordinal};

pub struct CandleSource;
const SPEC_VERSION: i32 = 1;

// Order matters: more specific first. Ordinals are stable and grouped
// by family (3-bar reversals first, then 2-bar, then 1-bar).
const CANDLE_TABLE: &[(&str, f64)] = &[
    ("morning_star", 30.0),
    ("evening_star", 31.0),
    ("three_white_soldiers", 32.0),
    ("three_black_crows", 33.0),
    ("three_inside_up", 34.0),
    ("three_inside_down", 35.0),
    ("three_outside_up", 36.0),
    ("three_outside_down", 37.0),
    ("engulfing", 20.0),
    ("harami", 21.0),
    ("piercing_line", 22.0),
    ("dark_cloud_cover", 23.0),
    ("tweezer_top", 24.0),
    ("tweezer_bottom", 25.0),
    ("dragonfly_doji", 10.0),
    ("gravestone_doji", 11.0),
    ("long_legged_doji", 12.0),
    ("doji", 13.0),
    ("hammer", 1.0),
    ("inverted_hammer", 2.0),
    ("hanging_man", 3.0),
    ("shooting_star", 4.0),
    ("marubozu", 5.0),
    ("spinning_top", 6.0),
];

#[async_trait]
impl ConfluenceSource for CandleSource {
    fn key(&self) -> &'static str {
        "candle"
    }

    async fn extract(
        &self,
        ctx: &SourceContext<'_>,
        _query: &dyn SourceQuery,
    ) -> Option<FeatureSnapshot> {
        let raw = ctx.raw_detection;
        let family = raw.get("family").and_then(|v| v.as_str()).unwrap_or("");
        if !family.eq_ignore_ascii_case("candle") {
            return None;
        }
        let subkind = raw.get("subkind").and_then(|v| v.as_str()).unwrap_or("");

        let mut snap = FeatureSnapshot::new("candle", SPEC_VERSION);
        snap.insert_f64("direction", direction_from_subkind(subkind));
        snap.insert_f64(
            "candle_kind_ordinal",
            subkind_ordinal(subkind, CANDLE_TABLE, 0.0),
        );
        snap.insert_str("subkind", subkind);
        if let Some(score) = raw.get("structural_score").and_then(|v| v.as_f64()) {
            snap.insert_f64("structural_score", score);
        }
        Some(snap)
    }
}

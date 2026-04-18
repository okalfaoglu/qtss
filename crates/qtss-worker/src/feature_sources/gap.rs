//! Gap ConfluenceSource — common/breakaway/runaway/exhaustion gaps and
//! island reversals. Faz 10 Aşama 2.
//!
//! v1 feature set:
//!   * direction              +1 bull / -1 bear
//!   * gap_kind_ordinal       0 common, 1 breakaway, 2 runaway,
//!                            3 exhaustion, 4 island_reversal
//!   * gap_pct                signed gap magnitude (fraction)
//!   * volume_ratio           volume / SMA(volume) at gap bar
//!   * structural_score
//!
//! Kind dispatch is a data table — add a row, no code change (CLAUDE.md #1).

use async_trait::async_trait;
use qtss_confluence::{ConfluenceSource, FeatureSnapshot, SourceContext, SourceQuery};

use super::util::{direction_from_subkind, subkind_ordinal};

pub struct GapSource;
const SPEC_VERSION: i32 = 1;

// Order matters: more specific first ("island_reversal" before any
// generic "reversal" match; explicit ordering for ordinal stability).
const GAP_TABLE: &[(&str, f64)] = &[
    ("island_reversal", 4.0),
    ("exhaustion_gap", 3.0),
    ("runaway_gap", 2.0),
    ("breakaway_gap", 1.0),
    ("common_gap", 0.0),
];

#[async_trait]
impl ConfluenceSource for GapSource {
    fn key(&self) -> &'static str {
        "gap"
    }

    async fn extract(
        &self,
        ctx: &SourceContext<'_>,
        _query: &dyn SourceQuery,
    ) -> Option<FeatureSnapshot> {
        let raw = ctx.raw_detection;
        let family = raw.get("family").and_then(|v| v.as_str()).unwrap_or("");
        if !family.eq_ignore_ascii_case("gap") {
            return None;
        }
        let subkind = raw.get("subkind").and_then(|v| v.as_str()).unwrap_or("");

        let mut snap = FeatureSnapshot::new("gap", SPEC_VERSION);
        snap.insert_f64("direction", direction_from_subkind(subkind));
        snap.insert_f64("gap_kind_ordinal", subkind_ordinal(subkind, GAP_TABLE, 0.0));
        snap.insert_str("subkind", subkind);

        if let Some(score) = raw.get("structural_score").and_then(|v| v.as_f64()) {
            snap.insert_f64("structural_score", score);
        }
        // Anchors carry a synthetic "gap_pct" pivot (price field reused as
        // the gap fraction) — surface it as a feature when present.
        if let Some(anchors) = raw.get("anchors").and_then(|v| v.as_array()) {
            for a in anchors {
                let label = a.get("label").and_then(|v| v.as_str()).unwrap_or("");
                if label == "gap_pct" {
                    if let Some(p) = a
                        .get("price")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<f64>().ok())
                        .or_else(|| a.get("price").and_then(|v| v.as_f64()))
                    {
                        snap.insert_f64("gap_pct", p);
                    }
                }
            }
            snap.insert_i64("anchors_count", anchors.len() as i64);
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
            timeframe: "1h",
            detection_id: None,
            setup_id: None,
            event_bar_ms: None,
            raw_detection: raw,
        }
    }

    #[tokio::test]
    async fn island_before_common() {
        let raw = json!({"family":"gap","subkind":"island_reversal_bear"});
        let s = GapSource.extract(&ctx(&raw), &NoopQ).await.unwrap();
        assert_eq!(s.features["gap_kind_ordinal"].as_f64().unwrap(), 4.0);
        assert_eq!(s.features["direction"].as_f64().unwrap(), -1.0);
    }

    #[tokio::test]
    async fn ignores_non_gap() {
        let raw = json!({"family":"classical","subkind":"double_top_bear"});
        assert!(GapSource.extract(&ctx(&raw), &NoopQ).await.is_none());
    }
}

//! Wyckoff ConfluenceSource — extracts structural features from the
//! `raw_detection` JSON emitted by the Wyckoff detector.
//!
//! v1 feature set (sabit anahtar, numeric değerler tercih edilir):
//!   * phase_ordinal           (A=0, B=1, C=2, D=3, E=4, else -1)
//!   * events_count            (len(events_json))
//!   * spring_fired            (bool → 0/1 via insert_bool)
//!   * utad_fired              (bool)
//!   * range_bars              (phase_b duration proxy)
//!   * last_event_age_bars     (int, -1 if unknown)
//!   * structural_score        (detector score 0..1)
//!   * is_accumulation         (bool — long bias)
//!   * is_distribution         (bool — short bias)
//!
//! Eksik alanlar insert edilmez; nan/inf filtrelenir (helper safe-cast).

use async_trait::async_trait;
use qtss_confluence::{ConfluenceSource, FeatureSnapshot, SourceContext, SourceQuery};
use serde_json::Value;

pub struct WyckoffSource;

const SPEC_VERSION: i32 = 1;

fn phase_ordinal(p: &str) -> f64 {
    match p {
        "A" | "a" | "phase_a" => 0.0,
        "B" | "b" | "phase_b" => 1.0,
        "C" | "c" | "phase_c" => 2.0,
        "D" | "d" | "phase_d" => 3.0,
        "E" | "e" | "phase_e" => 4.0,
        _ => -1.0,
    }
}

fn any_event_with_kind(events: &Value, needle: &str) -> bool {
    events
        .as_array()
        .map(|arr| {
            arr.iter().any(|e| {
                e.get("kind")
                    .and_then(|k| k.as_str())
                    .map(|s| s.eq_ignore_ascii_case(needle))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

#[async_trait]
impl ConfluenceSource for WyckoffSource {
    fn key(&self) -> &'static str {
        "wyckoff"
    }

    async fn extract(
        &self,
        ctx: &SourceContext<'_>,
        _query: &dyn SourceQuery,
    ) -> Option<FeatureSnapshot> {
        let raw = ctx.raw_detection;
        let family = raw.get("family").and_then(|v| v.as_str()).unwrap_or("");
        if !family.eq_ignore_ascii_case("wyckoff") {
            return None;
        }
        let mut snap = FeatureSnapshot::new("wyckoff", SPEC_VERSION);
        if let Some(phase) = raw.get("phase").and_then(|v| v.as_str()) {
            snap.insert_f64("phase_ordinal", phase_ordinal(phase));
            snap.insert_str("phase", phase);
        }
        if let Some(events) = raw.get("events_json").or_else(|| raw.get("events")) {
            let n = events.as_array().map(|a| a.len()).unwrap_or(0);
            snap.insert_i64("events_count", n as i64);
            snap.insert_bool("spring_fired", any_event_with_kind(events, "spring"));
            snap.insert_bool("utad_fired", any_event_with_kind(events, "utad"));
            snap.insert_bool("sos_fired", any_event_with_kind(events, "sos"));
            snap.insert_bool("sow_fired", any_event_with_kind(events, "sow"));
        }
        if let Some(score) = raw.get("structural_score").and_then(|v| v.as_f64()) {
            snap.insert_f64("structural_score", score);
        }
        if let Some(range_bars) = raw.get("range_bars").and_then(|v| v.as_i64()) {
            snap.insert_i64("range_bars", range_bars);
        }
        if let Some(acc) = raw.get("is_accumulation").and_then(|v| v.as_bool()) {
            snap.insert_bool("is_accumulation", acc);
        }
        if let Some(dist) = raw.get("is_distribution").and_then(|v| v.as_bool()) {
            snap.insert_bool("is_distribution", dist);
        }
        Some(snap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
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

    #[tokio::test]
    async fn extracts_phase_and_events() {
        let raw = json!({
            "family": "wyckoff",
            "phase": "C",
            "events_json": [{"kind": "spring"}, {"kind": "sos"}],
            "structural_score": 0.72,
            "range_bars": 40,
            "is_accumulation": true
        });
        let ctx = SourceContext {
            exchange: "binance",
            symbol: "BTCUSDT",
            timeframe: "15m",
            detection_id: None,
            setup_id: None,
            event_bar_ms: None,
            raw_detection: &raw,
        };
        let s = WyckoffSource.extract(&ctx, &NoopQ).await.unwrap();
        assert_eq!(s.source, "wyckoff");
        assert!((s.features["phase_ordinal"].as_f64().unwrap() - 2.0).abs() < 1e-9);
        assert_eq!(s.features["events_count"].as_i64().unwrap(), 2);
        assert_eq!(s.features["spring_fired"], true);
        assert_eq!(s.features["sos_fired"], true);
        assert_eq!(s.features["utad_fired"], false);
        assert_eq!(s.features["is_accumulation"], true);
    }

    #[tokio::test]
    async fn ignores_non_wyckoff() {
        let raw = json!({"family": "elliott"});
        let ctx = SourceContext {
            exchange: "binance",
            symbol: "BTCUSDT",
            timeframe: "15m",
            detection_id: None,
            setup_id: None,
            event_bar_ms: None,
            raw_detection: &raw,
        };
        assert!(WyckoffSource.extract(&ctx, &NoopQ).await.is_none());
    }
}

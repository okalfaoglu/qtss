//! TBM (Top/Bottom Mining) ConfluenceSource — Faz 9.8.AI-Yol2.
//!
//! Pulls from both the detection envelope (`raw_meta`) and the
//! `latest_tbm` query port (DB fallback) so models see pillar scores
//! even when the detector didn't stamp them into raw_meta. All fields
//! optional — no field means "skip insert" (NaN/Inf filtered by
//! FeatureSnapshot).
//!
//! v1 feature set:
//!   * direction               +1 bottom_setup / -1 top_setup
//!   * total_score             TbmScore.total
//!   * signal_ordinal          0 weak, 1 neutral, 2 strong
//!   * active_pillars          i64
//!   * pillar_<kind>_score     one per pillar (structure, momentum,
//!                             volume, orderflow, regime, …) — names
//!                             mirror detector output, filtered to
//!                             numeric values to avoid NaN noise
//!   * structural_score        carried through if present

use async_trait::async_trait;
use qtss_confluence::{ConfluenceSource, FeatureSnapshot, SourceContext, SourceQuery};
use serde_json::Value;

pub struct TbmSource;
const SPEC_VERSION: i32 = 1;

fn direction_code(subkind: &str) -> f64 {
    match subkind {
        "bottom_setup" => 1.0,
        "top_setup" => -1.0,
        _ => 0.0,
    }
}

fn signal_ordinal(signal: &str) -> f64 {
    match signal.to_ascii_lowercase().as_str() {
        "strong" => 2.0,
        "neutral" => 1.0,
        "weak" => 0.0,
        _ => -1.0,
    }
}

/// Pillars may come as either a flat `{kind: score}` map or an array of
/// `{kind, score}` objects. Normalise to (kind, score) pairs.
fn extract_pillar_scores(pillars: &Value) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    if let Some(obj) = pillars.as_object() {
        for (k, v) in obj {
            if let Some(n) = v.as_f64() {
                out.push((k.clone(), n));
            } else if let Some(s) = v.get("score").and_then(|x| x.as_f64()) {
                out.push((k.clone(), s));
            }
        }
    } else if let Some(arr) = pillars.as_array() {
        for entry in arr {
            let kind = entry.get("kind").and_then(|v| v.as_str()).map(str::to_string);
            let score = entry.get("score").and_then(|v| v.as_f64());
            if let (Some(k), Some(s)) = (kind, score) {
                out.push((k, s));
            }
        }
    }
    out
}

#[async_trait]
impl ConfluenceSource for TbmSource {
    fn key(&self) -> &'static str {
        "tbm"
    }

    async fn extract(
        &self,
        ctx: &SourceContext<'_>,
        query: &dyn SourceQuery,
    ) -> Option<FeatureSnapshot> {
        let raw = ctx.raw_detection;
        let family = raw.get("family").and_then(|v| v.as_str()).unwrap_or("");
        if !family.eq_ignore_ascii_case("tbm") {
            return None;
        }
        let subkind = raw.get("subkind").and_then(|v| v.as_str()).unwrap_or("");

        let mut snap = FeatureSnapshot::new("tbm", SPEC_VERSION);
        snap.insert_f64("direction", direction_code(subkind));
        snap.insert_str("subkind", subkind);
        if let Some(score) = raw.get("structural_score").and_then(|v| v.as_f64()) {
            snap.insert_f64("structural_score", score);
        }

        // Prefer detector-embedded metrics; fall back to latest_tbm row
        // if the raw_meta envelope is sparse.
        let raw_meta = raw.get("raw_meta").cloned().unwrap_or(Value::Null);
        let tbm_fallback = query
            .latest_tbm(ctx.exchange, ctx.symbol, ctx.timeframe)
            .await;
        let metrics = if raw_meta.get("total_score").is_some()
            || raw_meta.get("pillars").is_some()
            || raw_meta.get("signal").is_some()
        {
            raw_meta
        } else {
            tbm_fallback.unwrap_or(Value::Null)
        };

        if let Some(total) = metrics.get("total_score").and_then(|v| v.as_f64()) {
            snap.insert_f64("total_score", total);
        }
        if let Some(sig) = metrics.get("signal").and_then(|v| v.as_str()) {
            snap.insert_f64("signal_ordinal", signal_ordinal(sig));
            snap.insert_str("signal", sig);
        }
        if let Some(n) = metrics.get("active_pillars").and_then(|v| v.as_i64()) {
            snap.insert_i64("active_pillars", n);
        }
        if let Some(pillars) = metrics.get("pillars") {
            for (kind, score) in extract_pillar_scores(pillars) {
                snap.insert_f64(&format!("pillar_{}_score", kind), score);
            }
        }

        Some(snap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct StubQ(Option<Value>);
    #[async_trait]
    impl SourceQuery for StubQ {
        async fn data_snapshot(&self, _: &str) -> Option<Value> {
            None
        }
        async fn latest_regime(&self, _: &str, _: &str, _: &str) -> Option<Value> {
            None
        }
        async fn latest_tbm(&self, _: &str, _: &str, _: &str) -> Option<Value> {
            self.0.clone()
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
    async fn uses_raw_meta_pillars_map() {
        let raw = json!({
            "family": "tbm",
            "subkind": "bottom_setup",
            "structural_score": 0.7,
            "raw_meta": {
                "total_score": 72.5,
                "signal": "strong",
                "active_pillars": 4,
                "pillars": {"structure": 18.0, "momentum": 22.0, "volume": 12.5}
            }
        });
        let q = StubQ(None);
        let s = TbmSource.extract(&ctx(&raw), &q).await.unwrap();
        assert_eq!(s.features["direction"].as_f64().unwrap(), 1.0);
        assert_eq!(s.features["signal_ordinal"].as_f64().unwrap(), 2.0);
        assert_eq!(s.features["active_pillars"].as_i64().unwrap(), 4);
        assert_eq!(s.features["pillar_structure_score"].as_f64().unwrap(), 18.0);
        assert_eq!(s.features["pillar_momentum_score"].as_f64().unwrap(), 22.0);
    }

    #[tokio::test]
    async fn falls_back_to_latest_tbm() {
        let raw = json!({"family":"tbm","subkind":"top_setup","raw_meta": {}});
        let q = StubQ(Some(json!({
            "total_score": 61.0,
            "signal": "neutral",
            "pillars": [{"kind":"structure","score":14.0}]
        })));
        let s = TbmSource.extract(&ctx(&raw), &q).await.unwrap();
        assert_eq!(s.features["direction"].as_f64().unwrap(), -1.0);
        assert_eq!(s.features["total_score"].as_f64().unwrap(), 61.0);
        assert_eq!(s.features["pillar_structure_score"].as_f64().unwrap(), 14.0);
    }

    #[tokio::test]
    async fn ignores_non_tbm() {
        let raw = json!({"family":"elliott","subkind":"impulse_5_bull"});
        let q = StubQ(None);
        assert!(TbmSource.extract(&ctx(&raw), &q).await.is_none());
    }
}

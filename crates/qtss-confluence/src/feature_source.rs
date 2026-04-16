//! Faz 9.0.2 — `ConfluenceSource` trait + FeatureSnapshot envelope.
//!
//! Her AI-ilgili veri kaynağı (Wyckoff, Elliott, Classical, TBM, Regime,
//! Derivatives, Session) bu trait'i impl eder. Worker orchestrator her
//! detection anında registry'deki tüm enabled source'ları çağırır,
//! dönen `FeatureSnapshot`'ları tek-tek `qtss_features_snapshot` tablosuna
//! yazar.
//!
//! CLAUDE.md #1: dispatch tablosu / polimorfizm — yeni kaynak ekleme tek
//! `impl ConfluenceSource for NewSource`; orchestrator kodu değişmez.
//!
//! CLAUDE.md #4: trait, DB veya HTTP bilmez — asenkron context `&SourceContext`
//! ile pas geçer ve extractor'lar sadece kendilerine pas geçilen veriden
//! feature üretir. Actual storage/query adapter worker katmanında.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Envelope written to `qtss_features_snapshot`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeatureSnapshot {
    /// Source key, e.g. "wyckoff", "derivatives". Matches the `source`
    /// column.
    pub source: &'static str,
    /// Spec version — bumped when the schema of `features` breaks.
    pub spec_version: i32,
    /// Flat feature map. Numeric values preferred (f64 cast); categorical
    /// should be encoded up-front (one-hot / ordinal).
    pub features: BTreeMap<String, Value>,
    /// Arbitrary diagnostic metadata (skip reason, last event_kind, etc.).
    #[serde(default)]
    pub meta: BTreeMap<String, Value>,
}

impl FeatureSnapshot {
    pub fn new(source: &'static str, spec_version: i32) -> Self {
        Self {
            source,
            spec_version,
            features: BTreeMap::new(),
            meta: BTreeMap::new(),
        }
    }

    pub fn insert_f64(&mut self, key: impl Into<String>, value: f64) {
        if value.is_finite() {
            self.features.insert(
                key.into(),
                Value::Number(
                    serde_json::Number::from_f64(value).unwrap_or_else(|| 0.into()),
                ),
            );
        }
    }

    pub fn insert_bool(&mut self, key: impl Into<String>, value: bool) {
        self.features.insert(key.into(), Value::Bool(value));
    }

    pub fn insert_str(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.features.insert(key.into(), Value::String(value.into()));
    }

    pub fn insert_i64(&mut self, key: impl Into<String>, value: i64) {
        self.features.insert(key.into(), Value::Number(value.into()));
    }

    pub fn into_json(self) -> (Value, Value) {
        (
            serde_json::to_value(&self.features).unwrap_or(Value::Null),
            serde_json::to_value(&self.meta).unwrap_or(Value::Null),
        )
    }
}

/// Minimal context handed to every extractor. Worker fills this from the
/// detection event; extractors are expected to pull whatever they need
/// out of `raw` (opaque JSON) or use ambient DB via their own adapter
/// (wrapped in an async trait object passed through `DynSourceQuery`).
#[derive(Debug, Clone)]
pub struct SourceContext<'a> {
    pub exchange: &'a str,
    pub symbol: &'a str,
    pub timeframe: &'a str,
    pub detection_id: Option<uuid::Uuid>,
    pub setup_id: Option<uuid::Uuid>,
    pub event_bar_ms: Option<i64>,
    pub raw_detection: &'a Value,
}

/// Adapter for sources that need live DB access (derivatives pulls from
/// `data_snapshots`, regime pulls from `qtss_v2_regime_snapshots`, etc.).
/// Worker supplies a concrete impl; the trait keeps qtss-confluence DB-free.
#[async_trait::async_trait]
pub trait SourceQuery: Send + Sync {
    /// Latest `data_snapshots.response_json` for a given key.
    async fn data_snapshot(&self, key: &str) -> Option<Value>;
    /// Latest regime snapshot JSON for (exchange, symbol, timeframe).
    async fn latest_regime(&self, exchange: &str, symbol: &str, timeframe: &str) -> Option<Value>;
    /// Latest TBM metric row JSON.
    async fn latest_tbm(&self, exchange: &str, symbol: &str, timeframe: &str) -> Option<Value>;
}

/// Dispatch contract. Each source implementation is **stateless** — all
/// state travels via `SourceContext` + `SourceQuery`. Registry is a
/// `&'static [&'static dyn ConfluenceSource]` assembled at worker boot.
#[async_trait::async_trait]
pub trait ConfluenceSource: Send + Sync {
    fn key(&self) -> &'static str;

    /// Config key read from `ai.feature_store.sources.<key>.enabled`.
    fn config_enabled_key(&self) -> String {
        format!("feature_store.sources.{}.enabled", self.key())
    }

    /// Extract features. Returning `None` is a valid "skip" (logged at
    /// debug, not warn).
    async fn extract(
        &self,
        ctx: &SourceContext<'_>,
        query: &dyn SourceQuery,
    ) -> Option<FeatureSnapshot>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_insert_numeric_safe() {
        let mut s = FeatureSnapshot::new("test", 1);
        s.insert_f64("finite", 1.5);
        s.insert_f64("nan", f64::NAN);
        s.insert_f64("inf", f64::INFINITY);
        assert!(s.features.contains_key("finite"));
        assert!(!s.features.contains_key("nan"));
        assert!(!s.features.contains_key("inf"));
    }

    #[test]
    fn snapshot_serializes_to_json() {
        let mut s = FeatureSnapshot::new("wyckoff", 1);
        s.insert_f64("phase_b_bars", 23.0);
        s.insert_bool("spring_fired", true);
        s.insert_str("phase", "C");
        let (features, _) = s.into_json();
        let obj = features.as_object().unwrap();
        assert!(obj.contains_key("phase_b_bars"));
        assert!(obj.contains_key("spring_fired"));
        assert_eq!(obj["phase"], "C");
    }

    #[test]
    fn config_key_format() {
        struct S;
        #[async_trait::async_trait]
        impl ConfluenceSource for S {
            fn key(&self) -> &'static str {
                "mysource"
            }
            async fn extract(
                &self,
                _: &SourceContext<'_>,
                _: &dyn SourceQuery,
            ) -> Option<FeatureSnapshot> {
                None
            }
        }
        assert_eq!(S.config_enabled_key(), "feature_store.sources.mysource.enabled");
    }
}

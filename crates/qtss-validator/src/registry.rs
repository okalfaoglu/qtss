//! Dispatch registry. Per-family lookup + default set with every
//! validator registered.

use crate::config::ValidatorConfig;
use crate::validators::{
    ClassicalValidator, GapValidator, HarmonicValidator, MotiveValidator, OrbValidator,
    RangeValidator, SmcValidator, Validator,
};
use crate::verdict::{InvalidationReason, ValidatorVerdict};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Minimal detection-row shape the validator reads. Worker adapters
/// project the SQL row into this struct so validators stay DB-free.
#[derive(Debug, Clone)]
pub struct DetectionRow {
    pub id: String,
    pub family: String,
    pub subkind: String,
    pub direction: i16,
    pub anchors: Value,
    pub raw_meta: Value,
}

pub struct ValidatorRegistry {
    map: HashMap<String, Arc<dyn Validator>>,
}

impl ValidatorRegistry {
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }
    pub fn register(&mut self, v: Arc<dyn Validator>) {
        self.map.insert(v.family().to_string(), v);
    }
    pub fn validate(
        &self,
        row: &DetectionRow,
        price: f64,
        atr: f64,
        cfg: &ValidatorConfig,
    ) -> (ValidatorVerdict, Option<InvalidationReason>) {
        let Some(v) = self.map.get(&row.family) else {
            return (ValidatorVerdict::Hold, None);
        };
        v.validate(row, price, atr, cfg)
    }
}

impl Default for ValidatorRegistry {
    fn default() -> Self {
        default_registry()
    }
}

/// Registry with every shipped validator. Adding a new family is one
/// `.register()` call (CLAUDE.md #1).
pub fn default_registry() -> ValidatorRegistry {
    let mut r = ValidatorRegistry::new();
    r.register(Arc::new(HarmonicValidator));
    r.register(Arc::new(ClassicalValidator));
    r.register(Arc::new(RangeValidator));
    r.register(Arc::new(GapValidator));
    r.register(Arc::new(MotiveValidator));
    r.register(Arc::new(SmcValidator));
    r.register(Arc::new(OrbValidator));
    r
}

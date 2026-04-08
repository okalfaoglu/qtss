//! Target engine — runs registered methods, clusters nearby targets,
//! caps the result by weight.

use crate::config::TargetEngineConfig;
use crate::error::TargetEngineResult;
use crate::methods::TargetMethodCalc;
use qtss_domain::v2::detection::{Detection, Target, TargetMethod};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::sync::Arc;

pub struct TargetEngine {
    config: TargetEngineConfig,
    methods: Vec<Arc<dyn TargetMethodCalc>>,
}

impl TargetEngine {
    pub fn new(config: TargetEngineConfig) -> TargetEngineResult<Self> {
        config.validate()?;
        Ok(Self {
            config,
            methods: Vec::new(),
        })
    }

    pub fn config(&self) -> &TargetEngineConfig {
        &self.config
    }

    pub fn method_count(&self) -> usize {
        self.methods.len()
    }

    pub fn register(&mut self, m: Arc<dyn TargetMethodCalc>) {
        self.methods.push(m);
    }

    /// Compute targets for a detection. Walks every registered method,
    /// gathers the candidates, then clusters and trims them per config.
    pub fn project(&self, det: &Detection) -> Vec<Target> {
        let mut raw: Vec<Target> = Vec::new();
        for m in &self.methods {
            raw.extend(m.project(det));
        }
        if raw.is_empty() {
            return raw;
        }
        let clustered = cluster(raw, self.config.cluster_tolerance);
        trim(clustered, self.config.min_weight, self.config.max_targets)
    }
}

/// Merge targets that fall within `tolerance * mid_price` of one another
/// into a single weighted-average target. The merged label notes how
/// many sources contributed; the merged method becomes `Cluster` when
/// the sources had different methods, otherwise the original method is
/// retained.
fn cluster(mut targets: Vec<Target>, tolerance: f64) -> Vec<Target> {
    targets.sort_by(|a, b| a.price.cmp(&b.price));
    let mut out: Vec<Target> = Vec::new();
    for t in targets {
        let merged_into = out.iter_mut().find(|c| within_tolerance(c, &t, tolerance));
        if let Some(c) = merged_into {
            *c = merge_two(c.clone(), t);
        } else {
            out.push(t);
        }
    }
    out
}

fn within_tolerance(a: &Target, b: &Target, tolerance: f64) -> bool {
    let ap = match a.price.to_f64() {
        Some(v) => v,
        None => return false,
    };
    let bp = match b.price.to_f64() {
        Some(v) => v,
        None => return false,
    };
    let mid = (ap.abs() + bp.abs()) / 2.0;
    if mid <= 0.0 {
        return false;
    }
    ((ap - bp).abs() / mid) <= tolerance
}

fn merge_two(a: Target, b: Target) -> Target {
    let aw = a.weight as f64;
    let bw = b.weight as f64;
    let total = (aw + bw).max(1e-9);
    let ap = a.price.to_f64().unwrap_or(0.0);
    let bp = b.price.to_f64().unwrap_or(0.0);
    let merged_price = (ap * aw + bp * bw) / total;
    let merged_weight = ((aw + bw).min(1.0)) as f32;
    let method = if a.method == b.method {
        a.method
    } else {
        TargetMethod::Cluster
    };
    let label = match (a.label, b.label) {
        (Some(la), Some(lb)) => Some(format!("{la} + {lb}")),
        (Some(l), None) | (None, Some(l)) => Some(l),
        _ => None,
    };
    Target {
        price: Decimal::from_f64_retain(merged_price).unwrap_or(Decimal::ZERO),
        method,
        weight: merged_weight,
        label,
    }
}

fn trim(mut targets: Vec<Target>, min_weight: f32, max_targets: usize) -> Vec<Target> {
    targets.retain(|t| t.weight >= min_weight);
    // Keep the heaviest `max_targets` (by weight), then re-sort by price.
    targets.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
    targets.truncate(max_targets);
    targets.sort_by(|a, b| a.price.cmp(&b.price));
    targets
}

//! FibExtensionResolver — uses the most recent swing X→A leg as a
//! ruler and projects 1.272 / 1.618 / 2.618 extensions forward.

use crate::config::TargetConfig;
use crate::resolver::{
    DetectionContext, TargetLevel, TargetResolver, TargetSet, TargetSource,
};

pub struct FibExtensionResolver;

impl TargetResolver for FibExtensionResolver {
    fn source(&self) -> TargetSource {
        TargetSource::FibExtension
    }
    fn resolve(&self, ctx: &DetectionContext, cfg: &TargetConfig) -> Option<TargetSet> {
        // Needs at least two structural anchors to define a leg. Uses
        // X and A when present (harmonic / Elliott-ish sets), else
        // start / end fallback that motive emits.
        let (x, a) = ctx
            .anchors
            .get("X")
            .zip(ctx.anchors.get("A"))
            .or_else(|| ctx.anchors.get("1").zip(ctx.anchors.get("2")))
            .or_else(|| ctx.anchors.get("start").zip(ctx.anchors.get("end")))?;
        let leg = *a - *x;
        if leg.abs() < f64::EPSILON {
            return None;
        }
        let tps: Vec<TargetLevel> = cfg
            .fib_extensions
            .iter()
            .enumerate()
            .map(|(i, fib)| TargetLevel {
                ordinal: (i + 1) as u8,
                price: *x + leg * fib,
                source: TargetSource::FibExtension,
                hit_prob_hint: (0.7 - (i as f64) * 0.15).clamp(0.25, 0.8),
                label: format!("Fib {:.3}", fib),
            })
            .collect();
        // SL at the leg origin — closing back through X invalidates.
        let stop_loss = *x;
        Some(TargetSet {
            direction: ctx.direction,
            entry: ctx.entry,
            take_profits: tps,
            stop_loss,
            invalidation: stop_loss,
            primary_source: TargetSource::FibExtension,
            notes: vec![format!(
                "Fib extension over leg {:.4} → {:.4}",
                x, a
            )],
        })
    }
}

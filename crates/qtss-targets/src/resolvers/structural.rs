//! StructuralInvalidationResolver — TPs at the next forward-direction
//! pivots, SL at the most recent opposite-direction pivot.

use crate::config::TargetConfig;
use crate::resolver::{
    DetectionContext, TargetLevel, TargetResolver, TargetSet, TargetSource,
};

pub struct StructuralInvalidationResolver;

impl TargetResolver for StructuralInvalidationResolver {
    fn source(&self) -> TargetSource {
        TargetSource::Structural
    }
    fn resolve(&self, ctx: &DetectionContext, cfg: &TargetConfig) -> Option<TargetSet> {
        if ctx.forward_pivots.is_empty() {
            return None;
        }
        let sign = ctx.direction.sign();
        let tps: Vec<TargetLevel> = ctx
            .forward_pivots
            .iter()
            .take(cfg.structural_tp_count)
            .enumerate()
            .filter(|(_, &price)| (price - ctx.entry) * sign > 0.0)
            .map(|(i, &price)| TargetLevel {
                ordinal: (i + 1) as u8,
                price,
                source: TargetSource::Structural,
                hit_prob_hint: (0.75 - (i as f64) * 0.18).clamp(0.3, 0.85),
                label: format!("Swing {}", i + 1),
            })
            .collect();
        if tps.is_empty() {
            return None;
        }
        // SL beyond opposite pivot if present, else no valid result.
        let Some(opp) = ctx.opposite_pivot else { return None; };
        let buffer = ctx.entry.abs() * cfg.structural_sl_buffer_pct;
        let stop_loss = opp - sign * buffer;
        Some(TargetSet {
            direction: ctx.direction,
            entry: ctx.entry,
            take_profits: tps,
            stop_loss,
            invalidation: opp,
            primary_source: TargetSource::Structural,
            notes: vec!["Structural pivot ladder".to_string()],
        })
    }
}

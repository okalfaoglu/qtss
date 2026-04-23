//! AtrBandResolver — universal fallback. TPs at entry ± k × ATR, SL
//! at entry ∓ ATR. Called last in the registry so pattern-aware
//! resolvers have first crack at the context.

use crate::config::TargetConfig;
use crate::resolver::{
    DetectionContext, TargetLevel, TargetResolver, TargetSet, TargetSource,
};

pub struct AtrBandResolver;

impl TargetResolver for AtrBandResolver {
    fn source(&self) -> TargetSource {
        TargetSource::AtrBand
    }
    fn resolve(&self, ctx: &DetectionContext, cfg: &TargetConfig) -> Option<TargetSet> {
        let atr = ctx.atr?;
        if atr.abs() < cfg.atr_min_abs {
            return None;
        }
        let sign = ctx.direction.sign();
        let tps: Vec<TargetLevel> = cfg
            .atr_tp_multipliers
            .iter()
            .enumerate()
            .map(|(i, mult)| TargetLevel {
                ordinal: (i + 1) as u8,
                price: ctx.entry + sign * atr * mult,
                source: TargetSource::AtrBand,
                hit_prob_hint: (0.7 - (i as f64) * 0.17).clamp(0.2, 0.8),
                label: format!("ATR × {:.1}", mult),
            })
            .collect();
        let stop_loss = ctx.entry - sign * atr * cfg.atr_sl_multiplier;
        Some(TargetSet {
            direction: ctx.direction,
            entry: ctx.entry,
            take_profits: tps,
            stop_loss,
            invalidation: stop_loss,
            primary_source: TargetSource::AtrBand,
            notes: vec![format!("ATR band fallback (ATR = {:.6})", atr)],
        })
    }
}

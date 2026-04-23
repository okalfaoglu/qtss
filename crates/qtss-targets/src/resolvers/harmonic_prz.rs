//! HarmonicPrzResolver — TP ladder from D back along CD toward C at
//! classical Carney Fib ratios (0.382 / 0.618 / 1.0).

use crate::config::TargetConfig;
use crate::resolver::{
    DetectionContext, TargetLevel, TargetResolver, TargetSet, TargetSource,
};

pub struct HarmonicPrzResolver;

impl TargetResolver for HarmonicPrzResolver {
    fn source(&self) -> TargetSource {
        TargetSource::HarmonicPrz
    }
    fn resolve(&self, ctx: &DetectionContext, cfg: &TargetConfig) -> Option<TargetSet> {
        if ctx.family != "harmonic" {
            return None;
        }
        let (x, a, _b, c, d) = (
            *ctx.anchors.get("X")?,
            *ctx.anchors.get("A")?,
            *ctx.anchors.get("B")?,
            *ctx.anchors.get("C")?,
            *ctx.anchors.get("D")?,
        );
        let cd = c - d; // signed — bullish PRZ = D below C, so cd > 0.
        let xa = a - x;
        let sign = ctx.direction.sign();
        let mut tps = Vec::with_capacity(cfg.harmonic_tp_fibs.len());
        for (i, fib) in cfg.harmonic_tp_fibs.iter().enumerate() {
            let price = d + fib * cd;
            // Decreasing hit probability as fib extends.
            let hit = (1.0 - (i as f64) * 0.25).clamp(0.2, 0.95);
            tps.push(TargetLevel {
                ordinal: (i + 1) as u8,
                price,
                source: TargetSource::HarmonicPrz,
                hit_prob_hint: hit,
                label: format!("T{} {:.3} of CD", i + 1, fib),
            });
        }
        // SL beyond the PRZ by a fraction of the XA leg.
        let sl_offset = xa.abs() * cfg.harmonic_sl_buffer_pct;
        let stop_loss = d - sign * sl_offset;
        let invalidation = d - sign * sl_offset * 0.5;
        Some(TargetSet {
            direction: ctx.direction,
            entry: ctx.entry,
            take_profits: tps,
            stop_loss,
            invalidation,
            primary_source: TargetSource::HarmonicPrz,
            notes: vec![format!(
                "Harmonic PRZ: D = {:.4}, CD = {:.4}, XA = {:.4}",
                d, cd, xa
            )],
        })
    }
}

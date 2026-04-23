//! VProfileMagnetResolver — uses VPOC/VAH/VAL as target magnets.
//! Prefers the 3 nearest profile levels in the trade direction within
//! `vprofile_max_distance_pct` of entry.

use crate::config::TargetConfig;
use crate::resolver::{
    DetectionContext, TargetLevel, TargetResolver, TargetSet, TargetSource, TradeDirection,
};

pub struct VProfileMagnetResolver;

impl TargetResolver for VProfileMagnetResolver {
    fn source(&self) -> TargetSource {
        TargetSource::VProfileMagnet
    }
    fn resolve(&self, ctx: &DetectionContext, cfg: &TargetConfig) -> Option<TargetSet> {
        if ctx.vprofile_levels.is_empty() {
            return None;
        }
        let entry = ctx.entry;
        let sign = ctx.direction.sign();
        // Filter to forward-direction levels within max distance.
        let mut candidates: Vec<(String, f64)> = ctx
            .vprofile_levels
            .iter()
            .filter_map(|(name, &price)| {
                let dist = (price - entry) * sign;
                if dist <= 0.0 {
                    return None;
                }
                if (dist / entry.abs().max(1e-9)) > cfg.vprofile_max_distance_pct {
                    return None;
                }
                Some((name.clone(), price))
            })
            .collect();
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        // For shorts: sort descending instead.
        if matches!(ctx.direction, TradeDirection::Short) {
            candidates.reverse();
        }
        if candidates.is_empty() {
            return None;
        }
        let tps: Vec<TargetLevel> = candidates
            .iter()
            .take(3)
            .enumerate()
            .map(|(i, (name, price))| TargetLevel {
                ordinal: (i + 1) as u8,
                price: *price,
                source: TargetSource::VProfileMagnet,
                hit_prob_hint: (0.8 - (i as f64) * 0.15).clamp(0.3, 0.9),
                label: format!("{} @ {:.4}", name, price),
            })
            .collect();
        // SL beyond the ATR if we have one, else a flat 1% band.
        let sl_width = ctx.atr.unwrap_or_else(|| entry.abs() * 0.01);
        let stop_loss = entry - sign * sl_width;
        let invalidation = stop_loss;
        Some(TargetSet {
            direction: ctx.direction,
            entry,
            take_profits: tps,
            stop_loss,
            invalidation,
            primary_source: TargetSource::VProfileMagnet,
            notes: vec!["VProfile magnet set".to_string()],
        })
    }
}

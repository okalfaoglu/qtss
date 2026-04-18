//! GapDetector — runs every [`GapSpec`] on the most recent bar window
//! and emits the highest-scoring [`Detection`] (if any).

use crate::config::GapConfig;
use crate::error::GapResult;
use crate::specs::{GapMatch, GAP_SPECS};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::detection::{Detection, PatternKind, PatternState, PivotRef};
use qtss_domain::v2::instrument::Instrument;
use qtss_domain::v2::pivot::PivotLevel;
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;

pub struct GapDetector {
    config: GapConfig,
}

impl GapDetector {
    pub fn new(config: GapConfig) -> GapResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &GapConfig {
        &self.config
    }

    pub fn detect(
        &self,
        bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Option<Detection> {
        if bars.len() < self.config.volume_sma_bars + 2 {
            return None;
        }

        let mut best: Option<(&'static str, GapMatch)> = None;
        for spec in GAP_SPECS {
            if let Some(m) = (spec.eval)(bars, &self.config) {
                if best.as_ref().map(|(_, b)| m.score > b.score).unwrap_or(true) {
                    best = Some((spec.name, m));
                }
            }
        }

        let (name, m) = best?;
        if (m.score as f32) < self.config.min_structural_score {
            return None;
        }

        let kind = PatternKind::Gap(format!("{}_{}", name, m.variant));
        let anchors = build_anchors(bars, &m);
        // Invalidation: for bull gaps, close back below gap_bar low; for
        // bear, above gap_bar high. The target-engine can refine this.
        let gap_bar = &bars[m.gap_bar];
        let invalidation_price = if m.variant == "bull" {
            gap_bar.low
        } else {
            gap_bar.high
        };

        Some(Detection::new(
            instrument.clone(),
            timeframe,
            kind,
            PatternState::Forming,
            anchors,
            m.score as f32,
            invalidation_price,
            regime.clone(),
        ))
    }
}

fn build_anchors(bars: &[Bar], m: &GapMatch) -> Vec<PivotRef> {
    let mut out = Vec::with_capacity(3);
    // Pre-gap bar (P), gap bar (G), optional partner (I) for island.
    if m.gap_bar >= 1 {
        let pre = &bars[m.gap_bar - 1];
        out.push(PivotRef {
            bar_index: m.gap_bar as u64 - 1,
            price: pre.close,
            level: PivotLevel::L0,
            label: Some("P".to_string()),
        });
    }
    let gap = &bars[m.gap_bar];
    out.push(PivotRef {
        bar_index: m.gap_bar as u64,
        price: gap.open,
        level: PivotLevel::L0,
        label: Some("G".to_string()),
    });
    if let Some(k) = m.partner_bar {
        let partner = &bars[k];
        out.push(PivotRef {
            bar_index: k as u64,
            price: partner.open,
            level: PivotLevel::L0,
            label: Some("I".to_string()),
        });
    }
    // Include gap magnitude / volume ratio as synthetic anchor metadata
    // via a zero-price row to keep the feature-source extractor simple.
    out.push(PivotRef {
        bar_index: m.gap_bar as u64,
        price: Decimal::from_f64(m.gap_pct).unwrap_or(Decimal::ZERO),
        level: PivotLevel::L0,
        label: Some("gap_pct".to_string()),
    });
    out
}

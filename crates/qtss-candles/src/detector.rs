//! CandleDetector — runs every [`CandleSpec`] on the most recent bars
//! and emits the highest-scoring detection (if any).

use crate::config::CandleConfig;
use crate::error::CandleResult;
use crate::specs::{CandleMatch, CANDLE_SPECS};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::detection::{Detection, PatternKind, PatternState, PivotRef};
use qtss_domain::v2::instrument::Instrument;
use qtss_domain::v2::pivot::PivotLevel;
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;

pub struct CandleDetector {
    config: CandleConfig,
}

impl CandleDetector {
    pub fn new(config: CandleConfig) -> CandleResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &CandleConfig {
        &self.config
    }

    pub fn detect(
        &self,
        bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Option<Detection> {
        // Timeframe gate: candle patterns emit too much noise on
        // sub-threshold timeframes. Threshold is DB-tunable — a 5m
        // morning_star has near-random hit rate; a 1h one is usable.
        if timeframe.seconds() < self.config.min_timeframe_seconds {
            return None;
        }

        // Need enough bars for trend-context lookbacks.
        let min_bars = self.config.trend_context_bars + 3;
        if bars.len() < min_bars {
            return None;
        }

        let mut best: Option<(&'static str, CandleMatch)> = None;
        for spec in CANDLE_SPECS {
            if bars.len() < spec.bars_needed {
                continue;
            }
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

        let subkind = if m.variant == "neutral" {
            name.to_string()
        } else {
            format!("{}_{}", name, m.variant)
        };
        let kind = PatternKind::Candle(subkind);

        // Anchors: open of first bar + close of last bar, with labels.
        let first = &bars[m.start_idx];
        let last = &bars[m.end_idx];
        let anchors = vec![
            PivotRef {
                bar_index: m.start_idx as u64,
                price: first.open,
                level: PivotLevel::L0,
                label: Some("open".to_string()),
            },
            PivotRef {
                bar_index: m.end_idx as u64,
                price: last.close,
                level: PivotLevel::L0,
                label: Some("close".to_string()),
            },
        ];

        // Invalidation: bull → pattern low; bear → pattern high; neutral → current low/high
        let (lo, hi) = (m.start_idx..=m.end_idx).fold(
            (last.low, last.high),
            |(lo, hi), i| {
                let b = &bars[i];
                (lo.min(b.low), hi.max(b.high))
            },
        );
        let invalidation_price = match m.variant {
            "bull" => lo,
            "bear" => hi,
            _ => last.low,
        };

        Some(Detection::new(
            instrument.clone(),
            timeframe,
            kind,
            PatternState::Confirmed,
            anchors,
            m.score as f32,
            invalidation_price,
            regime.clone(),
        ))
    }
}

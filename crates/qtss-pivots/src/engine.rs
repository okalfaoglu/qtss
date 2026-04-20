//! Multi-level pivot engine.
//!
//! Owns one ATR estimator + four parallel ZigZag instances (one per level).
//! On each new bar:
//!   1. Update ATR.
//!   2. Feed the bar to the L0 zigzag with `atr * mult[0]`.
//!   3. If a pivot is confirmed at L0, feed it as a synthetic sample to
//!      the L1 zigzag with `atr * mult[1]`. If L1 confirms, cascade to L2,
//!      and so on.
//!
//! That cascade is exactly what enforces the subset invariant — a level-N
//! pivot can only ever exist at a bar index that already cleared level N-1.

use crate::atr::AtrState;
use crate::config::PivotConfig;
use crate::error::PivotResult;
use crate::zigzag::{ConfirmedPivot, Sample, ZigZag};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::pivot::{Pivot, PivotLevel, PivotTree};
use rust_decimal::Decimal;

/// A pivot the engine just confirmed, tagged with the level it was
/// emitted at. Useful for callers that want to publish events
/// (`pivot.updated`) per level rather than diff a snapshot.
#[derive(Debug, Clone)]
pub struct NewPivot {
    pub level: PivotLevel,
    pub pivot: Pivot,
}

pub struct PivotEngine {
    config: PivotConfig,
    atr: AtrState,
    bar_index: u64,
    last_time: Option<chrono::DateTime<chrono::Utc>>,
    zigzags: [ZigZag; 4],
    confirmed: [Vec<Pivot>; 4],
}

impl PivotEngine {
    pub fn new(config: PivotConfig) -> PivotResult<Self> {
        config.validate()?;
        // **Fix B** — construct each level's ZigZag with its own
        // `min_hold_bars` gate. Higher levels are more tolerant of
        // noise (larger threshold) AND required to hold longer (larger
        // min_hold_bars) so structural pivots dominate.
        let mh = config.min_hold_bars;
        let zigzags = [
            ZigZag::with_min_hold_bars(mh[0]),
            ZigZag::with_min_hold_bars(mh[1]),
            ZigZag::with_min_hold_bars(mh[2]),
            ZigZag::with_min_hold_bars(mh[3]),
        ];
        Ok(Self {
            atr: AtrState::new(config.atr_period),
            config,
            bar_index: 0,
            last_time: None,
            zigzags,
            confirmed: [vec![], vec![], vec![], vec![]],
        })
    }

    /// Feed one bar. Returns the list of pivots confirmed by this bar
    /// across all levels (often empty, occasionally one or more).
    pub fn on_bar(&mut self, bar: &Bar) -> PivotResult<Vec<NewPivot>> {
        let idx = self.bar_index;
        self.bar_index += 1;
        // Monotonic-time guard. Out-of-order bars would corrupt the
        // ATR state and produce nonsense pivots; surface as an error
        // so the caller can drop or reorder.
        if let Some(prev) = self.last_time {
            if bar.open_time < prev {
                return Err(crate::error::PivotError::NonMonotonic(idx));
            }
        }
        self.last_time = Some(bar.open_time);

        let atr = match self.atr.update(bar.high, bar.low, bar.close) {
            Some(v) => v,
            // Still warming up — no pivots can be produced yet.
            None => return Ok(vec![]),
        };

        let sample = Sample {
            bar_index: idx,
            time: bar.open_time,
            high: bar.high,
            low: bar.low,
            volume: bar.volume,
        };

        // Cascade through the levels. Each level emits at most one pivot
        // per bar; if it does, the same pivot is fed forward as a sample
        // to the next, finer-to-coarser, threshold.
        let mut emitted = Vec::new();
        let mut next_sample = Some(sample);
        for (i, level) in PivotLevel::ALL.into_iter().enumerate() {
            let Some(s) = next_sample.take() else { break };
            let threshold = atr * self.config.atr_mult[i];
            let confirmed = self.zigzags[i].on_sample(&s, threshold);
            if let Some(cp) = confirmed {
                let mut pivot = build_pivot(&cp, level);
                // Classify swing type (HH/HL/LH/LL) vs previous same-kind pivot.
                pivot.swing_type = classify_swing(&self.confirmed[i], &pivot);
                self.confirmed[i].push(pivot.clone());
                emitted.push(NewPivot { level, pivot });
                // Cascade: feed this confirmation forward as a synthetic
                // sample to the next coarser level. high == low == price
                // because higher-level zigzags compare against pivot prices,
                // not bar ranges.
                next_sample = Some(Sample {
                    bar_index: cp.bar_index,
                    time: cp.time,
                    high: cp.price,
                    low: cp.price,
                    volume: cp.volume_at_pivot,
                });
            } else {
                next_sample = None;
            }
        }
        Ok(emitted)
    }

    /// Snapshot the current pivot tree. Cheap clone of the four vectors.
    pub fn snapshot(&self) -> PivotTree {
        PivotTree::new(
            self.confirmed[0].clone(),
            self.confirmed[1].clone(),
            self.confirmed[2].clone(),
            self.confirmed[3].clone(),
        )
    }

    /// Current ATR, exposed for diagnostics. `None` while warming up.
    pub fn current_atr(&self) -> Option<Decimal> {
        self.atr.value()
    }
}

fn build_pivot(cp: &ConfirmedPivot, level: PivotLevel) -> Pivot {
    Pivot {
        bar_index: cp.bar_index,
        time: cp.time,
        price: cp.price,
        kind: cp.kind,
        level,
        prominence: cp.prominence,
        volume_at_pivot: cp.volume_at_pivot,
        swing_type: None, // Set by classify_swing after construction.
    }
}

/// Compare a new pivot with the previous pivot of the same kind at the
/// same level. Produces HH/HL/LH/LL classification (PineScript dir=±2).
fn classify_swing(
    confirmed: &[Pivot],
    new: &Pivot,
) -> Option<qtss_domain::v2::pivot::SwingType> {
    use qtss_domain::v2::pivot::{PivotKind, SwingType};
    // Find the last pivot of the same kind (High→last High, Low→last Low).
    let prev = confirmed.iter().rev().find(|p| p.kind == new.kind)?;
    match new.kind {
        PivotKind::High => {
            if new.price >= prev.price { Some(SwingType::HH) }
            else { Some(SwingType::LH) }
        }
        PivotKind::Low => {
            if new.price <= prev.price { Some(SwingType::LL) }
            else { Some(SwingType::HL) }
        }
    }
}

//! Multi-level pivot engine (LuxAlgo pivot-window parity).
//!
//! Runs four parallel [`ZigZag`] detectors, one per level, each with a
//! different window length (see [`PivotConfig::lengths`]). Unlike the
//! previous ATR-threshold design, levels are **independent** — no
//! cascade needed because pivot-window is inherently subset-preserving:
//! a bar that is the extremum of a ±L_big window is necessarily also
//! the extremum of any smaller ±L_small window at the same index.
//!
//! No ATR, no warm-up — the only lag is `length` bars per level before
//! a pivot can confirm.

use crate::config::PivotConfig;
use crate::error::PivotResult;
use crate::zigzag::{ConfirmedPivot, ProvisionalExtreme, Sample, ZigZag};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::pivot::{Pivot, PivotLevel, PivotTree};

/// A pivot the engine just confirmed, tagged with the level it was
/// emitted at.
#[derive(Debug, Clone)]
pub struct NewPivot {
    pub level: PivotLevel,
    pub pivot: Pivot,
}

/// Level-tagged provisional pivot — the current running extreme of a
/// ZigZag. Chart-only; never stored, never consumed by detectors.
#[derive(Debug, Clone)]
pub struct ProvisionalPivot {
    pub level: PivotLevel,
    pub extreme: ProvisionalExtreme,
}

pub struct PivotEngine {
    bar_index: u64,
    last_time: Option<chrono::DateTime<chrono::Utc>>,
    zigzags: [ZigZag; 4],
    confirmed: [Vec<Pivot>; 4],
}

impl PivotEngine {
    pub fn new(config: PivotConfig) -> PivotResult<Self> {
        config.validate()?;
        let l = config.lengths;
        let zigzags = [
            ZigZag::with_length(l[0]),
            ZigZag::with_length(l[1]),
            ZigZag::with_length(l[2]),
            ZigZag::with_length(l[3]),
        ];
        Ok(Self {
            bar_index: 0,
            last_time: None,
            zigzags,
            confirmed: [vec![], vec![], vec![], vec![]],
        })
    }

    /// Feed one bar. Returns pivots confirmed across all levels by
    /// this bar (often empty, occasionally one or more).
    pub fn on_bar(&mut self, bar: &Bar) -> PivotResult<Vec<NewPivot>> {
        let idx = self.bar_index;
        self.bar_index += 1;

        if let Some(prev) = self.last_time {
            if bar.open_time < prev {
                return Err(crate::error::PivotError::NonMonotonic(idx));
            }
        }
        self.last_time = Some(bar.open_time);

        let sample = Sample {
            bar_index: idx,
            time: bar.open_time,
            high: bar.high,
            low: bar.low,
            volume: bar.volume,
        };

        let mut emitted = Vec::new();
        for (i, level) in PivotLevel::ALL.into_iter().enumerate() {
            if let Some(cp) = self.zigzags[i].push(&sample) {
                let mut pivot = build_pivot(&cp, level);
                pivot.swing_type = classify_swing(&self.confirmed[i], &pivot);
                self.confirmed[i].push(pivot.clone());
                emitted.push(NewPivot { level, pivot });
            }
        }
        Ok(emitted)
    }

    /// Snapshot the current pivot tree. Cheap clone.
    pub fn snapshot(&self) -> PivotTree {
        PivotTree::new(
            self.confirmed[0].clone(),
            self.confirmed[1].clone(),
            self.confirmed[2].clone(),
            self.confirmed[3].clone(),
        )
    }

    /// Provisional (unconfirmed) running extreme per level. Never fed
    /// into detectors; render-only.
    pub fn provisional_extremes(&self) -> [Option<ProvisionalPivot>; 4] {
        let mut out: [Option<ProvisionalPivot>; 4] = [None, None, None, None];
        for (i, level) in PivotLevel::ALL.into_iter().enumerate() {
            if let Some(e) = self.zigzags[i].provisional_extreme() {
                out[i] = Some(ProvisionalPivot { level, extreme: e });
            }
        }
        out
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
        swing_type: None,
    }
}

fn classify_swing(
    confirmed: &[Pivot],
    new: &Pivot,
) -> Option<qtss_domain::v2::pivot::SwingType> {
    use qtss_domain::v2::pivot::{PivotKind, SwingType};
    let prev = confirmed.iter().rev().find(|p| p.kind == new.kind)?;
    match new.kind {
        PivotKind::High => {
            if new.price >= prev.price { Some(SwingType::HH) } else { Some(SwingType::LH) }
        }
        PivotKind::Low => {
            if new.price <= prev.price { Some(SwingType::LL) } else { Some(SwingType::HL) }
        }
    }
}

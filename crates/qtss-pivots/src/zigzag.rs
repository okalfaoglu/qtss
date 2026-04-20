//! Pivot-window ZigZag detector (LuxAlgo-parity).
//!
//! # Algorithm
//!
//! A pivot at bar `i` is any bar whose price is the extremum over a
//! window of `length` bars on each side:
//!
//!   - **Pivot High**: `high[i]` is the maximum of `high[i-length..=i+length]`
//!   - **Pivot Low**:  `low[i]`  is the minimum of `low[i-length..=i+length]`
//!
//! The detector keeps a ring buffer of the last `2*length + 1` bars and,
//! on every incoming bar, evaluates whether the **centre** of the buffer
//! (bar index `current - length`) qualifies as a pivot. Confirmation lag
//! is therefore exactly `length` bars.
//!
//! # ZigZag alternation
//!
//! Raw pivot-window output can flag the same bar as both a high and a
//! low when the range is wide; it can also emit two highs in a row
//! (two consecutive swing tops). We apply ZigZag-style alternation on
//! top: track the direction of the last confirmed pivot; same-kind
//! candidates replace the previous extreme if they are more extreme,
//! opposite-kind candidates trigger emission and flip direction.
//!
//! The running extreme (the pivot we *would* emit if the next bar
//! closed the window) is exposed via [`ZigZag::provisional_extreme`] so
//! charts can render a pivot marker on the most recent bars — matching
//! LuxAlgo's visual behaviour where the ZigZag line reaches to the
//! current bar.

use chrono::{DateTime, Utc};
use qtss_domain::v2::pivot::PivotKind;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::VecDeque;

/// One observation — a bar's (high, low) range plus volume.
#[derive(Debug, Clone)]
pub struct Sample {
    pub bar_index: u64,
    pub time: DateTime<Utc>,
    pub high: Decimal,
    pub low: Decimal,
    pub volume: Decimal,
}

#[derive(Debug, Clone)]
pub struct ConfirmedPivot {
    pub bar_index: u64,
    pub time: DateTime<Utc>,
    pub price: Decimal,
    pub kind: PivotKind,
    pub prominence: Decimal,
    pub volume_at_pivot: Decimal,
}

/// Unconfirmed running extreme — the pivot currently "leading" the
/// swing. Exposed for chart rendering; never persisted.
#[derive(Debug, Clone)]
pub struct ProvisionalExtreme {
    pub bar_index: u64,
    pub time: DateTime<Utc>,
    pub price: Decimal,
    pub kind: PivotKind,
    pub volume_at_pivot: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Unknown,
    Up,
    Down,
}

#[derive(Debug, Clone)]
struct Candidate {
    bar_index: u64,
    time: DateTime<Utc>,
    price: Decimal,
    kind: PivotKind,
    volume: Decimal,
}

#[derive(Debug, Clone)]
pub struct ZigZag {
    /// Window radius — `length` bars on each side for pivot-window test.
    length: u32,
    /// Ring buffer of the last `2*length + 1` samples.
    buf: VecDeque<Sample>,
    /// Direction of the last confirmed (or provisional) pivot — used
    /// for ZigZag alternation on top of the raw pivot-window output.
    direction: Direction,
    /// Running extreme in the current swing (provisional — may still
    /// be replaced if a more extreme same-kind pivot appears before
    /// the opposite side confirms).
    pending: Option<Candidate>,
    /// Last confirmed pivot price — used to compute prominence.
    last_confirmed_price: Option<Decimal>,
}

impl ZigZag {
    /// Construct a pivot-window ZigZag with the given window radius.
    /// `length >= 1`. A pivot at bar `i` requires `length` bars on each
    /// side to evaluate, so confirmation lag is `length` bars.
    pub fn with_length(length: u32) -> Self {
        Self {
            length: length.max(1),
            buf: VecDeque::with_capacity((2 * length.max(1) + 1) as usize),
            direction: Direction::Unknown,
            pending: None,
            last_confirmed_price: None,
        }
    }

    /// Legacy no-arg constructor — defaults to length 4 (LuxAlgo L0).
    pub fn new() -> Self {
        Self::with_length(4)
    }

    /// Feed one sample. Returns at most one newly-confirmed pivot.
    pub fn on_sample(&mut self, s: &Sample, _unused_threshold: Decimal) -> Option<ConfirmedPivot> {
        self.push(s)
    }

    /// Feed one sample (preferred call site — no dummy threshold).
    pub fn push(&mut self, s: &Sample) -> Option<ConfirmedPivot> {
        self.buf.push_back(s.clone());
        let cap = (2 * self.length + 1) as usize;
        while self.buf.len() > cap {
            self.buf.pop_front();
        }
        // Need a full window before the centre bar can be evaluated.
        if self.buf.len() < cap {
            return None;
        }
        let centre_idx = self.length as usize;
        let centre = self.buf[centre_idx].clone();

        // Pivot-window test: is `centre` the extremum of the window?
        let mut is_high = true;
        let mut is_low = true;
        for (k, b) in self.buf.iter().enumerate() {
            if k == centre_idx {
                continue;
            }
            if b.high > centre.high {
                is_high = false;
            }
            if b.low < centre.low {
                is_low = false;
            }
            if !is_high && !is_low {
                break;
            }
        }

        // Pick the qualifying kind. In the rare case the centre is both
        // (e.g. gigantic outside bar), prefer the direction that
        // continues the current swing — it produces smoother alternation.
        let candidate_kind = match (is_high, is_low) {
            (true, false) => Some(PivotKind::High),
            (false, true) => Some(PivotKind::Low),
            (true, true) => Some(match self.direction {
                Direction::Up => PivotKind::High,
                Direction::Down => PivotKind::Low,
                Direction::Unknown => PivotKind::High,
            }),
            (false, false) => None,
        }?;

        let candidate = Candidate {
            bar_index: centre.bar_index,
            time: centre.time,
            price: match candidate_kind {
                PivotKind::High => centre.high,
                PivotKind::Low => centre.low,
            },
            kind: candidate_kind,
            volume: centre.volume,
        };

        self.apply_alternation(candidate)
    }

    /// Apply ZigZag alternation rules against the current pending
    /// extreme. Returns the pivot to emit, if any.
    fn apply_alternation(&mut self, c: Candidate) -> Option<ConfirmedPivot> {
        match &self.pending {
            None => {
                // First ever candidate — seed direction, don't emit yet
                // (we have nothing to anchor prominence against).
                self.direction = match c.kind {
                    PivotKind::High => Direction::Up,
                    PivotKind::Low => Direction::Down,
                };
                self.pending = Some(c);
                None
            }
            Some(prev) if prev.kind == c.kind => {
                // Same kind — replace if more extreme (LuxAlgo collapses
                // consecutive same-direction pivots to the extremum).
                let replace = match c.kind {
                    PivotKind::High => c.price > prev.price,
                    PivotKind::Low => c.price < prev.price,
                };
                if replace {
                    self.pending = Some(c);
                }
                None
            }
            Some(prev) => {
                // Opposite kind → emit the previous pending pivot, then
                // the new candidate becomes the next pending.
                let prev_cloned = prev.clone();
                let pivot = self.emit(&prev_cloned);
                self.direction = match c.kind {
                    PivotKind::High => Direction::Up,
                    PivotKind::Low => Direction::Down,
                };
                self.pending = Some(c);
                Some(pivot)
            }
        }
    }

    fn emit(&mut self, prev: &Candidate) -> ConfirmedPivot {
        let prominence = match self.last_confirmed_price {
            Some(p) => (prev.price - p).abs(),
            None => dec!(0),
        };
        self.last_confirmed_price = Some(prev.price);
        ConfirmedPivot {
            bar_index: prev.bar_index,
            time: prev.time,
            price: prev.price,
            kind: prev.kind,
            prominence,
            volume_at_pivot: prev.volume,
        }
    }

    /// The pivot we would emit if the next opposite-kind candidate
    /// arrived. Matches LuxAlgo's visual ZigZag line reaching the
    /// current bar. `None` before the first pivot-window confirms.
    pub fn provisional_extreme(&self) -> Option<ProvisionalExtreme> {
        self.pending.as_ref().map(|c| ProvisionalExtreme {
            bar_index: c.bar_index,
            time: c.time,
            price: c.price,
            kind: c.kind,
            volume_at_pivot: c.volume,
        })
    }
}

impl Default for ZigZag {
    fn default() -> Self {
        Self::new()
    }
}

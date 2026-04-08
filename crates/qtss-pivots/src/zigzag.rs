//! Streaming zigzag detector.
//!
//! Generic over a sample shape so the same algorithm can run twice:
//!   1. on raw bars (to produce L0 pivots), and
//!   2. on previously-confirmed pivots (to produce higher levels).
//!
//! That second pass is what guarantees the subset invariant — a level-N
//! pivot can only exist at a bar index that already exists at level N-1.
//!
//! ## Algorithm
//!
//! Track the running extreme in the current swing direction. A reversal
//! is *confirmed* when the price moves against the extreme by more than
//! `threshold` (typically `atr_mult * ATR`). On confirmation we emit the
//! previous extreme as a pivot and flip direction.

use chrono::{DateTime, Utc};
use qtss_domain::v2::pivot::PivotKind;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// One observation that the zigzag operates on. Either a raw bar (high,
/// low, close) or a previously-detected pivot (high == low == price).
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Unknown,
    Up,
    Down,
}

#[derive(Debug, Clone)]
pub struct ZigZag {
    direction: Direction,
    extreme_idx: u64,
    extreme_time: DateTime<Utc>,
    extreme_price: Decimal,
    extreme_volume: Decimal,
    /// Last confirmed pivot price — used to compute prominence on the
    /// next confirmation. None until the very first pivot is emitted.
    last_confirmed_price: Option<Decimal>,
}

impl ZigZag {
    pub fn new() -> Self {
        Self {
            direction: Direction::Unknown,
            extreme_idx: 0,
            extreme_time: chrono::DateTime::<Utc>::MIN_UTC,
            extreme_price: dec!(0),
            extreme_volume: dec!(0),
            last_confirmed_price: None,
        }
    }

    /// Feed one sample. Returns at most one newly-confirmed pivot.
    /// Threshold is the absolute (not percent) reversal distance — the
    /// caller multiplies ATR by the level multiplier and passes the
    /// product here.
    pub fn on_sample(&mut self, s: &Sample, threshold: Decimal) -> Option<ConfirmedPivot> {
        if self.direction == Direction::Unknown {
            // Bootstrap: anchor on the very first sample. We don't yet
            // know which way the swing goes, so just hold the extreme.
            self.extreme_idx = s.bar_index;
            self.extreme_time = s.time;
            self.extreme_price = s.high; // arbitrary; refined below
            self.extreme_volume = s.volume;
            self.direction = Direction::Up;
            return None;
        }

        match self.direction {
            Direction::Up => self.handle_up(s, threshold),
            Direction::Down => self.handle_down(s, threshold),
            Direction::Unknown => unreachable!("bootstrapped above"),
        }
    }

    fn handle_up(&mut self, s: &Sample, threshold: Decimal) -> Option<ConfirmedPivot> {
        // Extending the up-swing: track new highs.
        if s.high >= self.extreme_price {
            self.extreme_idx = s.bar_index;
            self.extreme_time = s.time;
            self.extreme_price = s.high;
            self.extreme_volume = s.volume;
            return None;
        }
        // Check for reversal.
        if self.extreme_price - s.low >= threshold {
            let pivot = self.emit(PivotKind::High);
            self.direction = Direction::Down;
            self.extreme_idx = s.bar_index;
            self.extreme_time = s.time;
            self.extreme_price = s.low;
            self.extreme_volume = s.volume;
            return Some(pivot);
        }
        None
    }

    fn handle_down(&mut self, s: &Sample, threshold: Decimal) -> Option<ConfirmedPivot> {
        if s.low <= self.extreme_price {
            self.extreme_idx = s.bar_index;
            self.extreme_time = s.time;
            self.extreme_price = s.low;
            self.extreme_volume = s.volume;
            return None;
        }
        if s.high - self.extreme_price >= threshold {
            let pivot = self.emit(PivotKind::Low);
            self.direction = Direction::Up;
            self.extreme_idx = s.bar_index;
            self.extreme_time = s.time;
            self.extreme_price = s.high;
            self.extreme_volume = s.volume;
            return Some(pivot);
        }
        None
    }

    fn emit(&mut self, kind: PivotKind) -> ConfirmedPivot {
        let prominence = match self.last_confirmed_price {
            Some(prev) => (self.extreme_price - prev).abs(),
            None => dec!(0),
        };
        self.last_confirmed_price = Some(self.extreme_price);
        ConfirmedPivot {
            bar_index: self.extreme_idx,
            time: self.extreme_time,
            price: self.extreme_price,
            kind,
            prominence,
            volume_at_pivot: self.extreme_volume,
        }
    }
}

impl Default for ZigZag {
    fn default() -> Self {
        Self::new()
    }
}

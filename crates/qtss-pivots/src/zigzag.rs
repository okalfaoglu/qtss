//! ZigZag detector — trailing-window "swing extreme" model.
//!
//! This is the single source of truth for zigzag pivots across the
//! entire system: the worker writes them to the `pivots` table, every
//! pattern detector consumes them through `PivotTree`, and the GUI
//! read path (`GET /v2/zigzag/...`) calls the same function.
//!
//! # Algorithm (EmreKb / TradingView-style)
//!
//! For each bar we maintain a rolling window of `length` bars and two
//! signals:
//!
//! * `to_up   = bar.high >= max(window.high)` — current bar ties or
//!   beats the window's highest high.
//! * `to_down = bar.low  <= min(window.low)`  — current bar ties or
//!   beats the window's lowest low.
//!
//! A `trend` state tracks direction (+1 up, -1 down). It flips to -1
//! the first time `to_down` fires while in uptrend; flips to +1 the
//! first time `to_up` fires while in downtrend.
//!
//! Between flips we accumulate two swing candidates:
//!
//! * **High candidate**: highest high seen since the last `to_down`.
//!   Tied highs overwrite (most-recent bar wins — matches Pine's
//!   `bar_index - ta.barssince(high_val == high)` idiom).
//! * **Low candidate**: lowest low seen since the last `to_up`. Tied
//!   lows overwrite likewise.
//!
//! On trend flip up→down we emit the accumulated high candidate as a
//! confirmed HIGH pivot (the peak of the just-ended uptrend). Mirror
//! for flip down→up → LOW pivot.
//!
//! # Why this instead of centered `ta.pivothigh`
//!
//! The strict centered-pivot model (`ta.pivothigh(length, 1)`) rejects
//! any bar whose high equals either a neighbor — very common in real
//! OHLCV when a tall spike is followed by a bar whose upper wick
//! retests the same level. That invisibly "swallows" obvious swing
//! tops. The trailing-window model tolerates ties (picks the most
//! recent) and guarantees that every swing between trend flips gets
//! exactly one pivot.
//!
//! Trade-off: confirmation lag is no longer a fixed 1 bar — it's the
//! number of bars until trend flips. A pivot is never revised once
//! emitted, but *pending* swing extremes stay unconfirmed until the
//! opposite signal fires.

use chrono::{DateTime, Utc};
use qtss_domain::v2::pivot::PivotKind;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::VecDeque;

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

/// Unconfirmed running extreme — kept for API back-compat with the
/// old API. Trailing-window model has no "provisional" concept that
/// render helpers can paint mid-swing (the candidate bar is already
/// historical by the time it's tracked), so this always returns
/// `None`. Chart renderers fall back to the last confirmed pivot.
#[derive(Debug, Clone)]
pub struct ProvisionalExtreme {
    pub bar_index: u64,
    pub time: DateTime<Utc>,
    pub price: Decimal,
    pub kind: PivotKind,
    pub volume_at_pivot: Decimal,
}

#[derive(Debug, Clone)]
struct Candidate {
    bar_index: u64,
    time: DateTime<Utc>,
    price: Decimal,
    volume: Decimal,
}

#[derive(Debug, Clone)]
pub struct ZigZag {
    /// Rolling-window length — `to_up`/`to_down` compare the current
    /// bar's high/low against the max/min of the last `length` bars
    /// (inclusive). Larger values yield fewer, more significant
    /// pivots; smaller values yield more, finer pivots. Pine's
    /// `zigzag_len` parameter. EmreKb's MSB-OB default is 9.
    length: u32,
    /// Rolling high/low window.
    window: VecDeque<Sample>,
    /// Trend state: +1 up, -1 down. Initialized +1.
    trend: i8,
    /// High candidate tracked since the last `to_down` event.
    /// Ties on `>=` overwrite (most-recent bar wins).
    high_cand: Option<Candidate>,
    /// Low candidate tracked since the last `to_up` event.
    low_cand: Option<Candidate>,
    /// Last confirmed pivot price — feeds `prominence`.
    last_confirmed_price: Option<Decimal>,
}

impl ZigZag {
    /// Construct with trailing-window `length` (minimum 1).
    pub fn with_length(length: u32) -> Self {
        Self {
            length: length.max(1),
            window: VecDeque::with_capacity(length.max(1) as usize),
            trend: 1,
            high_cand: None,
            low_cand: None,
            last_confirmed_price: None,
        }
    }

    /// Back-compat alias — defaults to length 3 (Z1 in the Fibonacci
    /// ladder 3/5/8/13/21).
    pub fn new() -> Self {
        Self::with_length(3)
    }

    /// Back-compat shim for callers still passing an ATR threshold
    /// (old trailing-window API). The threshold is ignored.
    pub fn on_sample(&mut self, s: &Sample, _unused_threshold: Decimal) -> Option<ConfirmedPivot> {
        self.push(s)
    }

    /// Feed one bar. Returns at most one pivot — the one confirmed by
    /// this bar's trend-flip, if any.
    pub fn push(&mut self, s: &Sample) -> Option<ConfirmedPivot> {
        self.window.push_back(s.clone());
        while self.window.len() > self.length as usize {
            self.window.pop_front();
        }

        // Warmup: need a full window before to_up/to_down are meaningful.
        // Still seed the candidates so early swings aren't missed when
        // the window fills.
        if self.window.len() < self.length as usize {
            self.update_high_cand(s);
            self.update_low_cand(s);
            return None;
        }

        let rolling_high = self.window.iter().map(|b| b.high).max().expect("window non-empty");
        let rolling_low = self.window.iter().map(|b| b.low).min().expect("window non-empty");
        let to_up = s.high >= rolling_high;
        let to_down = s.low <= rolling_low;

        // Include current bar in both accumulators before possibly
        // emitting — matches Pine where `low_val` / `high_val` are
        // computed every bar from the full "since last opposite event"
        // range, including the current one.
        self.update_high_cand(s);
        self.update_low_cand(s);

        // Trend flip detection.
        let new_trend = if self.trend == 1 && to_down {
            -1
        } else if self.trend == -1 && to_up {
            1
        } else {
            self.trend
        };

        let emitted = if new_trend != self.trend {
            let pivot = if new_trend == -1 {
                // Uptrend ended → confirm the HIGH pivot.
                self.high_cand.clone().map(|c| self.emit(c, PivotKind::High))
            } else {
                // Downtrend ended → confirm the LOW pivot.
                self.low_cand.clone().map(|c| self.emit(c, PivotKind::Low))
            };
            self.trend = new_trend;
            pivot
        } else {
            None
        };

        // Reset the OPPOSITE candidate on each event — it now starts
        // tracking from this bar forward. On a to_up event the new
        // swing's low starts here (nothing was lower yet); mirror for
        // to_down.
        if to_up {
            self.low_cand = Some(cand_from(s, s.low));
            // Current bar also participates in the high cand (already
            // updated via update_high_cand above).
        }
        if to_down {
            self.high_cand = Some(cand_from(s, s.high));
        }

        emitted
    }

    fn update_high_cand(&mut self, s: &Sample) {
        let overwrite = match &self.high_cand {
            // Strictly lower → keep previous.
            Some(c) if c.price > s.high => false,
            // Equal or higher → overwrite (most-recent tie wins).
            _ => true,
        };
        if overwrite {
            self.high_cand = Some(cand_from(s, s.high));
        }
    }

    fn update_low_cand(&mut self, s: &Sample) {
        let overwrite = match &self.low_cand {
            Some(c) if c.price < s.low => false,
            _ => true,
        };
        if overwrite {
            self.low_cand = Some(cand_from(s, s.low));
        }
    }

    fn emit(&mut self, c: Candidate, kind: PivotKind) -> ConfirmedPivot {
        let prominence = match self.last_confirmed_price {
            Some(p) => (c.price - p).abs(),
            None => dec!(0),
        };
        self.last_confirmed_price = Some(c.price);
        ConfirmedPivot {
            bar_index: c.bar_index,
            time: c.time,
            price: c.price,
            kind,
            prominence,
            volume_at_pivot: c.volume,
        }
    }

    /// Trailing-window pivots are confirmed only on trend flip; no
    /// provisional extreme is surfaced to chart renderers. Kept for
    /// API back-compat; always returns `None`.
    pub fn provisional_extreme(&self) -> Option<ProvisionalExtreme> {
        None
    }
}

impl Default for ZigZag {
    fn default() -> Self {
        Self::new()
    }
}

fn cand_from(s: &Sample, price: Decimal) -> Candidate {
    Candidate {
        bar_index: s.bar_index,
        time: s.time,
        price,
        volume: s.volume,
    }
}

/// Batch helper — runs the same trailing-window algorithm over a whole
/// bar slice. Trend flips naturally alternate H/L/H/L so the output
/// sequence needs no further dedup. Retained `kind != last.kind`
/// guard as a belt-and-braces check against pathological inputs.
pub fn compute_pivots(bars: &[Sample], length: u32) -> Vec<ConfirmedPivot> {
    let mut zz = ZigZag::with_length(length);
    let mut out: Vec<ConfirmedPivot> = Vec::new();
    for s in bars {
        if let Some(p) = zz.push(s) {
            match out.last() {
                Some(last) if last.kind == p.kind => {
                    // Shouldn't happen with trailing-window, but if it
                    // does, keep whichever is more extreme.
                    let keep_new = match p.kind {
                        PivotKind::High => p.price >= last.price,
                        PivotKind::Low => p.price <= last.price,
                    };
                    if keep_new {
                        *out.last_mut().unwrap() = p;
                    }
                }
                _ => out.push(p),
            }
        }
    }
    out
}

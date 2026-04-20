//! ZigZag detector — LuxAlgo Pine `zigzag()` birebir parite.
//!
//! Referans (LuxAlgo Elliott Waves Pine):
//! ```pine
//! export method pivots(int length, bool patternWaitForClose) =>
//!     float phigh = ta.highestbars(src, length) == 0 ? high : na
//!     float plow  = ta.lowestbars(src, length)  == 0 ? low  : na
//!     dir = 0
//!     iff_1 = plow and na(phigh) ? -1 : dir[1]
//!     dir := phigh and na(plow) ? 1  : iff_1
//!     [dir, phigh, plow]
//!
//! export method zigzag(length, ..., zigzagpivots, zigzagpivotbars, ...) =>
//!     [dir, phigh, plow] = pivots(length, patternWaitForClose)
//!     dirchanged = ta.change(dir)
//!     if phigh or plow
//!         value = dir == 1 ? phigh : plow
//!         bar   = bar_index
//!         if not dirchanged and size >= 1
//!             pivot = shift()
//!             useNewValues = value * pivotdir < pivot * pivotdir
//!             value := useNewValues ? pivot : value
//!         unshift(pivots, value)
//! ```
//!
//! # Çeviri
//!
//! * `ta.highestbars(high, length) == 0` ⇔ current bar, son `length` barlık
//!   **trailing** pencerede maksimum. Pencere tek taraflı (yalnızca geriye
//!   bakar). Bu yüzden LuxAlgo pivotu anlık — confirm lag yok, ZigZag
//!   çizgisi son muma kadar uzanır.
//! * Direction: phigh-only → +1, plow-only → -1, hem de/hem de yok → önceki
//!   dir korunur.
//! * Direction değişmediyse (`!dirchanged`): en son pivotu POP et, yeni aday
//!   daha mı ekstrem kontrol et; değilse eski değeri geri yaz (running
//!   extreme update).
//! * Direction değiştiyse: önceki pivot kilitlenir (emit edilir), yeni aday
//!   yeni pending olur.
//!
//! # Bizim API
//!
//! ZigZag'in C ABI'si aynen: `push(&Sample) -> Option<ConfirmedPivot>`.
//! Pivot ancak **direction flip** anında emit edilir — böylece aynı swing
//! içinde running update'ler `pending` olarak kalır, `provisional_extreme`
//! ile dışarıya açılır (render tarafı son muma kadar çizebilsin).

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

/// Unconfirmed running extreme — the "front" of the pivot array in
/// LuxAlgo's model. Updated every bar a same-direction extreme appears;
/// emitted (and locked) on direction flip. Exposed for chart rendering
/// so the ZigZag line can reach the current bar.
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
    /// `length` as in LuxAlgo — trailing window size. Current bar is a
    /// pivot high/low iff it is the extremum of the last `length` bars
    /// (current bar included).
    length: u32,
    /// Rolling buffer of the last `length` samples.
    buf: VecDeque<Sample>,
    /// Current swing direction. `Unknown` until the first
    /// phigh-xor-plow resolves it.
    direction: Direction,
    /// Running extreme — equivalent to Pine's `array.get(pivots, 0)`.
    /// Locked (emitted) on direction flip.
    pending: Option<Candidate>,
    /// Last **confirmed** pivot price — for prominence computation.
    last_confirmed_price: Option<Decimal>,
}

impl ZigZag {
    /// Construct with LuxAlgo `length`. The pivot-window is one-sided
    /// (trailing) so confirmation lag is zero — the running extreme
    /// updates on every qualifying bar and locks only on direction flip.
    pub fn with_length(length: u32) -> Self {
        Self {
            length: length.max(1),
            buf: VecDeque::with_capacity(length.max(1) as usize),
            direction: Direction::Unknown,
            pending: None,
            last_confirmed_price: None,
        }
    }

    /// Back-compat alias — defaults to length 4 (LuxAlgo L0).
    pub fn new() -> Self {
        Self::with_length(4)
    }

    /// Back-compat shim for the old ATR-threshold API signature. The
    /// `_unused_threshold` is discarded — kept only so callers that
    /// still pass ATR values compile.
    pub fn on_sample(&mut self, s: &Sample, _unused_threshold: Decimal) -> Option<ConfirmedPivot> {
        self.push(s)
    }

    /// Feed one bar. Returns a pivot only on **direction flip** — that
    /// is the moment the previous running extreme becomes structurally
    /// fixed in LuxAlgo's model. Same-direction updates are absorbed
    /// silently into `pending` (visible via `provisional_extreme`).
    pub fn push(&mut self, s: &Sample) -> Option<ConfirmedPivot> {
        self.buf.push_back(s.clone());
        while self.buf.len() > self.length as usize {
            self.buf.pop_front();
        }
        if self.buf.len() < self.length as usize {
            return None;
        }

        // `ta.highestbars(high, length) == 0`  ⇔ current bar is the
        // MAX of the last `length` bars (current included). Non-strict
        // (`<=`) so ties still qualify — matches Pine's `ta.highestbars`
        // which returns 0 for the most recent bar when tied.
        let current = self.buf.back().cloned()?;
        let is_high = self.buf.iter().all(|b| b.high <= current.high);
        let is_low  = self.buf.iter().all(|b| b.low  >= current.low);

        // Pine's dir resolver:
        //   iff_1 = plow  and na(phigh) ? -1 : dir[1]
        //   dir  := phigh and na(plow)  ? +1 : iff_1
        // i.e. a *pure* high → Up, a *pure* low → Down, both-or-neither
        // → keep previous direction.
        let prev_dir = self.direction;
        let new_dir = match (is_high, is_low) {
            (true, false) => Direction::Up,
            (false, true) => Direction::Down,
            _             => prev_dir,
        };
        let dir_changed = new_dir != prev_dir && prev_dir != Direction::Unknown;
        self.direction = new_dir;

        // If neither phigh nor plow, nothing to do — Pine exits early
        // via `if phigh or plow`.
        if !is_high && !is_low {
            return None;
        }
        // Direction still unknown (first bar that qualifies) → seed.
        if new_dir == Direction::Unknown {
            return None;
        }

        let (kind, price) = match new_dir {
            Direction::Up   => (PivotKind::High, current.high),
            Direction::Down => (PivotKind::Low,  current.low),
            Direction::Unknown => unreachable!(),
        };
        let candidate = Candidate {
            bar_index: current.bar_index,
            time: current.time,
            price,
            kind,
            volume: current.volume,
        };

        match self.pending.take() {
            None => {
                // First pivot ever — seed pending, nothing to emit yet.
                self.pending = Some(candidate);
                None
            }
            Some(prev) if !dir_changed => {
                // Same direction — Pine's rollback branch:
                //   useNewValues = value * pivotdir < pivot * pivotdir
                //   (true → revert to previous pivot)
                // For High (dir=+1): useNewValues = value < pivot → keep old
                // For Low  (dir=-1): useNewValues = -value < -pivot → value>pivot → keep old
                // I.e. keep whichever is more extreme in the swing direction.
                let keep_new = match kind {
                    PivotKind::High => candidate.price >= prev.price,
                    PivotKind::Low  => candidate.price <= prev.price,
                };
                self.pending = Some(if keep_new { candidate } else { prev });
                None
            }
            Some(prev) => {
                // Direction flip — lock the previous running extreme as
                // a confirmed pivot; the new candidate becomes the new
                // pending.
                let pivot = self.emit(&prev);
                self.pending = Some(candidate);
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

    /// Current running extreme (Pine's `array.get(pivots, 0)`). Always
    /// reachable to the latest bar that qualified as phigh/plow —
    /// matches LuxAlgo's visual ZigZag which reaches the current bar.
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

//! ZigZag detector — 1:1 port of HeWhoMustNotBeNamed's Pine v4
//! `zigzag` export method.
//!
//! # Algorithm
//!
//! Per bar `s`:
//!
//! 1. **Window signals** — keep a rolling window of `length` bars.
//!    - `high_hit = s.high >= max(window.high)`
//!    - `low_hit  = s.low  <= min(window.low)`
//!
//! 2. **Exclusive direction signal** — Pine:
//!    ```text
//!    dir := phigh and na(plow) ? 1 : (plow and na(phigh) ? -1 : dir[1])
//!    ```
//!    So the current bar fires `phigh` only when it hits the window
//!    high **without** hitting the window low (and vice-versa). Outside
//!    bars that hit both leave direction unchanged — this is what stops
//!    the zigzag from flipping on every spike inside a consolidation.
//!
//! 3. **Skip if neither signal fires** — nothing to emit.
//!
//! 4. **Pivot emission** — `value = dir == 1 ? s.high : s.low`.
//!    - If the direction **didn't change** and a pivot already exists,
//!      compare `value` to the current head:
//!      - Head stays if it's more extreme (Pine: `useNewValues = value *
//!        pivotdir < pivot * pivotdir`).
//!      - Otherwise replace the head (pop + push) so the zigzag anchor
//!        drifts to the latest bar at the running extreme.
//!    - If direction flipped, the new pivot is pushed without touching
//!      the previous head.
//!
//! 5. **Strong flag** — after the possible pop in step 4, compare the
//!    new value against the previous same-direction pivot (which now
//!    sits at `pivots[len-2]`). Pine encodes this as `newDir := dir*2`;
//!    we surface it as a boolean `strong` field. True means the new
//!    high beats the prior high (HH) or the new low undercuts the prior
//!    low (LL) — a Dow-style trend-continuation signal.
//!
//! # Running head vs confirmed pivots
//!
//! Unlike a strict "emit on trend flip" model, this port writes a pivot
//! as soon as a signal fires and keeps replacing it while the same swing
//! extends. The last entry in `pivots()` is therefore the **running
//! head** — the current swing's live extreme, which may still drift to
//! the right on future bars. Only when the next opposite-direction
//! signal fires does the previous head become locked in as a confirmed
//! swing end. Chart renderers can treat every entry as drawable; pattern
//! detectors that need strict alternance must look at every pivot except
//! the last, or wait until a same-level pivot of the opposite kind has
//! pushed behind it.

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
    /// Absolute price distance to the previous pivot. Zero on the very
    /// first pivot.
    pub prominence: Decimal,
    pub volume_at_pivot: Decimal,
    /// Pine's `newDir` marker — encodes both direction and strength:
    ///   *  1 → normal high (LH: lower high vs previous high)
    ///   *  2 → strong high (HH: higher high vs previous high)
    ///   * -1 → normal low  (HL: higher low vs previous low)
    ///   * -2 → strong low  (LL: lower low vs previous low)
    /// Mirrors Pine's `newDir := dir * value > dir * LastPoint ? dir * 2 : dir`.
    pub direction: i8,
}

/// Back-compat shim — the new Pine-style model keeps the running head
/// as the last entry of [`ZigZag::pivots`], so there is no separate
/// "provisional" concept. Kept as an empty type so older callers in
/// [`crate::engine`] still compile.
#[derive(Debug, Clone)]
pub struct ProvisionalExtreme {
    pub bar_index: u64,
    pub time: DateTime<Utc>,
    pub price: Decimal,
    pub kind: PivotKind,
    pub volume_at_pivot: Decimal,
}

#[derive(Debug, Clone)]
pub struct ZigZag {
    length: u32,
    window: VecDeque<Sample>,
    /// Current direction: +1 up, -1 down, 0 until the first signal.
    dir: i8,
    /// Emitted pivots, oldest → newest. The last entry is the running
    /// head (may still be replaced by a later bar at the same swing's
    /// more extreme price).
    pivots: Vec<ConfirmedPivot>,
}

impl ZigZag {
    pub fn with_length(length: u32) -> Self {
        Self {
            length: length.max(1),
            window: VecDeque::with_capacity(length.max(1) as usize),
            dir: 0,
            pivots: Vec::new(),
        }
    }

    pub fn new() -> Self {
        Self::with_length(3)
    }

    /// Back-compat alias — old callers passed an ATR threshold which
    /// the trailing-window model does not use.
    pub fn on_sample(&mut self, s: &Sample, _unused: Decimal) -> Option<ConfirmedPivot> {
        self.push(s)
    }

    /// Feed one bar. Returns the pivot emitted by this bar, if any:
    /// either a brand-new pivot (direction flipped, or first pivot
    /// ever), or a replacement for the current running head (same
    /// direction, more extreme value). Bars that fire no signal, or
    /// that would merely confirm a less-extreme same-direction value,
    /// return `None`.
    pub fn push(&mut self, s: &Sample) -> Option<ConfirmedPivot> {
        self.window.push_back(s.clone());
        while self.window.len() > self.length as usize {
            self.window.pop_front();
        }
        if self.window.len() < self.length as usize {
            return None;
        }

        let rh = self.window.iter().map(|b| b.high).max().expect("non-empty window");
        let rl = self.window.iter().map(|b| b.low).min().expect("non-empty window");
        let high_hit = s.high >= rh;
        let low_hit = s.low <= rl;

        let phigh = high_hit && !low_hit;
        let plow = low_hit && !high_hit;

        if !(phigh || plow) {
            return None;
        }

        let new_dir: i8 = if phigh { 1 } else { -1 };
        let dirchanged = new_dir != self.dir;
        let (value, kind) = if new_dir == 1 {
            (s.high, PivotKind::High)
        } else {
            (s.low, PivotKind::Low)
        };

        // Step A: same direction → keep the more extreme of {head, new}.
        if !dirchanged {
            if let Some(last) = self.pivots.last() {
                let more_extreme = match kind {
                    PivotKind::High => value > last.price,
                    PivotKind::Low => value < last.price,
                };
                if !more_extreme {
                    // Old head wins; nothing emitted.
                    self.dir = new_dir;
                    return None;
                }
                self.pivots.pop();
            }
        }

        // Step B: strength marker — Pine's `newDir := dir * value >
        // dir * LastPoint ? dir * 2 : dir`. Compare `value` against the
        // previous same-direction pivot. After the optional pop in
        // step A, that pivot sits at `pivots[len - 2]` regardless of
        // `dirchanged`:
        //   - dirchanged=true:  [..., prev_same, prev_opp]  (no pop).
        //     The head is opposite-dir; prev-same-dir is two back.
        //   - dirchanged=false (after pop): [..., prev_same_2, prev_opp].
        //     Same slot; prev_same_2 is the prior same-dir pivot.
        let strong = self.pivots.len() >= 2 && {
            let prev_same = &self.pivots[self.pivots.len() - 2];
            match kind {
                PivotKind::High => value > prev_same.price,
                PivotKind::Low => value < prev_same.price,
            }
        };
        let direction: i8 = if strong { new_dir * 2 } else { new_dir };

        let prominence = match self.pivots.last() {
            Some(p) => (value - p.price).abs(),
            None => dec!(0),
        };

        let pivot = ConfirmedPivot {
            bar_index: s.bar_index,
            time: s.time,
            price: value,
            kind,
            prominence,
            volume_at_pivot: s.volume,
            direction,
        };

        self.pivots.push(pivot.clone());
        self.dir = new_dir;
        Some(pivot)
    }

    /// All pivots the detector has seen so far, oldest first. The last
    /// entry is the running head (may still be replaced).
    pub fn pivots(&self) -> &[ConfirmedPivot] {
        &self.pivots
    }

    /// Back-compat shim. The Pine-style model folds the running extreme
    /// into the main pivot array, so there is no separate provisional
    /// surface. Always `None`; callers should use [`Self::pivots`] and
    /// treat the last entry as the running head.
    pub fn provisional_extreme(&self) -> Option<ProvisionalExtreme> {
        None
    }
}

impl Default for ZigZag {
    fn default() -> Self {
        Self::new()
    }
}

/// Batch helper — runs the detector over a full bar slice and returns
/// every pivot it emitted, oldest first. The last entry is the running
/// head at the end of the series.
pub fn compute_pivots(bars: &[Sample], length: u32) -> Vec<ConfirmedPivot> {
    let mut zz = ZigZag::with_length(length);
    for s in bars {
        zz.push(s);
    }
    zz.pivots().to_vec()
}

/// Post-filter: drop pivot pairs whose `|Δprice| / prev_price < min_pct`,
/// absorbing them into the surrounding swing. Matches the TradingView /
/// LuxAlgo visual where tiny retracements inside a large move do not
/// break the zigzag into noisy sub-legs. A threshold ≤ 0 is a no-op.
///
/// Invariant preserved: the output still alternates H/L/H/L. When a
/// pair `[last, cand]` cancels because their swing is too small, `last`
/// is popped; if the new tail and `cand` are the same kind they merge
/// into whichever is more extreme; if they are opposite kind we
/// re-evaluate against the new tail. If the pop empties the list,
/// `cand` is dropped too — the cancelled swing had no anchor on either
/// side.
pub fn filter_prominence(pivots: &[ConfirmedPivot], min_pct: f64) -> Vec<ConfirmedPivot> {
    if min_pct <= 0.0 || pivots.len() < 2 {
        return pivots.to_vec();
    }
    use rust_decimal::prelude::ToPrimitive;
    let mut out: Vec<ConfirmedPivot> = Vec::with_capacity(pivots.len());
    for cand in pivots.iter().cloned() {
        let mut absorbed = false;
        loop {
            let Some(last) = out.last() else { break; };
            let base = last.price.to_f64().unwrap_or(0.0).abs().max(1e-9);
            let delta = (cand.price - last.price).to_f64().unwrap_or(0.0).abs();
            if delta / base >= min_pct {
                break;
            }
            out.pop();
            if let Some(prev) = out.last_mut() {
                if prev.kind == cand.kind {
                    let keep_new = match cand.kind {
                        PivotKind::High => cand.price > prev.price,
                        PivotKind::Low => cand.price < prev.price,
                    };
                    if keep_new {
                        *prev = cand.clone();
                    }
                    absorbed = true;
                    break;
                }
                continue;
            }
            absorbed = true;
            break;
        }
        if !absorbed {
            out.push(cand);
        }
    }
    out
}

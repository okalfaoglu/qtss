//! LuxAlgo Elliott Wave — motive/ABC/fib state machine on top of the
//! canonical trailing-window zigzag in `qtss_pivots::zigzag`.
//!
//! Reference: `reference/luxalgo/elliott_wave_luxalgo.pine` (© LuxAlgo,
//! CC BY-NC-SA 4.0). Pine's state machine (impulse 12345 / ABC / fib
//! band / break box / "(5)(1)" & "(b)(1)" label fusion) is ported 1:1.
//!
//! What is **not** ported is Pine's `ta.pivothigh(src, left, 1)` —
//! the centered-strict pivot. That primitive silently drops swing tops
//! whenever two adjacent bars share the same high (see `zigzag.rs` docs
//! for the 2026-04-07 incident). Instead we feed the state machine the
//! same pivots the rest of the system uses — `compute_pivots` from
//! `qtss-pivots`, a trailing-window / trend-flip detector with tie
//! tolerance. Every downstream layer (worker → DB, API zigzag route,
//! every Elliott / harmonic / Wyckoff detector, this Pine port) thus
//! agrees on "where the pivots are" by construction.
//!
//! Trade-offs of this substitution:
//!   * Visual parity: the zigzag you see on the chart is the same pivot
//!     set the detectors score against. No more "TS port found a motive
//!     at bars the backend zigzag doesn't even mark."
//!   * `HiSource::Close` / `HiSource::MaxOpenClose` etc. are accepted
//!     in the config for API shape compatibility but ignored —
//!     trailing-window only understands high/low.
//!   * Confirmation lag is "until next trend flip" instead of Pine's
//!     fixed 1-bar right-confirmation. Waves therefore appear slightly
//!     later than in TradingView's LuxAlgo indicator.
//!
//! Output is a plain-data snapshot — no rendering primitives — so the
//! caller (qtss-api → React chart) can draw it however it wants.

use qtss_domain::v2::pivot::PivotKind;
use qtss_pivots::zigzag::{compute_pivots, Sample};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::Serialize;

/// Which price field feeds the high / low streams. Mirrors the Pine
/// `i_hi` / `i_lo` inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiSource {
    High,
    Close,
    MaxOpenClose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoSource {
    Low,
    Close,
    MinOpenClose,
}

/// One OHLC bar. We only read open/high/low/close here (Pine does the
/// same in this indicator — volume is unused).
#[derive(Debug, Clone)]
pub struct Bar {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

/// Per-zigzag-level configuration. Pine ships three levels by default
/// (`len=4`, `len=8`, `len=16`, colors red/blue/white). The caller is
/// free to pass any subset.
#[derive(Debug, Clone)]
pub struct LevelConfig {
    pub length: usize,
    pub color: String,
}

/// Top-level indicator configuration. Defaults reproduce the Pine
/// `Elliott Wave [LuxAlgo]` indicator out of the box.
#[derive(Debug, Clone)]
pub struct PinePortConfig {
    pub hi_source: HiSource,
    pub lo_source: LoSource,
    pub levels: Vec<LevelConfig>,
    pub fib_level_1: f64,
    pub fib_level_2: f64,
    pub fib_level_3: f64,
    pub fib_level_4: f64,
}

impl Default for PinePortConfig {
    fn default() -> Self {
        Self {
            hi_source: HiSource::High,
            lo_source: LoSource::Low,
            levels: vec![
                LevelConfig { length: 4, color: "#ef4444".into() },
                LevelConfig { length: 8, color: "#3b82f6".into() },
                LevelConfig { length: 16, color: "#e5e7eb".into() },
            ],
            fib_level_1: 0.500,
            fib_level_2: 0.618,
            fib_level_3: 0.764,
            fib_level_4: 0.854,
        }
    }
}

//-----------------------------------------------------------------------------
// Output DTOs — plain data the caller (API → UI) can draw directly.
//-----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct PivotPoint {
    /// `+1` = high pivot, `-1` = low pivot. Mirrors Pine's `d` array.
    pub direction: i8,
    pub bar_index: i64,
    pub price: f64,
    /// Pine `label.set_text(...)` override. Present when two label
    /// spots collide at the same bar and Pine fuses them, e.g.
    /// "(5) (1)" or "(b) (1)".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_override: Option<String>,
    /// Pine `label.set_textcolor(color(na))` equivalent — skip
    /// rendering this anchor's text (the fused twin shows it).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hide_label: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MotivePattern {
    /// `+1` bullish, `-1` bearish.
    pub direction: i8,
    /// Anchors p0..p5 in chronological order.
    /// Pine's `_1x` is the oldest (p0), `_6x` is the newest (p5).
    pub anchors: [PivotPoint; 6],
    /// Pine's `on` flag — set to `false` via `gEW.dot()` once the
    /// pattern has been invalidated (a later same-signature pivot
    /// failed `isWave`). The wave stays on-screen dotted.
    pub live: bool,
    /// Pine's `next` flag — at least one post-ABC pivot crossed back
    /// through the wave's p5 level, hinting at a new (1).
    pub next_hint: bool,
    /// Optional ABC correction attached to this motive.
    pub abc: Option<AbcPattern>,
    /// Pine's post-wave "break box": a rectangle from p5 to `p5 + (p5.x - p1.x)` horizontally
    /// and bounded vertically by p4 and p5. A break of its far edge
    /// triggers a cross marker (see [`BreakMarker`]).
    pub break_box: Option<BreakBox>,
    /// Pine's `lb` — small circle drawn above/below the pivot when a
    /// potential new (1) is printed after the ABC.
    pub next_marker: Option<NextMarker>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AbcPattern {
    /// `+1` = bullish ABC (follows a bearish motive), `-1` = bearish
    /// ABC (follows a bullish motive). Matches Pine's convention
    /// (`dir == -1` → bullish ABC draw).
    pub direction: i8,
    /// 4 anchors: [p5_of_parent, a, b, c].
    pub anchors: [PivotPoint; 4],
    /// `true` when the ABC was invalidated via `gEW.dash()`.
    pub invalidated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BreakBox {
    pub left_bar: i64,
    pub right_bar: i64,
    pub top: f64,
    pub bottom: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BreakMarker {
    /// `+1` = price broke above the bear-box top, `-1` = below bull-box bottom.
    pub direction: i8,
    pub bar_index: i64,
    pub price: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NextMarker {
    pub direction: i8,
    pub bar_index: i64,
    pub price: f64,
}

/// One full level worth of output (one Pine `draw()` call).
#[derive(Debug, Clone, Serialize)]
pub struct LevelOutput {
    pub length: usize,
    pub color: String,
    /// All pivots we ever produced for this level, in chronological
    /// order — handy if the UI wants to draw the zigzag itself.
    pub pivots: Vec<PivotPoint>,
    /// All motive waves produced (Pine keeps only the last 15 on
    /// screen; we track the same cap).
    pub motives: Vec<MotivePattern>,
    pub break_markers: Vec<BreakMarker>,
    /// Pine's final fib band (drawn only on `barstate.islast` for the
    /// freshest wave). Present only when the wave is still valid and
    /// hasn't yet spawned a full ABC beyond its 5.
    pub fib_band: Option<FibBand>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FibBand {
    pub x_anchor: i64,
    pub x_far: i64,
    pub pole_top: f64,
    pub pole_bottom: f64,
    pub y_500: f64,
    pub y_618: f64,
    pub y_764: f64,
    pub y_854: f64,
    pub broken: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PinePortOutput {
    pub bar_count: i64,
    pub levels: Vec<LevelOutput>,
}

/// Terse constructor — keeps the wave/ABC literals readable and makes
/// label_override / hide_label additions a one-line patch site.
#[inline]
fn pp(direction: i8, bar_index: i64, price: f64) -> PivotPoint {
    PivotPoint { direction, bar_index, price, label_override: None, hide_label: false }
}

//-----------------------------------------------------------------------------
// State machine — per-level, private to this module.
//-----------------------------------------------------------------------------

const ZZ_RING_SIZE: usize = 11;
const EW_CAP: usize = 15;

#[derive(Debug, Clone, Copy)]
struct ZzSlot {
    d: i8,
    x: i64,
    y: f64,
}

#[derive(Debug)]
struct LevelState {
    cfg: LevelConfig,
    /// Pine `aZZ`: ring of the 11 most recent pivots, newest at index 0.
    /// We use a fixed-size `Vec` and emulate `unshift`/`pop` manually.
    zz: Vec<ZzSlot>,
    /// Pine `aEW`: waves list, newest at index 0.
    ew: Vec<MotivePattern>,
    /// Chronological log of all pivots ever emitted for this level.
    pivots_log: Vec<PivotPoint>,
    break_markers: Vec<BreakMarker>,
}

impl LevelState {
    fn new(cfg: LevelConfig) -> Self {
        Self {
            cfg,
            zz: vec![ZzSlot { d: 0, x: 0, y: 0.0 }; ZZ_RING_SIZE],
            ew: Vec::new(),
            pivots_log: Vec::new(),
            break_markers: Vec::new(),
        }
    }

    /// Pine `in_out(aZZ, d, x1, y1, x2, y2, col)` — push a new pivot
    /// to the front and drop the tail, flipping direction.
    fn zz_push(&mut self, d: i8, x: i64, y: f64) {
        // unshift(new) + pop(tail)
        self.zz.insert(0, ZzSlot { d, x, y });
        self.zz.pop();
    }

    fn zz_replace_front(&mut self, x: i64, y: f64) {
        self.zz[0].x = x;
        self.zz[0].y = y;
    }

    fn emit_pivot(&mut self, d: i8, x: i64, y: f64) {
        // Only log when it's a NEW pivot (direction flip) or an
        // extension. We call this after `zz_push` / `zz_replace_front`
        // to keep the log in chronological order.
        //
        // Replace: if the last emitted pivot has the same direction,
        // we're extending, so update it in place instead of appending.
        if let Some(last) = self.pivots_log.last_mut() {
            if last.direction == d && last.bar_index <= x {
                last.bar_index = x;
                last.price = y;
                return;
            }
        }
        self.pivots_log.push(pp(d, x, y));
    }
}

//-----------------------------------------------------------------------------
// Core — pivot handlers and bar-level helpers
//-----------------------------------------------------------------------------
//
// Pivots are produced by `qtss_pivots::zigzag::compute_pivots` on the
// bar slice; the state machine here only reacts to them. No per-bar
// pivot detection lives in this module anymore.

/// Provisional pivot = the most extreme opposite-direction bar since
/// the last confirmed pivot. Mirrors `v2_zigzag.rs` so the Elliott
/// state machine and the zigzag overlay agree on "the live swing's
/// tentative anchor" — otherwise wave labels would stop at the last
/// flip while the zigzag leg reaches the current bar.
///
/// Returns `None` when `pivots` is empty (no flip yet) or when no bars
/// follow the last confirmed pivot.
struct ProvisionalPivot {
    kind: PivotKind,
    bar_index: i64,
    price: f64,
}

fn provisional_pivot(pivots: &[qtss_pivots::zigzag::ConfirmedPivot], bars: &[Bar]) -> Option<ProvisionalPivot> {
    let last = pivots.last()?;
    let start = last.bar_index as usize + 1;
    if start >= bars.len() { return None; }

    // After a HIGH confirmation → pending swing is a LOW; mirror for LOW.
    let look_for_low = matches!(last.kind, PivotKind::High);
    let prov_kind = if look_for_low { PivotKind::Low } else { PivotKind::High };

    let mut best_idx = start;
    let mut best_price = if look_for_low { bars[start].low } else { bars[start].high };
    for i in (start + 1)..bars.len() {
        let p = if look_for_low { bars[i].low } else { bars[i].high };
        let better = if look_for_low { p < best_price } else { p > best_price };
        if better { best_idx = i; best_price = p; }
    }
    Some(ProvisionalPivot { kind: prov_kind, bar_index: best_idx as i64, price: best_price })
}

/// Convert Pine port's f64 Bar to the `Sample` flavour `compute_pivots`
/// wants. `MAX` for volume is fine — compute_pivots doesn't use it for
/// the pivot decision, only records it on the emitted pivot.
fn samples_from_bars(bars: &[Bar]) -> Vec<Sample> {
    bars.iter()
        .enumerate()
        .map(|(i, b)| Sample {
            bar_index: i as u64,
            time: chrono::Utc::now(), // not used by compute_pivots for ordering
            high: Decimal::from_f64_retain(b.high).unwrap_or_default(),
            low: Decimal::from_f64_retain(b.low).unwrap_or_default(),
            volume: Decimal::ZERO,
        })
        .collect()
}

/// Pine `isSame(gEW, _1x, _2x, _3x, _4x)` — the four earlier anchors
/// match the existing wave's first four. We compare by bar_index
/// (Pine compares line x-coords which are bar indices).
fn is_same_motive(m: &MotivePattern, _1x: i64, _2x: i64, _3x: i64, _4x: i64) -> bool {
    m.anchors[0].bar_index == _1x
        && m.anchors[1].bar_index == _2x
        && m.anchors[2].bar_index == _3x
        && m.anchors[3].bar_index == _4x
}

/// Pine `isSame2(gEW, _1x, _2x, _3x)` — the last three anchors of the
/// last motive (p3, p4, p5) match the incoming (_1, _2, _3). Used to
/// extend an in-progress ABC's C leg rather than rebuild it.
fn is_same_abc(m: &MotivePattern, _1x: i64, _2x: i64, _3x: i64) -> bool {
    m.anchors[3].bar_index == _1x
        && m.anchors[4].bar_index == _2x
        && m.anchors[5].bar_index == _3x
}

fn process_high_pivot(
    state: &mut LevelState,
    cfg: &PinePortConfig,
    pivot_x: i64,
    pivot_y: f64,
) {
    // Trailing-window pivots already sit on the exact bar that is the
    // swing extreme, so the Pine `bar_index - 1` shift is gone.
    let x2 = pivot_x;
    let y2 = pivot_y;

    let dir = state.zz[0].d;
    let y1 = state.zz[0].y;

    if dir < 1 {
        state.zz_push(1, x2, y2);
        state.emit_pivot(1, x2, y2);
    } else if dir == 1 && y2 > y1 {
        state.zz_replace_front(x2, y2);
        state.emit_pivot(1, x2, y2);
    } else {
        // Pine silently keeps the older, higher pivot in place.
    }

    // Read 6 most recent anchors (p0 oldest = index 5; p5 newest = index 0)
    let zz = &state.zz;
    let _6x = zz[0].x; let _6y = zz[0].y;
    let _5x = zz[1].x; let _5y = zz[1].y;
    let _4x = zz[2].x; let _4y = zz[2].y;
    let _3x = zz[3].x; let _3y = zz[3].y;
    let _2x = zz[4].x; let _2y = zz[4].y;
    let _1x = zz[5].x; let _1y = zz[5].y;

    // ───── 12345 (bullish motive check) ─────
    let w5 = _6y - _5y;
    let w3 = _4y - _3y;
    let w1 = _2y - _1y;
    let min_leg = w1.min(w3).min(w5);
    let is_wave = w3 != min_leg && _6y > _4y && _3y > _1y && _5y > _2y;

    // `same` test uses the most-recent wave at index 0 (Pine gEW).
    let same_against_front = state
        .ew
        .first()
        .map(|m| is_same_motive(m, _1x, _2x, _3x, _4x))
        .unwrap_or(false);

    if is_wave {
        if same_against_front {
            // Extend the 5th leg of the existing wave.
            if let Some(front) = state.ew.first_mut() {
                front.anchors[5] = pp(1, _6x, _6y);
            }
        } else {
            // Pine: if _2x matches the prior wave's b5 x, relabel the
            // prior b5 to "" and mark this (1) as "(5) (1)".
            let mut first_override: Option<String> = None;
            if let Some(front) = state.ew.first_mut() {
                if _2x == front.anchors[5].bar_index {
                    front.anchors[5].hide_label = true;
                    first_override = Some("(5) (1)".into());
                }
            }
            let mut new_wave = MotivePattern {
                direction: 1,
                anchors: [
                    pp(-1, _1x, _1y),
                    pp(1, _2x, _2y),
                    pp(-1, _3x, _3y),
                    pp(1, _4x, _4y),
                    pp(-1, _5x, _5y),
                    pp(1, _6x, _6y),
                ],
                live: true,
                next_hint: false,
                abc: None,
                break_box: None,
                next_marker: None,
            };
            new_wave.anchors[1].label_override = first_override;
            state.ew.insert(0, new_wave);
            if state.ew.len() > EW_CAP {
                state.ew.pop();
            }
        }
    } else if same_against_front {
        // Pine `gEW.dot()`: mark invalidated.
        if let Some(front) = state.ew.first_mut() {
            if front.live {
                front.live = false;
            }
        }
    }

    // ───── ABC (bullish ABC follows a bearish motive: dir == -1) ─────
    // Pine reads gEW = aEW.get(0); if its direction is -1, test for a
    // bullish ABC using the just-captured _3y.._6y anchors.
    if let Some(front) = state.ew.first_mut() {
        if front.direction == -1 {
            let get_x = front.anchors[5].bar_index; // l5.x2
            let get_y = front.anchors[5].price;    // l5.y2
            let last_0y = front.anchors[0].price;  // l1.y1
            let diff = (get_y - last_0y).abs();
            let fib_854 = cfg.fib_level_4;

            let is_valid = _3x == get_x
                && _6y < get_y + diff * fib_854
                && _4y < get_y + diff * fib_854
                && _5y > get_y;

            let have_abc = front.abc.is_some();
            let same_abc = have_abc
                && front
                    .abc
                    .as_ref()
                    .map(|_| is_same_abc(front, _1x, _2x, _3x))
                    .unwrap_or(false)
                && front.abc.as_ref().unwrap().anchors[1].bar_index > _3x;

            if is_valid {
                let width = _6x - _2x;
                if same_abc {
                    // Extend C
                    let abc = front.abc.as_mut().unwrap();
                    abc.anchors[3] = pp(1, _6x, _6y);
                    front.break_box = Some(BreakBox {
                        left_bar: _6x,
                        right_bar: _6x + width,
                        top: _6y,
                        bottom: _4y,
                    });
                } else {
                    front.abc = Some(AbcPattern {
                        direction: 1,
                        anchors: [
                            pp(-1, _3x, _3y),
                            pp(1, _4x, _4y),
                            pp(-1, _5x, _5y),
                            pp(1, _6x, _6y),
                        ],
                        invalidated: false,
                    });
                    front.break_box = Some(BreakBox {
                        left_bar: _6x,
                        right_bar: _6x + width,
                        top: _6y,
                        bottom: _4y,
                    });
                }
            } else if same_abc {
                if let Some(abc) = front.abc.as_mut() {
                    abc.invalidated = true;
                }
            }
        }
    }

    // ───── new (1)? (after a bullish motive + bearish ABC: dir == 1) ─────
    if let Some(front) = state.ew.first_mut() {
        if front.direction == 1 {
            let has_abc_bearish = front
                .abc
                .as_ref()
                .map(|a| a.direction == -1 && a.anchors[2].bar_index == _5x)
                .unwrap_or(false);
            if has_abc_bearish && _6y > front.anchors[5].price && !front.next_hint {
                front.next_hint = true;
                front.next_marker = Some(NextMarker {
                    direction: 1,
                    bar_index: _6x,
                    price: _6y,
                });
            }
        }
    }
}

fn process_low_pivot(
    state: &mut LevelState,
    cfg: &PinePortConfig,
    pivot_x: i64,
    pivot_y: f64,
) {
    let x2 = pivot_x;
    let y2 = pivot_y;

    let dir = state.zz[0].d;
    let y1 = state.zz[0].y;

    if dir > -1 {
        state.zz_push(-1, x2, y2);
        state.emit_pivot(-1, x2, y2);
    } else if dir == -1 && y2 < y1 {
        state.zz_replace_front(x2, y2);
        state.emit_pivot(-1, x2, y2);
    }

    let zz = &state.zz;
    let _6x = zz[0].x; let _6y = zz[0].y;
    let _5x = zz[1].x; let _5y = zz[1].y;
    let _4x = zz[2].x; let _4y = zz[2].y;
    let _3x = zz[3].x; let _3y = zz[3].y;
    let _2x = zz[4].x; let _2y = zz[4].y;
    let _1x = zz[5].x; let _1y = zz[5].y;

    // ───── 12345 (bearish motive) ─────
    let w5 = _5y - _6y;
    let w3 = _3y - _4y;
    let w1 = _1y - _2y;
    let min_leg = w1.min(w3).min(w5);
    let is_wave = w3 != min_leg && _4y > _6y && _1y > _3y && _2y > _5y;

    let same_against_front = state
        .ew
        .first()
        .map(|m| is_same_motive(m, _1x, _2x, _3x, _4x))
        .unwrap_or(false);

    if is_wave {
        if same_against_front {
            if let Some(front) = state.ew.first_mut() {
                front.anchors[5] = pp(-1, _6x, _6y);
            }
        } else {
            // Mirror bullish fusion: hide the prior b5 and relabel this
            // wave's (1) as "(5) (1)" when they share a bar.
            let mut first_override: Option<String> = None;
            if let Some(front) = state.ew.first_mut() {
                if _2x == front.anchors[5].bar_index {
                    front.anchors[5].hide_label = true;
                    first_override = Some("(5) (1)".into());
                }
            }
            let mut new_wave = MotivePattern {
                direction: -1,
                anchors: [
                    pp(1, _1x, _1y),
                    pp(-1, _2x, _2y),
                    pp(1, _3x, _3y),
                    pp(-1, _4x, _4y),
                    pp(1, _5x, _5y),
                    pp(-1, _6x, _6y),
                ],
                live: true,
                next_hint: false,
                abc: None,
                break_box: None,
                next_marker: None,
            };
            new_wave.anchors[1].label_override = first_override;
            state.ew.insert(0, new_wave);
            if state.ew.len() > EW_CAP {
                state.ew.pop();
            }
        }
    } else if same_against_front {
        if let Some(front) = state.ew.first_mut() {
            if front.live {
                front.live = false;
            }
        }
    }

    // ───── bearish ABC (follows a bullish motive: dir == 1) ─────
    if let Some(front) = state.ew.first_mut() {
        if front.direction == 1 {
            let get_x = front.anchors[5].bar_index;
            let get_y = front.anchors[5].price;
            let last_0y = front.anchors[0].price;
            let diff = (get_y - last_0y).abs();
            let fib_854 = cfg.fib_level_4;

            let is_valid = _3x == get_x
                && _6y > get_y - diff * fib_854
                && _4y > get_y - diff * fib_854
                && _5y < get_y;

            let have_abc = front.abc.is_some();
            let same_abc = have_abc
                && is_same_abc(front, _1x, _2x, _3x)
                && front.abc.as_ref().unwrap().anchors[1].bar_index > _3x;

            if is_valid {
                let width = _6x - _2x;
                if same_abc {
                    let abc = front.abc.as_mut().unwrap();
                    abc.anchors[3] = pp(-1, _6x, _6y);
                    front.break_box = Some(BreakBox {
                        left_bar: _6x,
                        right_bar: _6x + width,
                        top: _4y,
                        bottom: _6y,
                    });
                } else {
                    front.abc = Some(AbcPattern {
                        direction: -1,
                        anchors: [
                            pp(1, _3x, _3y),
                            pp(-1, _4x, _4y),
                            pp(1, _5x, _5y),
                            pp(-1, _6x, _6y),
                        ],
                        invalidated: false,
                    });
                    front.break_box = Some(BreakBox {
                        left_bar: _6x,
                        right_bar: _6x + width,
                        top: _4y,
                        bottom: _6y,
                    });
                }
            } else if same_abc {
                if let Some(abc) = front.abc.as_mut() {
                    abc.invalidated = true;
                }
            }
        }
    }

    // ───── new (1)? after bearish motive + bullish ABC (dir == -1) ─────
    if let Some(front) = state.ew.first_mut() {
        if front.direction == -1 {
            let has_abc_bullish = front
                .abc
                .as_ref()
                .map(|a| a.direction == 1 && a.anchors[2].bar_index == _5x)
                .unwrap_or(false);
            if has_abc_bullish && _6y < front.anchors[5].price && !front.next_hint {
                front.next_hint = true;
                front.next_marker = Some(NextMarker {
                    direction: -1,
                    bar_index: _6x,
                    price: _6y,
                });
            }
        }
    }
}

/// Pine "check for break box" — if the latest motive has a break_box
/// and price pierced its far edge, emit a cross marker.
fn process_break_box(state: &mut LevelState, bars: &[Bar], bar_index: i64) {
    if let Some(front) = state.ew.first() {
        if let Some(bx) = &front.break_box {
            let b = bar_index as usize;
            if b >= bars.len() { return; }
            if bar_index <= bx.right_bar {
                if front.direction == 1 {
                    // Bullish motive → box sits below; a LOW breach prints an X.
                    if bars[b].low < bx.bottom {
                        state.break_markers.push(BreakMarker {
                            direction: -1,
                            bar_index,
                            price: bars[b].low,
                        });
                    }
                } else {
                    if bars[b].high > bx.top {
                        state.break_markers.push(BreakMarker {
                            direction: 1,
                            bar_index,
                            price: bars[b].high,
                        });
                    }
                }
            }
        }
    }
}

//-----------------------------------------------------------------------------
// Public entry point
//-----------------------------------------------------------------------------

/// Run the full Pine indicator over a chronological slice of bars.
/// Stateless from the caller's POV — the entire replay lives inside
/// this function.
pub fn run(bars: &[Bar], cfg: &PinePortConfig) -> PinePortOutput {
    let bar_count = bars.len() as i64;
    if bars.is_empty() {
        return PinePortOutput { bar_count, levels: Vec::new() };
    }

    let samples = samples_from_bars(bars);
    let mut levels: Vec<LevelOutput> = Vec::with_capacity(cfg.levels.len());

    for level_cfg in &cfg.levels {
        let mut state = LevelState::new(level_cfg.clone());

        // Feed the state machine the canonical zigzag for this level.
        // Pivots arrive in chronological order, alternating High/Low
        // (compute_pivots guarantees both), so the `aZZ` ring sees the
        // exact sequence Pine would — just derived from a tie-tolerant
        // detector instead of `ta.pivothigh`.
        let pivots = compute_pivots(&samples, level_cfg.length as u32);
        let mut pivot_idx = 0usize;

        for b in 0..bars.len() {
            while pivot_idx < pivots.len() && pivots[pivot_idx].bar_index as usize == b {
                let p = &pivots[pivot_idx];
                let px = p.bar_index as i64;
                let py = p.price.to_f64().unwrap_or(0.0);
                match p.kind {
                    PivotKind::High => process_high_pivot(&mut state, cfg, px, py),
                    PivotKind::Low => process_low_pivot(&mut state, cfg, px, py),
                }
                pivot_idx += 1;
            }
            process_break_box(&mut state, bars, b as i64);
        }

        // Trailing-window pivots only emit on trend flip — the current
        // in-progress swing is not in `pivots`. `/v2/zigzag` handles that
        // by synthesizing a "provisional" pivot = the most extreme
        // opposite-direction bar after the last confirmed pivot. We feed
        // that same provisional into the Elliott state machine so wave
        // labels track the live swing rather than stopping at the last
        // flip. This is what keeps Elliott visually consistent with the
        // zigzag overlay (same final anchor bar either way).
        if let Some(prov) = provisional_pivot(&pivots, bars) {
            match prov.kind {
                PivotKind::High => process_high_pivot(&mut state, cfg, prov.bar_index, prov.price),
                PivotKind::Low => process_low_pivot(&mut state, cfg, prov.bar_index, prov.price),
            }
        }

        // Pine `barstate.islast` label fusion — "(2) / (c)" overlap.
        //   if getEW.b2.x == getEW1.bC.x
        //       gEW.b1.textcolor=na; gEW.b2.textcolor=na
        //       bB → "(b) (1)";  bC → "(c) (2)"
        // Runs once per level after the main bar loop.
        if state.ew.len() >= 2 {
            let b2x = state.ew[0].anchors[2].bar_index;
            let c_x_match = state.ew[1]
                .abc
                .as_ref()
                .map(|a| a.anchors[3].bar_index == b2x)
                .unwrap_or(false);
            if c_x_match {
                state.ew[0].anchors[1].hide_label = true;
                state.ew[0].anchors[2].hide_label = true;
                if let Some(abc) = state.ew[1].abc.as_mut() {
                    abc.anchors[2].label_override = Some("(b) (1)".into());
                    abc.anchors[3].label_override = Some("(c) (2)".into());
                }
            }
        }

        // Pine `barstate.islast` — fib band on the freshest wave.
        let fib_band = state.ew.first().and_then(|front| {
            if !front.live { return None; }
            // If an ABC's C is already beyond p5, Pine clears the band.
            if let Some(abc) = &front.abc {
                if abc.anchors[3].bar_index > front.anchors[5].bar_index {
                    return None;
                }
            }
            let last_0y = front.anchors[0].price;
            let last_6x = front.anchors[5].bar_index;
            let last_6y = front.anchors[5].price;
            let diff = (last_6y - last_0y).abs();
            let bull = front.direction == 1;
            let sign = if bull { -1.0 } else { 1.0 };
            let y_500 = last_6y + sign * diff * cfg.fib_level_1;
            let y_618 = last_6y + sign * diff * cfg.fib_level_2;
            let y_764 = last_6y + sign * diff * cfg.fib_level_3;
            let y_854 = last_6y + sign * diff * cfg.fib_level_4;
            // Broken? — price pierced the 85.4 bound between wave end
            // and the present bar.
            let mut broken = false;
            let from = last_6x.max(0) as usize;
            let to = bars.len() - 1;
            if from <= to {
                for i in from..=to {
                    if bull {
                        if bars[i].low < y_854 { broken = true; break; }
                    } else if bars[i].high > y_854 { broken = true; break; }
                }
            }
            Some(FibBand {
                x_anchor: last_6x,
                x_far: bar_count - 1 + 10,
                pole_top: last_6y.max(y_854),
                pole_bottom: last_6y.min(y_854),
                y_500, y_618, y_764, y_854,
                broken,
            })
        });

        levels.push(LevelOutput {
            length: level_cfg.length,
            color: level_cfg.color.clone(),
            pivots: state.pivots_log,
            motives: state.ew,
            break_markers: state.break_markers,
            fib_band,
        });
    }

    PinePortOutput { bar_count, levels }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(o: f64, h: f64, l: f64, c: f64) -> Bar {
        Bar { open: o, high: h, low: l, close: c }
    }

    #[test]
    fn run_empty() {
        let out = run(&[], &PinePortConfig::default());
        assert_eq!(out.bar_count, 0);
        assert!(out.levels.is_empty());
    }

    #[test]
    fn run_generates_some_pivots_on_sawtooth() {
        // Trivial sawtooth — must produce at least a few pivots at len=4.
        let mut bars = Vec::new();
        for i in 0..60 {
            let phase = (i % 10) as f64;
            let base = 100.0 + (i as f64);
            let wiggle = if phase < 5.0 { phase } else { 10.0 - phase };
            let h = base + wiggle;
            let l = base - wiggle;
            bars.push(bar(base, h, l, base));
        }
        let cfg = PinePortConfig {
            levels: vec![LevelConfig { length: 4, color: "#000".into() }],
            ..Default::default()
        };
        let out = run(&bars, &cfg);
        assert_eq!(out.levels.len(), 1);
        assert!(out.levels[0].pivots.len() >= 4, "expected pivots, got {:?}", out.levels[0].pivots);
    }
}

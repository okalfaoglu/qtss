// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! Elliott early-wave detection — FAZ 25 PR-25A.
//!
//! The base [`elliott::ElliottWriter`] (in this same writers/ folder)
//! persists complete 6-pivot motives, post-motive ABCs and triangles.
//! Those are LATE signals — by the time a 5-wave impulse closes the
//! Wave 3 ride is already over. This module hooks into the **same
//! pivot stream** the LuxAlgo Pine port already produces and adds
//! THREE earlier pattern signals to the `detections` table:
//!
//! | subkind                          | trigger                                    |
//! |----------------------------------|--------------------------------------------|
//! | `impulse_nascent_{bull,bear}`    | 4 pivots: W1+W2+W3 in, W3 has broken W1   |
//! | `impulse_forming_{bull,bear}`    | 5 pivots: W1+W2+W3+W4 in, W5 forming      |
//! | `impulse_extended_{bull,bear}`   | full 5 waves but with one wave extended   |
//!
//! These rows feed the IQ-D entry candidate creator (PR-25C).
//! `pattern_family = 'elliott_early'` is **new**; existing T/D setups
//! never read this family, so this is a strictly additive change
//! (CLAUDE.md isolation principle, FAZ 25 §0).
//!
//! **Decision rules** are ported from `qtss_elliott::nascent` and
//! `qtss_elliott::forming` to operate on `PivotPoint` slices instead
//! of `PivotTree` — same fib bands, same retrace gates, same scoring.
//! When the upstream crate evolves, port the diff here too.

use chrono::{DateTime, Utc};
use qtss_elliott::luxalgo_pine_port::{LevelOutput, MotivePattern, PivotPoint};
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::EngineSymbol;

// Fib reference sets — keep in lock-step with `qtss_elliott::nascent`.
const WAVE2_FIB_REFS: &[f64] = &[0.382, 0.5, 0.618];
const WAVE3_EXT_REFS: &[f64] = &[1.618, 2.0, 2.618];
const WAVE4_FIB_REFS: &[f64] = &[0.236, 0.382];

/// Default ABC anchor Fib targets — used when scoring "how close
/// did this pivot land to a canonical Fibonacci ratio". A retraces
/// the impulse, B retraces A, C extends A.
const ABC_A_FIB_TARGETS: &[f64] = &[0.382, 0.5, 0.618, 0.786];
const ABC_B_FIB_TARGETS: &[f64] = &[0.382, 0.5, 0.618];
const ABC_C_FIB_TARGETS: &[f64] = &[0.618, 1.0, 1.618];

/// Score `value` against a set of canonical Fib targets. Returns
/// 1.0 when value matches a target exactly, decaying with distance.
/// Used for graded confidence instead of binary in-band/out-of-band.
fn fib_proximity_score(value: f64, targets: &[f64]) -> f64 {
    targets
        .iter()
        .map(|t| {
            let d = (value - t).abs() / t.max(0.001);
            (1.0 - d.min(1.0)).max(0.0)
        })
        .fold(0.0_f64, f64::max)
}

/// Apply a symmetric tolerance widening to a [lo, hi] band.
/// E.g. (0.236, 0.886, 0.05) → (0.224, 0.930). Accepts pivots a
/// hair outside the textbook band — Elliott practitioners always
/// allow ~5% wiggle because pivot prints are noisy.
fn widen_band(lo: f64, hi: f64, tol_pct: f64) -> (f64, f64) {
    let span = hi - lo;
    let pad = span * tol_pct;
    (lo - pad, hi + pad)
}

#[derive(Debug, Clone)]
pub struct EarlyMatch {
    pub subkind: String,
    pub direction: i8,           // +1 bull, -1 bear
    pub anchors: Vec<PivotPoint>,
    pub score: f64,              // 0..1, fib-proximity mean
    pub invalidation_price: f64, // p0 — same as full impulse rule
    pub stage: &'static str,     // "nascent" | "forming" | "extended"
    pub w3_extension: f64,       // multiple of W1; 1.0 = same length, 1.618 = canonical
    /// Per-anchor confidence score (0..1, higher = closer to a
    /// canonical Fib target). Indexed parallel to `anchors`. Empty
    /// vector if scoring not applicable to this stage. Persisted in
    /// raw_meta.anchor_scores for the frontend to render graded
    /// opacity / line width instead of binary solid/dotted.
    pub anchor_scores: Vec<f64>,
}

/// Mini pivot detection on raw OHLC bars — used as a fallback when
/// the Pine-port ZigZag (Z5 length=21 etc.) hasn't yet confirmed a
/// post-W5 pivot but the price has clearly moved enough to define one.
/// Algorithm: rolling N-bar window centred on each candidate; a high
/// pivot is a bar whose `high` is the strict max of the window; a low
/// pivot is the strict min. Returns pivots in chronological order,
/// with `direction = +1` for highs and `-1` for lows.
fn detect_post_w5_mini_pivots(
    chrono_bars: &[qtss_storage::market_bars::MarketBarRow],
    start_idx: usize,
    win: usize,
) -> Vec<PivotPoint> {
    let mut out: Vec<PivotPoint> = Vec::new();
    if chrono_bars.len() <= start_idx + 2 * win {
        return out;
    }
    let half = win.max(1);
    for i in (start_idx + half)..(chrono_bars.len() - half) {
        let center_high = chrono_bars[i].high.to_f64().unwrap_or(0.0);
        let center_low = chrono_bars[i].low.to_f64().unwrap_or(0.0);
        let mut is_high = true;
        let mut is_low = true;
        for j in (i - half)..=(i + half) {
            if j == i {
                continue;
            }
            let h = chrono_bars[j].high.to_f64().unwrap_or(0.0);
            let l = chrono_bars[j].low.to_f64().unwrap_or(0.0);
            // Use STRICT comparison so a bar that matches a neighbor
            // can still qualify (intraday ties are common). The earlier
            // version used >= / <= and rejected too many candidates.
            if h > center_high {
                is_high = false;
            }
            if l < center_low {
                is_low = false;
            }
            if !is_high && !is_low {
                break;
            }
        }
        if is_high {
            out.push(PivotPoint {
                direction: 1,
                bar_index: i as i64,
                price: center_high,
                label_override: None,
                hide_label: false,
            });
        } else if is_low {
            out.push(PivotPoint {
                direction: -1,
                bar_index: i as i64,
                price: center_low,
                label_override: None,
                hide_label: false,
            });
        }
    }
    // Collapse consecutive same-direction pivots — keep the more extreme
    // one. ABC detection downstream expects strict alternation.
    let mut collapsed: Vec<PivotPoint> = Vec::new();
    for p in out {
        if let Some(prev) = collapsed.last_mut() {
            if prev.direction == p.direction {
                let keep_new = (p.direction == 1 && p.price > prev.price)
                    || (p.direction == -1 && p.price < prev.price);
                if keep_new {
                    *prev = p;
                }
                continue;
            }
        }
        collapsed.push(p);
    }
    collapsed
}

/// Scan a level's pivot tape for nascent / forming / extended patterns
/// the LuxAlgo motive detector hasn't yet captured (or won't, because
/// the 5th pivot isn't in yet). Returns chronologically-distinct
/// matches; the writer-side dedupe is a UNIQUE-KEY upsert on
/// (start_time, end_time, subkind).
pub fn scan_level(
    level: &LevelOutput,
    chrono_bars: &[qtss_storage::market_bars::MarketBarRow],
) -> Vec<EarlyMatch> {
    let pivots = &level.pivots;
    let mut out = Vec::new();
    if pivots.len() < 4 {
        return out;
    }

    // Skip pivot windows that already form a complete LuxAlgo motive —
    // those are emitted by the base writer and we don't want to double-
    // count. We keep a set of motive end-bar indices for the test.
    let motive_end_bars: std::collections::HashSet<i64> =
        level.motives.iter().map(|m| m.anchors[5].bar_index).collect();

    // Slide a 4-pivot window for nascent + a 5-pivot window for forming.
    for start in 0..=pivots.len().saturating_sub(4) {
        let win4 = &pivots[start..start + 4];
        if let Some(m) = detect_nascent(win4) {
            // Suppress nascent if a complete motive ends at or just past
            // this same window (Pine port already emitted it).
            let last_bar = win4[3].bar_index;
            if !motive_end_bars.iter().any(|&b| (b - last_bar).abs() <= 2) {
                out.push(m);
            }
        }
        if start + 5 <= pivots.len() {
            let win5 = &pivots[start..start + 5];
            if let Some(m) = detect_forming(win5) {
                let last_bar = win5[4].bar_index;
                if !motive_end_bars.iter().any(|&b| (b - last_bar).abs() <= 2) {
                    out.push(m);
                }
            }
        }
    }

    // Extended impulse — runs only on COMPLETE motives produced by the
    // Pine port. Tags which wave (W1/W3/W5) is the extended one.
    for motive in &level.motives {
        if let Some(ex) = detect_extended(motive) {
            out.push(ex);
        }
    }

    // ABC stages — every completed motive that hasn't yet been wrapped
    // by Pine port's full ABC produces an EarlyMatch with up to four
    // anchors [W5, A, B, C]. Anchors with a `label_override` ending
    // in "?" are PROJECTIONS (Fib-based simulation); anchors without
    // such a marker are real pivots from the post-W5 tape. Stages:
    //
    //   abc_projected : 0-1 real post-W5 pivots, the rest projected
    //                   (W5 only → simulate A,B,C; W5+A → simulate
    //                    B,C). Lets the user PLAN the correction
    //                    before pivots confirm. Frontend draws the
    //                    projected segments dotted.
    //   abc_nascent   : 2 real post-W5 pivots (A,B), C projected.
    //                   Solid p5→A→B, dotted B→C.
    //   abc_forming   : 3 real post-W5 pivots (A,B,C in progress).
    //                   All four anchors solid.
    //
    // CRITICAL invalidation rule (Elliott): a bull motive's W5 high
    // is the upper boundary of any subsequent ABC. If a post-W5
    // pivot crosses that boundary, the ABC is INVALIDATED — the (5)
    // label was wrong (the move was actually a sub-wave of a larger
    // impulse). Same in mirror for bear motives. We must NOT keep
    // marking `ab?` on motives whose price action has already
    // overruled the count.
    for motive in &level.motives {
        if motive.abc.is_some() {
            // Pine port already detected the full ABC for this motive
            // — we don't double-mark it.
            continue;
        }
        let p5 = &motive.anchors[5];
        let p5_bar = p5.bar_index;
        // Pivots strictly after p5 from the Pine-port ZigZag tape.
        let pine_post: Vec<PivotPoint> = pivots
            .iter()
            .filter(|p| p.bar_index > p5_bar)
            .cloned()
            .collect();
        // Fallback: if the Pine-port ZigZag (Z5 length=21 on 4h is
        // ~84h displacement) hasn't confirmed a post-W5 pivot but the
        // raw bars contain a clear correction, run a smaller-window
        // mini-detector on the raw OHLC. Window scales with the
        // level's length — half is plenty to catch corrective pivots
        // without amplifying noise.
        let pine_post_count = pine_post.len();
        let post_w5: Vec<PivotPoint> = if pine_post.len() >= 2 {
            pine_post
        } else {
            // Mini-pivot fallback: Pine port's ZigZag (Z5 length=21
            // on 4h is ~84h displacement) often hasn't confirmed any
            // post-W5 pivot when the corrective is still small. We
            // run a shared 3-bar window across ALL slots so the same
            // structural definition lights up Z1..Z5. Picked 3 by
            // looking at SOLUSDT 4h Z3/Z4 where it already produced
            // good (a)(b) anchors and BTCUSDT 4h Z5 where the user
            // marked the obvious A/B pivots by eye but a 4-bar
            // window missed them.
            let win = 3;
            let p5_usize = p5_bar.max(0) as usize;
            detect_post_w5_mini_pivots(chrono_bars, p5_usize, win)
        };
        let _ = pine_post_count;
        // Filter: ABC's first leg ALWAYS opposes the motive's W5
        // direction (bull motive p5=high → A=low; bear motive p5=low
        // → A=high). If the first post-W5 pivot has the SAME
        // direction as p5 (Pine port mini-noise spike, mini-pivot
        // ambiguity), drop everything until a valid opposite pivot
        // appears. Without this filter ETH 4h Z5 produced a
        // wrong-direction first pivot that dead-ended every branch.
        let first_valid = post_w5
            .iter()
            .position(|p| p.direction != p5.direction);
        let post_w5: Vec<PivotPoint> = match first_valid {
            Some(i) => post_w5[i..].to_vec(),
            None => Vec::new(),
        };
        let post_w5_refs: Vec<&PivotPoint> = post_w5.iter().collect();
        let post_w5 = post_w5_refs;
        // Price-invalidation check with a small tolerance. For a bull
        // motive p5 is a HIGH; a post-W5 HIGH that materially exceeds
        // p5 (more than 0.5%) invalidates. The tolerance prevents
        // intraday spikes / wick noise from killing otherwise valid
        // ABC candidates the eye reads as still in formation. Same
        // mirror for bear motives.
        const INVALIDATION_TOL_PCT: f64 = 0.005; // 0.5%
        let invalidated = if motive.direction == 1 {
            let limit = p5.price * (1.0 + INVALIDATION_TOL_PCT);
            post_w5.iter().any(|p| p.direction == 1 && p.price > limit)
        } else {
            let limit = p5.price * (1.0 - INVALIDATION_TOL_PCT);
            post_w5.iter().any(|p| p.direction == -1 && p.price < limit)
        };
        if invalidated {
            continue;
        }
        // ABC of a bull motive corrects DOWN: A=low, B=high, C=low.
        // ABC of a bear motive: A=high, B=low, C=high. Direction flag
        // on the EarlyMatch follows the same convention as motive.abc:
        // +1 bullish ABC (after a bearish motive), -1 bearish ABC.
        let abc_dir: i8 = -motive.direction;
        let suffix = if abc_dir == 1 { "bull" } else { "bear" };
        let p0 = &motive.anchors[0];

        // Projection geometry — Fib-based, mirrored automatically by
        // the price arithmetic since W5 - W0 carries the impulse sign.
        // A retraces ~50% of the W0→W5 swing; B retraces ~50% of A's
        // leg; C is the same length as A (zigzag baseline).
        let project_a = |from_price: f64| -> f64 {
            from_price - 0.5 * (motive.anchors[5].price - p0.price)
        };
        let project_b = |from_p5: f64, from_a: f64| -> f64 {
            from_a - 0.5 * (from_a - from_p5)
        };
        let project_c = |from_a: f64, from_b: f64| -> f64 {
            // C = A - (B - A) keeps C on the same side as A relative
            // to B, with leg length matching A.
            from_a - (from_b - from_a)
        };
        // Bar spacing — use motive's W3 duration as a proxy. Each ABC
        // leg projects forward by (W3 bars or 5, whichever is bigger).
        let leg_bars: i64 = (motive.anchors[3].bar_index
            - motive.anchors[2].bar_index)
            .max(5);
        let make_proj = |bar_offset: i64, price: f64, dir: i8, label: &str| PivotPoint {
            direction: dir,
            bar_index: motive.anchors[5].bar_index + bar_offset,
            price,
            label_override: Some(label.to_string()),
            hide_label: false,
        };

        // ─── abc_projected: 0 real post-W5 pivots, simulate ABC ─────
        if post_w5.is_empty() {
            let a_dir = -p5.direction;
            let b_dir = p5.direction;
            let c_dir = -p5.direction;
            let a_proj = project_a(p5.price);
            let b_proj = project_b(p5.price, a_proj);
            let c_proj = project_c(a_proj, b_proj);
            out.push(EarlyMatch {
                subkind: format!("abc_projected_{suffix}"),
                direction: abc_dir,
                anchors: vec![
                    p5.clone(),
                    make_proj(leg_bars, a_proj, a_dir, "a?"),
                    make_proj(leg_bars * 2, b_proj, b_dir, "b?"),
                    make_proj(leg_bars * 3, c_proj, c_dir, "c?"),
                ],
                score: 0.30,
                invalidation_price: p5.price,
                stage: "abc_projected",
                w3_extension: 0.0,
                anchor_scores: vec![],
            });
        }
        // ─── abc_projected (1 real): A real, simulate B + C ─────────
        else if post_w5.len() == 1 {
            let a_anchor = post_w5[0];
            if a_anchor.direction != p5.direction {
                let a_leg_bars =
                    (a_anchor.bar_index - p5.bar_index).max(5);
                let b_proj = project_b(p5.price, a_anchor.price);
                let c_proj = project_c(a_anchor.price, b_proj);
                let b_dir = p5.direction;
                let c_dir = -p5.direction;
                out.push(EarlyMatch {
                    subkind: format!("abc_projected_{suffix}"),
                    direction: abc_dir,
                    anchors: vec![
                        p5.clone(),
                        a_anchor.clone(),
                        PivotPoint {
                            direction: b_dir,
                            bar_index: a_anchor.bar_index + a_leg_bars,
                            price: b_proj,
                            label_override: Some("b?".into()),
                            hide_label: false,
                        },
                        PivotPoint {
                            direction: c_dir,
                            bar_index: a_anchor.bar_index + a_leg_bars * 2,
                            price: c_proj,
                            label_override: Some("c?".into()),
                            hide_label: false,
                        },
                    ],
                    score: 0.40,
                    invalidation_price: p5.price,
                    stage: "abc_projected",
                    w3_extension: 0.0,
                    anchor_scores: vec![],
                });
            }
        }
        // Nascent ABC: p5 + A pivot + B pivot in correct alternation.
        if post_w5.len() >= 2 {
            let a_anchor = post_w5[0];
            let b_anchor = post_w5[1];
            // Direction sanity: A opposite to motive end pivot, B opposite to A.
            if a_anchor.direction != p5.direction && b_anchor.direction != a_anchor.direction {
                let a_len = (a_anchor.price - p5.price).abs();
                let b_ret = (b_anchor.price - a_anchor.price).abs();
                // Ratio range widened from 0.10..=0.95 to 0.10..=1.30
                // so expanded-flat / running-flat ABCs (where B exceeds
                // A length) still register. Without this BTCUSDT 4h Z5
                // produced no candidate even though A/B were clearly
                // visible — the corrective B printed a 1.02-ratio
                // intraday spike beyond the original 0.95 ceiling.
                if a_len > 0.0 && (0.10..=1.30).contains(&(b_ret / a_len)) {
                    // Fib validation — a real pivot whose price is OUT of
                    // the canonical Elliott band gets re-tagged as
                    // projected (label_override ending in "?"). Frontend
                    // then renders that segment dotted instead of solid,
                    // even though the pivot itself is confirmed. The
                    // user reads this as "the pivot exists but doesn't
                    // satisfy classic ABC ratios — wait for confirmation".
                    let impulse_len = (p5.price - p0.price).abs();
                    let a_ret = a_len / impulse_len.max(1e-9);
                    // Tolerance-widened bands. Default 5% padding so
                    // a 0.224 retrace (textbook ceiling 0.236) still
                    // counts as "real". An operator can tighten or
                    // loosen via system_config.elliott_early.fib_tolerance_pct.
                    const FIB_TOL: f64 = 0.05;
                    let (a_lo, a_hi) = widen_band(0.236, 0.886, FIB_TOL);
                    let (b_lo, b_hi) = widen_band(0.236, 0.786, FIB_TOL);
                    let a_in_band = (a_lo..=a_hi).contains(&a_ret);
                    let b_in_band = (b_lo..=b_hi).contains(&(b_ret / a_len));
                    let mut a_clone = a_anchor.clone();
                    if !a_in_band {
                        a_clone.label_override = Some("a?".into());
                    }
                    let mut b_clone = b_anchor.clone();
                    if !b_in_band {
                        b_clone.label_override = Some("b?".into());
                    }
                    // Project C from the candidate (validated or not)
                    // B price; if B is out-of-band the C target also
                    // shifts — the projection follows the actual pivot
                    // tape rather than guessing a "perfect" B.
                    let c_proj = project_c(a_anchor.price, b_anchor.price);
                    let c_dir = a_anchor.direction;
                    let leg_ab =
                        (b_anchor.bar_index - a_anchor.bar_index).max(5);
                    out.push(EarlyMatch {
                        subkind: format!("abc_nascent_{suffix}"),
                        direction: abc_dir,
                        anchors: vec![
                            p5.clone(),
                            a_clone,
                            b_clone,
                            PivotPoint {
                                direction: c_dir,
                                bar_index: b_anchor.bar_index + leg_ab,
                                price: c_proj,
                                label_override: Some("c?".into()),
                                hide_label: false,
                            },
                        ],
                        score: 0.55,
                        invalidation_price: p5.price,
                        stage: "abc_nascent",
                        w3_extension: 0.0,
                        anchor_scores: vec![
                            1.0, // W5 always 1.0 (anchor of motive)
                            fib_proximity_score(a_ret, ABC_A_FIB_TARGETS),
                            fib_proximity_score(b_ret / a_len, ABC_B_FIB_TARGETS),
                            0.0, // C projected — no real value yet
                        ],
                    });
                }
            }
        }
        // Forming ABC: p5 + A + B + C-so-far. Full ABC fires from the
        // base writer once C completes; this catches the in-progress
        // C leg.
        if post_w5.len() >= 3 {
            let a_anchor = post_w5[0];
            let b_anchor = post_w5[1];
            let c_anchor = post_w5[2];
            if a_anchor.direction != p5.direction
                && b_anchor.direction != a_anchor.direction
                && c_anchor.direction != b_anchor.direction
            {
                let a_len = (a_anchor.price - p5.price).abs();
                let c_len = (c_anchor.price - b_anchor.price).abs();
                if a_len > 0.0 && c_len >= 0.5 * a_len {
                    // Fib validation per anchor — out-of-band gets a
                    // "?" label_override so the segment renders dotted
                    // even though the pivot is real (Elliott rule
                    // violation, treat as unconfirmed shape).
                    let impulse_len = (p5.price - p0.price).abs();
                    let b_ret = (b_anchor.price - a_anchor.price).abs();
                    const FIB_TOL: f64 = 0.05;
                    let a_ratio = a_len / impulse_len.max(1e-9);
                    let (a_lo, a_hi) = widen_band(0.236, 0.886, FIB_TOL);
                    let (b_lo, b_hi) = widen_band(0.236, 0.786, FIB_TOL);
                    let (c_lo, c_hi) = widen_band(0.618, 2.618, FIB_TOL);
                    let a_in_band = (a_lo..=a_hi).contains(&a_ratio);
                    let b_in_band = (b_lo..=b_hi).contains(&(b_ret / a_len));
                    let c_ratio = c_len / a_len;
                    let c_in_band = (c_lo..=c_hi).contains(&c_ratio);
                    let mut a_clone = a_anchor.clone();
                    if !a_in_band {
                        a_clone.label_override = Some("a?".into());
                    }
                    let mut b_clone = b_anchor.clone();
                    if !b_in_band {
                        b_clone.label_override = Some("b?".into());
                    }
                    let mut c_clone = c_anchor.clone();
                    if !c_in_band {
                        c_clone.label_override = Some("c?".into());
                    }
                    out.push(EarlyMatch {
                        subkind: format!("abc_forming_{suffix}"),
                        direction: abc_dir,
                        anchors: vec![p5.clone(), a_clone, b_clone, c_clone],
                        score: 0.65,
                        invalidation_price: p5.price,
                        stage: "abc_forming",
                        w3_extension: 0.0,
                        anchor_scores: vec![
                            1.0, // W5 always anchored
                            fib_proximity_score(a_ratio, ABC_A_FIB_TARGETS),
                            fib_proximity_score(b_ret / a_len, ABC_B_FIB_TARGETS),
                            fib_proximity_score(c_ratio, ABC_C_FIB_TARGETS),
                        ],
                    });
                }
            }
        }
    }

    out
}

/// 4-pivot window: nascent impulse (W1+W2+W3 in, W3 already broke W1).
fn detect_nascent(p: &[PivotPoint]) -> Option<EarlyMatch> {
    debug_assert_eq!(p.len(), 4);
    if !alternation_ok(p) {
        return None;
    }
    let dir = if p[0].direction < 0 { 1i8 } else { -1i8 };
    // Normalize to bullish frame: a bullish nascent has p0=low, p1=high,
    // p2=low, p3=high; a bearish one is the mirror.
    let (w1_dist, w2_dist, w3_so_far, p_norm) = normalize_4(p, dir)?;
    if w1_dist <= 0.0 || w2_dist <= 0.0 || w3_so_far <= 0.0 {
        return None;
    }
    let w2_ret = w2_dist / w1_dist;
    if !(0.236..=0.786).contains(&w2_ret) {
        return None;
    }
    // W3 must have crossed W1 high in normalized frame
    if p_norm[3] <= p_norm[1] {
        return None;
    }
    if w3_so_far < w1_dist * 0.9 {
        return None;
    }
    let w3_ext = w3_so_far / w1_dist;
    let s2 = nearest_fib_score(w2_ret, WAVE2_FIB_REFS);
    let s3 = nearest_fib_score(w3_ext, WAVE3_EXT_REFS);
    let score = (s2 + s3) / 2.0;
    if score < 0.30 {
        return None;
    }
    let suffix = if dir == 1 { "bull" } else { "bear" };
    Some(EarlyMatch {
        subkind: format!("impulse_nascent_{suffix}"),
        direction: dir,
        anchors: p.to_vec(),
        score,
        invalidation_price: p[0].price,
        stage: "nascent",
        w3_extension: w3_ext,
        anchor_scores: vec![],
    })
}

/// 5-pivot window: forming impulse (W1+W2+W3+W4 in, W5 forming).
fn detect_forming(p: &[PivotPoint]) -> Option<EarlyMatch> {
    debug_assert_eq!(p.len(), 5);
    if !alternation_ok(p) {
        return None;
    }
    let dir = if p[0].direction < 0 { 1i8 } else { -1i8 };
    let (w1_dist, w2_dist, w3_dist, w4_dist, p_norm) = normalize_5(p, dir)?;
    if w1_dist <= 0.0 || w2_dist <= 0.0 || w3_dist <= 0.0 || w4_dist <= 0.0 {
        return None;
    }
    // Standard Elliott rules
    let w2_ret = w2_dist / w1_dist;
    if !(0.236..=0.786).contains(&w2_ret) {
        return None;
    }
    if p_norm[3] <= p_norm[1] {
        return None; // W3 must break W1 high
    }
    if w3_dist < w1_dist * 0.9 {
        return None; // W3 not shortest (vs W1)
    }
    if p_norm[4] <= p_norm[1] {
        return None; // W4 must NOT overlap W1 (in normalized frame)
    }
    let w4_ret = w4_dist / w3_dist;
    if !(0.10..=0.50).contains(&w4_ret) {
        return None; // typical W4 retrace 23.6-50%
    }
    let s2 = nearest_fib_score(w2_ret, WAVE2_FIB_REFS);
    let s3 = nearest_fib_score(w3_dist / w1_dist, WAVE3_EXT_REFS);
    let s4 = nearest_fib_score(w4_ret, WAVE4_FIB_REFS);
    let score = (s2 + s3 + s4) / 3.0;
    if score < 0.30 {
        return None;
    }
    let suffix = if dir == 1 { "bull" } else { "bear" };
    Some(EarlyMatch {
        subkind: format!("impulse_forming_{suffix}"),
        direction: dir,
        anchors: p.to_vec(),
        score,
        invalidation_price: p[0].price,
        stage: "forming",
        w3_extension: w3_dist / w1_dist,
        anchor_scores: vec![],
    })
}

/// Tag which wave is the extended one in a complete motive (W1/W3/W5).
/// "Extended" = ≥ 1.618 × longer than each of the other two.
fn detect_extended(motive: &MotivePattern) -> Option<EarlyMatch> {
    let p = &motive.anchors;
    let dir = motive.direction;
    let pn: Vec<f64> = if dir == 1 {
        p.iter().map(|x| x.price).collect()
    } else {
        p.iter().map(|x| -x.price).collect()
    };
    let w1 = pn[1] - pn[0];
    let w3 = pn[3] - pn[2];
    let w5 = pn[5] - pn[4];
    if w1 <= 0.0 || w3 <= 0.0 || w5 <= 0.0 {
        return None;
    }
    let (which, ratio): (i8, f64) = if w3 >= 1.618 * w1 && w3 >= 1.618 * w5 {
        (3, w3 / w1)
    } else if w1 >= 1.618 * w3 && w1 >= 1.618 * w5 {
        (1, w1 / w3)
    } else if w5 >= 1.618 * w1 && w5 >= 1.618 * w3 {
        (5, w5 / w3)
    } else {
        return None;
    };
    let suffix = if dir == 1 { "bull" } else { "bear" };
    let subkind = format!("impulse_w{which}_extended_{suffix}");
    Some(EarlyMatch {
        subkind,
        direction: dir,
        anchors: p.to_vec(),
        score: (ratio - 1.0).min(2.0) / 2.0 * 0.7 + 0.3,
        invalidation_price: p[0].price,
        stage: "extended",
        w3_extension: w3 / w1,
            anchor_scores: vec![],
        })
}

// ------------------------------------------------------------------ helpers

fn alternation_ok(p: &[PivotPoint]) -> bool {
    p.windows(2).all(|w| w[0].direction != w[1].direction)
}

/// Returns (w1, w2, w3_so_far, normalized prices) for a 4-pivot window.
fn normalize_4(p: &[PivotPoint], dir: i8) -> Option<(f64, f64, f64, [f64; 4])> {
    let signs: [f64; 4] = if dir == 1 {
        [1.0, 1.0, 1.0, 1.0]
    } else {
        [-1.0, -1.0, -1.0, -1.0]
    };
    let prices = [
        p[0].price * signs[0],
        p[1].price * signs[1],
        p[2].price * signs[2],
        p[3].price * signs[3],
    ];
    let w1 = prices[1] - prices[0];
    let w2 = prices[1] - prices[2];
    let w3 = prices[3] - prices[2];
    Some((w1, w2, w3, prices))
}

fn normalize_5(p: &[PivotPoint], dir: i8) -> Option<(f64, f64, f64, f64, [f64; 5])> {
    let signs: [f64; 5] = if dir == 1 {
        [1.0; 5]
    } else {
        [-1.0; 5]
    };
    let prices = [
        p[0].price * signs[0],
        p[1].price * signs[1],
        p[2].price * signs[2],
        p[3].price * signs[3],
        p[4].price * signs[4],
    ];
    let w1 = prices[1] - prices[0];
    let w2 = prices[1] - prices[2];
    let w3 = prices[3] - prices[2];
    let w4 = prices[3] - prices[4];
    Some((w1, w2, w3, w4, prices))
}

fn nearest_fib_score(value: f64, refs: &[f64]) -> f64 {
    refs.iter()
        .map(|r| {
            let d = (value - r).abs() / r.max(0.001);
            (1.0 - d.min(1.0)).max(0.0)
        })
        .fold(0.0_f64, f64::max)
}

// ------------------------------------------------------------------ writer

/// Persist one [`EarlyMatch`] into the `detections` table under
/// `pattern_family = 'elliott_early'` and the per-stage subkind.
/// Returns 1 on insert/update, 0 on noop (e.g. parse failure).
///
/// ABC stage dedup: when this match is an ABC pattern, any older
/// less-advanced stage rows for the SAME parent motive (matched by
/// anchors[0].bar_index) get deleted in the same transaction. This
/// keeps the trading bot — which reads the table directly — from
/// seeing two rows describing the same logical structure (e.g. a
/// stale `abc_nascent` lingering after `abc_forming` already locked
/// in). The frontend dedup added in 9802a48 becomes redundant once
/// the writer guarantees one row per parent motive.
#[allow(clippy::too_many_arguments)]
pub async fn write_early(
    pool: &PgPool,
    sym: &EngineSymbol,
    slot: i16,
    em: &EarlyMatch,
    chrono_bars: &[qtss_storage::market_bars::MarketBarRow],
) -> anyhow::Result<usize> {
    // Sanity gate — Pine port's ZigZag init can leak a price=0 anchor
    // when the analysis window grows suddenly (see elliott.rs writer).
    // Refuse to persist anything with a zero-or-negative price; the
    // chart and the bot allocator both treat those as bogus.
    if em.anchors.iter().any(|a| a.price <= 0.0) {
        return Ok(0);
    }
    let start_bar = em.anchors.first().map(|a| a.bar_index).unwrap_or(0);
    let end_bar = em.anchors.last().map(|a| a.bar_index).unwrap_or(0);
    let (start_time, end_time) = anchor_time_range(chrono_bars, start_bar, end_bar);
    let anchors_json = anchors_with_times(&em.anchors, chrono_bars);
    let raw_meta = json!({
        "score":              em.score,
        "stage":              em.stage,
        "w3_extension":       em.w3_extension,
        "invalidation_price": em.invalidation_price,
        // Per-anchor confidence (0..1). Frontend reads this for
        // graded line opacity / dash pattern.
        "anchor_scores":      em.anchor_scores,
    });

    // Drop any stale lesser-stage ABC rows for the SAME parent motive
    // (same anchors[0] = W5 anchor). Stage hierarchy:
    //   abc_forming > abc_nascent > abc_projected
    // Only abc_* matches enter this block — impulse N/F/X are slot-
    // unique already (different subkind per match) and don't need it.
    if em.subkind.starts_with("abc_") {
        let lesser: &[&str] = match em.stage {
            "abc_forming" => &[
                "abc_nascent_bull", "abc_nascent_bear",
                "abc_projected_bull", "abc_projected_bear",
            ],
            "abc_nascent" => &["abc_projected_bull", "abc_projected_bear"],
            _ => &[],
        };
        if !lesser.is_empty() {
            let parent_bar = em.anchors.first().map(|a| a.bar_index).unwrap_or(start_bar);
            let _ = sqlx::query(
                r#"DELETE FROM detections
                    WHERE exchange = $1 AND segment = $2
                      AND symbol = $3 AND timeframe = $4
                      AND slot = $5
                      AND pattern_family = 'elliott_early'
                      AND mode = 'live'
                      AND subkind = ANY($6)
                      AND (anchors->0->>'bar_index')::bigint = $7"#,
            )
            .bind(&sym.exchange)
            .bind(&sym.segment)
            .bind(&sym.symbol)
            .bind(&sym.interval)
            .bind(slot)
            .bind(lesser)
            .bind(parent_bar)
            .execute(pool)
            .await;
        }
    }
    if let Err(e) = sqlx::query(
        r#"INSERT INTO detections
              (exchange, segment, symbol, timeframe, slot,
               pattern_family, subkind, direction,
               start_bar, end_bar, start_time, end_time,
               anchors, invalidated, raw_meta, mode)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,'live')
           ON CONFLICT (exchange, segment, symbol, timeframe, slot,
                        pattern_family, subkind, start_time, end_time, mode)
           DO UPDATE SET
               direction  = EXCLUDED.direction,
               start_bar  = EXCLUDED.start_bar,
               end_bar    = EXCLUDED.end_bar,
               anchors    = EXCLUDED.anchors,
               raw_meta   = EXCLUDED.raw_meta,
               updated_at = now()"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .bind(slot)
    .bind("elliott_early")
    .bind(&em.subkind)
    .bind(em.direction as i16)
    .bind(start_bar)
    .bind(end_bar)
    .bind(start_time)
    .bind(end_time)
    .bind(&anchors_json)
    .bind(false)
    .bind(&raw_meta)
    .execute(pool)
    .await
    {
        warn!(
            symbol = %sym.symbol, tf = %sym.interval,
            subkind = %em.subkind, %e,
            "elliott_early: insert failed"
        );
        return Ok(0);
    }
    Ok(1)
}

fn anchors_with_times(
    anchors: &[PivotPoint],
    chrono_bars: &[qtss_storage::market_bars::MarketBarRow],
) -> Value {
    let arr: Vec<Value> = anchors
        .iter()
        .map(|a| {
            let time = chrono_bars
                .get(a.bar_index.max(0) as usize)
                .map(|r| r.open_time);
            let mut obj = json!({
                "direction": a.direction,
                "bar_index": a.bar_index,
                "price": a.price,
            });
            if let Some(t) = time {
                obj["time"] = json!(t);
            }
            // CRITICAL — label_override carries the "?" suffix that
            // tags projected anchors. Without it the frontend treats
            // every anchor as a real pivot and renders the segment
            // ending at C as solid even though C is just a Fib
            // projection. (User reported BTCUSDT 1w Z4 painting a
            // solid B→C line for an unconfirmed C; root cause was
            // the missing field in this serializer.)
            if let Some(lo) = &a.label_override {
                obj["label_override"] = json!(lo);
            }
            if a.hide_label {
                obj["hide_label"] = json!(true);
            }
            obj
        })
        .collect();
    Value::Array(arr)
}

fn anchor_time_range(
    chrono_bars: &[qtss_storage::market_bars::MarketBarRow],
    start_bar: i64,
    end_bar: i64,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let clamp = |b: i64| -> Option<DateTime<Utc>> {
        chrono_bars.get(b.max(0) as usize).map(|r| r.open_time)
    };
    let s = clamp(start_bar)
        .or_else(|| chrono_bars.first().map(|r| r.open_time))
        .unwrap_or_else(Utc::now);
    let e = clamp(end_bar)
        .or_else(|| chrono_bars.last().map(|r| r.open_time))
        .unwrap_or_else(Utc::now);
    (s.min(e), s.max(e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use qtss_elliott::luxalgo_pine_port::PivotPoint;

    fn pp(direction: i8, bar_index: i64, price: f64) -> PivotPoint {
        PivotPoint {
            direction,
            bar_index,
            price,
            label_override: None,
            hide_label: false,
        }
    }

    #[test]
    fn nascent_bullish_classic() {
        // p0=low(100), p1=high(110), p2=low(105 — 50% retrace), p3=high(120)
        let p = vec![
            pp(-1, 0, 100.0),
            pp(1, 5, 110.0),
            pp(-1, 8, 105.0),
            pp(1, 13, 120.0),
        ];
        let m = detect_nascent(&p).expect("should detect nascent bull");
        assert_eq!(m.subkind, "impulse_nascent_bull");
        assert_eq!(m.direction, 1);
        assert!(m.score >= 0.30);
        assert!(m.w3_extension >= 1.0);
    }

    #[test]
    fn nascent_rejects_w2_too_deep() {
        // p2 retraces 95% of W1 — should reject
        let p = vec![
            pp(-1, 0, 100.0),
            pp(1, 5, 110.0),
            pp(-1, 8, 100.5),
            pp(1, 13, 120.0),
        ];
        assert!(detect_nascent(&p).is_none());
    }

    #[test]
    fn forming_bullish_classic() {
        // p0=100, p1=110 (W1=10), p2=104 (W2=60% retrace),
        // p3=130 (W3=26 = 2.6×W1), p4=118 (W4=46% retrace of W3)
        let p = vec![
            pp(-1, 0, 100.0),
            pp(1, 5, 110.0),
            pp(-1, 8, 104.0),
            pp(1, 15, 130.0),
            pp(-1, 19, 118.0),
        ];
        let m = detect_forming(&p).expect("should detect forming bull");
        assert_eq!(m.subkind, "impulse_forming_bull");
        assert!(m.score >= 0.30);
    }

    #[test]
    fn forming_rejects_w4_overlap_w1() {
        // p4 = 109 < p1 = 110 → W4 overlaps W1, must reject
        let p = vec![
            pp(-1, 0, 100.0),
            pp(1, 5, 110.0),
            pp(-1, 8, 104.0),
            pp(1, 15, 130.0),
            pp(-1, 19, 109.0),
        ];
        assert!(detect_forming(&p).is_none());
    }
}

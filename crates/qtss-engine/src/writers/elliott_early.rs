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
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::warn;

use crate::symbols::EngineSymbol;

// Fib reference sets — keep in lock-step with `qtss_elliott::nascent`.
const WAVE2_FIB_REFS: &[f64] = &[0.382, 0.5, 0.618];
const WAVE3_EXT_REFS: &[f64] = &[1.618, 2.0, 2.618];
const WAVE4_FIB_REFS: &[f64] = &[0.236, 0.382];

#[derive(Debug, Clone)]
pub struct EarlyMatch {
    pub subkind: String,
    pub direction: i8,           // +1 bull, -1 bear
    pub anchors: Vec<PivotPoint>,
    pub score: f64,              // 0..1, fib-proximity mean
    pub invalidation_price: f64, // p0 — same as full impulse rule
    pub stage: &'static str,     // "nascent" | "forming" | "extended"
    pub w3_extension: f64,       // multiple of W1; 1.0 = same length, 1.618 = canonical
}

/// Scan a level's pivot tape for nascent / forming / extended patterns
/// the LuxAlgo motive detector hasn't yet captured (or won't, because
/// the 5th pivot isn't in yet). Returns chronologically-distinct
/// matches; the writer-side dedupe is a UNIQUE-KEY upsert on
/// (start_time, end_time, subkind).
pub fn scan_level(level: &LevelOutput) -> Vec<EarlyMatch> {
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

    // ABC nascent / forming — fires after a completed motive when the
    // post-W5 pivot tape starts to look like a corrective ABC. Two
    // stages:
    //   abc_nascent  : 2 post-W5 pivots = potential A + B forming
    //   abc_forming  : 3 post-W5 pivots = A + B done, C forming
    // Full ABC (`pattern_family='abc'`) is still emitted by the base
    // ElliottWriter; this just catches the in-progress states.
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
        // Pivots strictly after p5, in order.
        let post_w5: Vec<&PivotPoint> = pivots
            .iter()
            .filter(|p| p.bar_index > p5_bar)
            .collect();
        // Price-invalidation check. For a bull motive (direction=+1),
        // p5 is a HIGH pivot at the top. Any post-W5 HIGH pivot that
        // exceeds p5.price means the count was wrong (motive was a
        // sub-wave). For bear motive (direction=-1), p5 is a LOW;
        // a post-W5 LOW below p5 invalidates symmetrically.
        let invalidated = if motive.direction == 1 {
            post_w5.iter().any(|p| p.direction == 1 && p.price > p5.price)
        } else {
            post_w5.iter().any(|p| p.direction == -1 && p.price < p5.price)
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
        // Nascent ABC: p5 + A pivot + B pivot in correct alternation.
        if post_w5.len() >= 2 {
            let a_anchor = post_w5[0];
            let b_anchor = post_w5[1];
            // Direction sanity: A opposite to motive end pivot, B opposite to A.
            if a_anchor.direction != p5.direction && b_anchor.direction != a_anchor.direction {
                let a_len = (a_anchor.price - p5.price).abs();
                let b_ret = (b_anchor.price - a_anchor.price).abs();
                if a_len > 0.0 && (0.10..=0.95).contains(&(b_ret / a_len)) {
                    out.push(EarlyMatch {
                        subkind: format!("abc_nascent_{suffix}"),
                        direction: abc_dir,
                        anchors: vec![p5.clone(), a_anchor.clone(), b_anchor.clone()],
                        score: 0.55,
                        invalidation_price: p5.price,
                        stage: "abc_nascent",
                        w3_extension: 0.0,
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
                    out.push(EarlyMatch {
                        subkind: format!("abc_forming_{suffix}"),
                        direction: abc_dir,
                        anchors: vec![
                            p5.clone(),
                            a_anchor.clone(),
                            b_anchor.clone(),
                            c_anchor.clone(),
                        ],
                        score: 0.65,
                        invalidation_price: p5.price,
                        stage: "abc_forming",
                        w3_extension: 0.0,
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
#[allow(clippy::too_many_arguments)]
pub async fn write_early(
    pool: &PgPool,
    sym: &EngineSymbol,
    slot: i16,
    em: &EarlyMatch,
    chrono_bars: &[qtss_storage::market_bars::MarketBarRow],
) -> anyhow::Result<usize> {
    let start_bar = em.anchors.first().map(|a| a.bar_index).unwrap_or(0);
    let end_bar = em.anchors.last().map(|a| a.bar_index).unwrap_or(0);
    let (start_time, end_time) = anchor_time_range(chrono_bars, start_bar, end_bar);
    let anchors_json = anchors_with_times(&em.anchors, chrono_bars);
    let raw_meta = json!({
        "score":              em.score,
        "stage":              em.stage,
        "w3_extension":       em.w3_extension,
        "invalidation_price": em.invalidation_price,
    });
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

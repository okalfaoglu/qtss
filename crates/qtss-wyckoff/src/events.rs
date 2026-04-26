//! Wyckoff event evaluators. Every spec shares the same
//! `(bars, config) → Vec<event>` signature so the detector loop is a
//! single iteration over [`WYCKOFF_SPECS`] with no per-event branch
//! (CLAUDE.md #1).
//!
//! First-pass heuristics (tightened later as live hit-rate data
//! accumulates):
//!   * SC / BC: volume >= climax_volume_mult × SMA AND range >=
//!     climax_range_atr_mult × ATR, with directional bias.
//!   * Spring / UTAD: wick past recent range low/high then close
//!     reclaim within reclaim window.
//!   * SOS / SOW: wide bullish/bearish bar with volume amplifier.
//!   * AR: bounce after SC within ~8 bars.
//!   * ST: low-volume retest of SC level.
//!   * PS / LPS / BU: softer heuristics kept as stubs for now; enable
//!     when Wyckoff outcomes table has enough labelled data.

use crate::config::WyckoffConfig;
use crate::event::{WyckoffEvent, WyckoffEventKind, WyckoffSpec};
use qtss_domain::v2::bar::Bar;
use rust_decimal::prelude::ToPrimitive;

fn bar_high(b: &Bar) -> f64 {
    b.high.to_f64().unwrap_or(0.0)
}
fn bar_low(b: &Bar) -> f64 {
    b.low.to_f64().unwrap_or(0.0)
}
fn bar_close(b: &Bar) -> f64 {
    b.close.to_f64().unwrap_or(0.0)
}
fn bar_range(b: &Bar) -> f64 {
    bar_high(b) - bar_low(b)
}
fn bar_vol(b: &Bar) -> f64 {
    b.volume.to_f64().unwrap_or(0.0)
}

fn sma_volume(bars: &[Bar], end: usize, window: usize) -> f64 {
    if end == 0 || window == 0 {
        return 0.0;
    }
    let start = end.saturating_sub(window);
    let n = end - start;
    if n == 0 {
        return 0.0;
    }
    bars[start..end].iter().map(bar_vol).sum::<f64>() / n as f64
}

fn atr(bars: &[Bar], end: usize, window: usize) -> f64 {
    if end < 2 || window == 0 {
        return 0.0;
    }
    let start = end.saturating_sub(window);
    let mut sum = 0.0;
    let mut n = 0;
    for i in start.max(1)..end {
        let tr = (bar_high(&bars[i]) - bar_low(&bars[i])).max(
            (bar_high(&bars[i]) - bar_close(&bars[i - 1]))
                .abs()
                .max((bar_low(&bars[i]) - bar_close(&bars[i - 1])).abs()),
        );
        sum += tr;
        n += 1;
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

fn recent_range(
    bars: &[Bar],
    end: usize,
    lookback: usize,
) -> Option<(f64, f64, usize)> {
    if end < 2 || lookback == 0 || end <= 2 {
        return None;
    }
    let start = end.saturating_sub(lookback);
    let mut hi = f64::NEG_INFINITY;
    let mut lo = f64::INFINITY;
    let mut count = 0;
    for b in &bars[start..end] {
        hi = hi.max(bar_high(b));
        lo = lo.min(bar_low(b));
        count += 1;
    }
    if count < 2 {
        return None;
    }
    Some((hi, lo, count))
}

// ── SC — Selling Climax ─────────────────────────────────────────────

pub fn eval_selling_climax(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < (cfg.volume_sma_bars as usize) + 2 {
        return out;
    }
    let start = n.saturating_sub(cfg.scan_lookback);
    for i in start.max(cfg.volume_sma_bars as usize)..n {
        let bar = &bars[i];
        let vol = bar_vol(bar);
        let avg = sma_volume(bars, i, cfg.volume_sma_bars as usize);
        if avg < 1e-9 {
            continue;
        }
        let vr = vol / avg;
        let a = atr(bars, i, 14).max(1e-9);
        let rr = bar_range(bar) / a;
        if vr < cfg.climax_volume_mult || rr < cfg.climax_range_atr_mult {
            continue;
        }
        let body = bar_close(bar) - bar.open.to_f64().unwrap_or(0.0);
        // SC = wide-range, high-volume bar with a long LOWER wick
        // (panic flush + rejection). Body sign relaxed: classic SC
        // often has a NEUTRAL or even slightly positive body
        // (capitulation bottoms reverse intra-bar). Body must just
        // not be a strong continuation up — that would be a SOS
        // not a SC. Threshold: body ≤ 20% of range allowed.
        let lower_wick = bar.open.to_f64().unwrap_or(0.0).min(bar_close(bar)) - bar_low(bar);
        let range = bar_range(bar);
        let weak_body = body <= range * 0.2;
        if weak_body && lower_wick > range * 0.4 {
            out.push(WyckoffEvent {
                kind: WyckoffEventKind::Sc,
                variant: "bull",
                score: (vr / cfg.climax_volume_mult).min(2.0) / 2.0 * 0.6 + 0.4,
                bar_index: i,
                reference_price: bar_low(bar),
                volume_ratio: vr,
                range_ratio: rr,
                note: format!(
                    "Selling Climax: vol {vr:.1}× SMA, range {rr:.1}× ATR"
                ),
            });
        }
    }
    out
}

// ── BC — Buying Climax (mirror of SC) ──────────────────────────────

pub fn eval_buying_climax(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < (cfg.volume_sma_bars as usize) + 2 {
        return out;
    }
    let start = n.saturating_sub(cfg.scan_lookback);
    for i in start.max(cfg.volume_sma_bars as usize)..n {
        let bar = &bars[i];
        let vol = bar_vol(bar);
        let avg = sma_volume(bars, i, cfg.volume_sma_bars as usize);
        if avg < 1e-9 {
            continue;
        }
        let vr = vol / avg;
        let a = atr(bars, i, 14).max(1e-9);
        let rr = bar_range(bar) / a;
        if vr < cfg.climax_volume_mult || rr < cfg.climax_range_atr_mult {
            continue;
        }
        let body = bar_close(bar) - bar.open.to_f64().unwrap_or(0.0);
        let upper_wick = bar_high(bar) - bar.open.to_f64().unwrap_or(0.0).max(bar_close(bar));
        let range = bar_range(bar);
        // BC mirror of SC: weak (any near-zero or slightly negative)
        // body OK as long as the upper wick dominates — classic
        // blowoff top often has a small body with a long upper wick
        // showing rejection of the high.
        let weak_body = body >= range * -0.2;
        if weak_body && upper_wick > range * 0.4 {
            out.push(WyckoffEvent {
                kind: WyckoffEventKind::Bc,
                variant: "bear",
                score: (vr / cfg.climax_volume_mult).min(2.0) / 2.0 * 0.6 + 0.4,
                bar_index: i,
                reference_price: bar_high(bar),
                volume_ratio: vr,
                range_ratio: rr,
                note: format!(
                    "Buying Climax: vol {vr:.1}× SMA, range {rr:.1}× ATR"
                ),
            });
        }
    }
    out
}

// ── Spring — wick below range + close reclaim ─────────────────────

pub fn eval_spring(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < 20 {
        return out;
    }
    let start = n.saturating_sub(cfg.scan_lookback);
    for i in start.max(20)..n {
        let bar = &bars[i];
        let (hi, lo, _) = match recent_range(bars, i, 20) {
            Some(x) => x,
            None => continue,
        };
        let range_height = hi - lo;
        if range_height < 1e-9 {
            continue;
        }
        let wick_below = lo - bar_low(bar);
        if wick_below <= 0.0 {
            continue;
        }
        if wick_below > range_height * cfg.spring_wick_max_pct {
            continue;
        }
        // Reclaim — close within reclaim window above range low.
        let end_r = (i + cfg.spring_reclaim_bars as usize).min(n - 1);
        let mut reclaimed = false;
        for k in i..=end_r {
            if bar_close(&bars[k]) > lo {
                reclaimed = true;
                break;
            }
        }
        if reclaimed {
            out.push(WyckoffEvent {
                kind: WyckoffEventKind::Spring,
                variant: "bull",
                score: 0.7,
                bar_index: i,
                reference_price: bar_low(bar),
                volume_ratio: 0.0,
                range_ratio: 0.0,
                note: format!(
                    "Spring: wick {:.2} below range low {:.2}, reclaimed",
                    bar_low(bar),
                    lo
                ),
            });
        }
    }
    out
}

// ── UTAD — mirror of spring (above range) ─────────────────────────

pub fn eval_utad(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < 20 {
        return out;
    }
    let start = n.saturating_sub(cfg.scan_lookback);
    for i in start.max(20)..n {
        let bar = &bars[i];
        let (hi, lo, _) = match recent_range(bars, i, 20) {
            Some(x) => x,
            None => continue,
        };
        let range_height = hi - lo;
        if range_height < 1e-9 {
            continue;
        }
        let wick_above = bar_high(bar) - hi;
        if wick_above <= 0.0 {
            continue;
        }
        if wick_above > range_height * cfg.spring_wick_max_pct {
            continue;
        }
        let end_r = (i + cfg.spring_reclaim_bars as usize).min(n - 1);
        let mut rejected = false;
        for k in i..=end_r {
            if bar_close(&bars[k]) < hi {
                rejected = true;
                break;
            }
        }
        if rejected {
            out.push(WyckoffEvent {
                kind: WyckoffEventKind::Utad,
                variant: "bear",
                score: 0.7,
                bar_index: i,
                reference_price: bar_high(bar),
                volume_ratio: 0.0,
                range_ratio: 0.0,
                note: format!(
                    "UTAD: wick {:.2} above range high {:.2}, rejected",
                    bar_high(bar),
                    hi
                ),
            });
        }
    }
    out
}

// ── SOS — Sign of Strength (wide bull bar + volume amplifier) ───

pub fn eval_sos(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < (cfg.volume_sma_bars as usize) + 2 {
        return out;
    }
    let start = n.saturating_sub(cfg.scan_lookback);
    for i in start.max(cfg.volume_sma_bars as usize)..n {
        let bar = &bars[i];
        let vol = bar_vol(bar);
        let avg = sma_volume(bars, i, cfg.volume_sma_bars as usize);
        if avg < 1e-9 {
            continue;
        }
        let a = atr(bars, i, 14).max(1e-9);
        let rr = bar_range(bar) / a;
        if vol < avg * cfg.sos_amplifier || rr < cfg.sos_amplifier {
            continue;
        }
        let body = bar_close(bar) - bar.open.to_f64().unwrap_or(0.0);
        if body > 0.0 && body > bar_range(bar) * 0.6 {
            out.push(WyckoffEvent {
                kind: WyckoffEventKind::Sos,
                variant: "bull",
                score: 0.65,
                bar_index: i,
                reference_price: bar_close(bar),
                volume_ratio: vol / avg,
                range_ratio: rr,
                note: format!("SOS: wide bull bar, vol {:.1}× SMA", vol / avg),
            });
        }
    }
    out
}

// ── SOW — Sign of Weakness (wide bear bar + volume amplifier) ───

pub fn eval_sow(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < (cfg.volume_sma_bars as usize) + 2 {
        return out;
    }
    let start = n.saturating_sub(cfg.scan_lookback);
    for i in start.max(cfg.volume_sma_bars as usize)..n {
        let bar = &bars[i];
        let vol = bar_vol(bar);
        let avg = sma_volume(bars, i, cfg.volume_sma_bars as usize);
        if avg < 1e-9 {
            continue;
        }
        let a = atr(bars, i, 14).max(1e-9);
        let rr = bar_range(bar) / a;
        if vol < avg * cfg.sos_amplifier || rr < cfg.sos_amplifier {
            continue;
        }
        let body = bar_close(bar) - bar.open.to_f64().unwrap_or(0.0);
        if body < 0.0 && body.abs() > bar_range(bar) * 0.6 {
            out.push(WyckoffEvent {
                kind: WyckoffEventKind::Sow,
                variant: "bear",
                score: 0.65,
                bar_index: i,
                reference_price: bar_close(bar),
                volume_ratio: vol / avg,
                range_ratio: rr,
                note: format!("SOW: wide bear bar, vol {:.1}× SMA", vol / avg),
            });
        }
    }
    out
}

// ── AR — Automatic Rally (post-SC bounce defining range top) ──────
//
// Heuristic: scan recent SCs; for each, find the highest CLOSE within
// `ar_window` bars after the SC bar. That high becomes AR. We don't
// repeat-fire — only the latest AR per range.
pub fn eval_ar(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    let window = cfg.ar_window_bars.max(3) as usize;
    if n < (cfg.volume_sma_bars as usize) + window + 2 {
        return out;
    }
    let scs = eval_selling_climax(bars, cfg);
    for sc in scs {
        let sc_idx = sc.bar_index;
        let end = (sc_idx + window).min(n - 1);
        if end <= sc_idx {
            continue;
        }
        let mut best_idx = sc_idx + 1;
        let mut best_high = bar_high(&bars[sc_idx + 1]);
        for k in (sc_idx + 1)..=end {
            let h = bar_high(&bars[k]);
            if h > best_high {
                best_high = h;
                best_idx = k;
            }
        }
        // AR must rise meaningfully above SC low (≥ 1× ATR).
        let a = atr(bars, sc_idx, 14).max(1e-9);
        if best_high - sc.reference_price < a {
            continue;
        }
        out.push(WyckoffEvent {
            kind: WyckoffEventKind::Ar,
            variant: "bull",
            score: 0.55,
            bar_index: best_idx,
            reference_price: best_high,
            volume_ratio: 0.0,
            range_ratio: 0.0,
            note: format!(
                "AR: {} bars after SC, top {:.2}",
                best_idx - sc_idx,
                best_high
            ),
        });
    }
    out
}

// ── ST — Secondary Test (low-volume retest of SC) ─────────────────
//
// After AR defines range top, a healthy accumulation revisits the SC
// area on REDUCED volume. Heuristic: bar whose low is within
// `st_proximity_pct` of an SC low AND volume is below baseline SMA.
pub fn eval_st(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < (cfg.volume_sma_bars as usize) + 5 {
        return out;
    }
    let scs = eval_selling_climax(bars, cfg);
    for sc in scs {
        let sc_idx = sc.bar_index;
        let sc_low = sc.reference_price;
        let proximity = sc_low * cfg.st_proximity_pct;
        // Look at bars [sc_idx + ar_window/2, n) for retests.
        let start = (sc_idx + (cfg.ar_window_bars as usize) / 2).min(n);
        for i in start..n {
            let bar = &bars[i];
            let dist = (bar_low(bar) - sc_low).abs();
            if dist > proximity {
                continue;
            }
            let avg = sma_volume(bars, i, cfg.volume_sma_bars as usize);
            if avg < 1e-9 || bar_vol(bar) > avg * cfg.st_volume_max_mult {
                continue;
            }
            // Reclaim required: close above sc_low.
            if bar_close(bar) <= sc_low {
                continue;
            }
            out.push(WyckoffEvent {
                kind: WyckoffEventKind::St,
                variant: "bull",
                score: 0.55,
                bar_index: i,
                reference_price: bar_low(bar),
                volume_ratio: bar_vol(bar) / avg,
                range_ratio: 0.0,
                note: format!("ST: low-vol retest of SC at {:.2}", sc_low),
            });
            break; // one ST per SC
        }
    }
    out
}

// ── LPS — Last Point of Support (higher-low after SOS) ────────────
//
// Heuristic: after a SOS, the first higher-low (close < SOS close
// but > prior swing low) on declining volume = LPS. Marks the W4
// of the impulse / Phase D pullback.
pub fn eval_lps(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < (cfg.volume_sma_bars as usize) + 5 {
        return out;
    }
    let sos_events = eval_sos(bars, cfg);
    for sos in sos_events {
        let sos_idx = sos.bar_index;
        let sos_close = sos.reference_price;
        let scan_end = (sos_idx + cfg.lps_lookforward_bars as usize).min(n - 1);
        if scan_end <= sos_idx + 1 {
            continue;
        }
        // Find the lowest LOW in (sos_idx, scan_end] that is still
        // ABOVE the SOS bar's low (= higher-low confirmation).
        let sos_low = bar_low(&bars[sos_idx]);
        let mut best_idx = 0usize;
        let mut best_low = f64::INFINITY;
        for k in (sos_idx + 1)..=scan_end {
            let l = bar_low(&bars[k]);
            if l <= sos_low {
                continue;
            }
            if l < best_low {
                best_low = l;
                best_idx = k;
            }
        }
        if best_idx == 0 || !best_low.is_finite() {
            continue;
        }
        // Volume should be DECREASING vs the SOS spike.
        let avg = sma_volume(bars, best_idx, cfg.volume_sma_bars as usize);
        if avg > 0.0 && bar_vol(&bars[best_idx]) > avg {
            continue; // too much volume — not a clean pullback
        }
        out.push(WyckoffEvent {
            kind: WyckoffEventKind::Lps,
            variant: "bull",
            score: 0.55,
            bar_index: best_idx,
            reference_price: best_low,
            volume_ratio: 0.0,
            range_ratio: 0.0,
            note: format!(
                "LPS: higher-low {:.2} after SOS at {:.2}",
                best_low, sos_close
            ),
        });
    }
    out
}

// ── PS — Preliminary Support (first volume spike on the way down) ─
//
// Heuristic: scan a downtrend; the FIRST bar whose volume exceeds
// SMA × climax_volume_mult / 1.3 (slightly looser than full SC) AND
// closes off the low (lower-wick reclaim) = PS. Distinct from SC by
// being EARLIER + smaller magnitude.
pub fn eval_ps(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < (cfg.volume_sma_bars as usize) + 10 {
        return out;
    }
    let start = n.saturating_sub(cfg.scan_lookback);
    let ps_threshold = cfg.climax_volume_mult / 1.3;
    let mut emitted = false;
    for i in start.max(cfg.volume_sma_bars as usize)..n {
        if emitted {
            break;
        }
        let bar = &bars[i];
        let avg = sma_volume(bars, i, cfg.volume_sma_bars as usize);
        if avg < 1e-9 {
            continue;
        }
        let vr = bar_vol(bar) / avg;
        if vr < ps_threshold || vr >= cfg.climax_volume_mult {
            continue; // SC zone — handled separately
        }
        let lower_wick =
            bar.open.to_f64().unwrap_or(0.0).min(bar_close(bar)) - bar_low(bar);
        let range = bar_range(bar).max(1e-9);
        if lower_wick < range * 0.30 {
            continue;
        }
        out.push(WyckoffEvent {
            kind: WyckoffEventKind::Ps,
            variant: "bull",
            score: 0.55,
            bar_index: i,
            reference_price: bar_low(bar),
            volume_ratio: vr,
            range_ratio: 0.0,
            note: format!("PS: vol {vr:.1}× SMA, lower-wick reclaim"),
        });
        emitted = true;
    }
    out
}

// ── BU/JAC — Back-Up after breakout (Jump-Across-Creek pullback) ─
//
// Heuristic: after a SOS that broke a recent range high, look for
// the next HIGHER-LOW that retests the broken range high from above
// (the "creek" being jumped). Volume on the BU should be light.
pub fn eval_bu(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < (cfg.volume_sma_bars as usize) + 10 {
        return out;
    }
    let sos_events = eval_sos(bars, cfg);
    for sos in sos_events {
        let sos_idx = sos.bar_index;
        let scan_end = (sos_idx + cfg.lps_lookforward_bars as usize).min(n - 1);
        // Define the "creek" as the highest HIGH in the 20 bars
        // preceding SOS (the range top SOS just broke).
        let creek_start = sos_idx.saturating_sub(20);
        let mut creek = f64::NEG_INFINITY;
        for k in creek_start..sos_idx {
            creek = creek.max(bar_high(&bars[k]));
        }
        if !creek.is_finite() {
            continue;
        }
        // Find a bar in (sos, scan_end] whose LOW touches creek
        // from above and CLOSES back above.
        let proximity = creek * 0.005; // 0.5% tolerance
        for k in (sos_idx + 1)..=scan_end {
            let l = bar_low(&bars[k]);
            let c = bar_close(&bars[k]);
            if (l - creek).abs() <= proximity && c > creek {
                let avg = sma_volume(bars, k, cfg.volume_sma_bars as usize);
                let light = avg < 1e-9 || bar_vol(&bars[k]) < avg;
                if !light {
                    continue;
                }
                out.push(WyckoffEvent {
                    kind: WyckoffEventKind::Bu,
                    variant: "bull",
                    score: 0.55,
                    bar_index: k,
                    reference_price: l,
                    volume_ratio: 0.0,
                    range_ratio: 0.0,
                    note: format!(
                        "BU/JAC: retest of creek {:.2} from above",
                        creek
                    ),
                });
                break;
            }
        }
    }
    out
}

// ── Test — Test of Spring (low-volume retest of spring low) ───────
//
// Mirror of ST but for Spring instead of SC. After a spring fires,
// the first low-volume retest of the spring low = Test event.
pub fn eval_test(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < (cfg.volume_sma_bars as usize) + 5 {
        return out;
    }
    let springs = eval_spring(bars, cfg);
    for sp in springs {
        let sp_idx = sp.bar_index;
        let sp_low = sp.reference_price;
        let proximity = sp_low * cfg.st_proximity_pct;
        let start = (sp_idx + (cfg.ar_window_bars as usize) / 2).min(n);
        for i in start..n {
            let bar = &bars[i];
            if (bar_low(bar) - sp_low).abs() > proximity {
                continue;
            }
            let avg = sma_volume(bars, i, cfg.volume_sma_bars as usize);
            if avg < 1e-9 || bar_vol(bar) > avg * cfg.st_volume_max_mult {
                continue;
            }
            if bar_close(bar) <= sp_low {
                continue;
            }
            out.push(WyckoffEvent {
                kind: WyckoffEventKind::Test,
                variant: "bull",
                score: 0.60,
                bar_index: i,
                reference_price: bar_low(bar),
                volume_ratio: bar_vol(bar) / avg,
                range_ratio: 0.0,
                note: format!("Test: low-vol retest of Spring at {:.2}", sp_low),
            });
            break;
        }
    }
    out
}

// ── FAZ 25.4.C — distribution-side evaluators (the missing half) ──
//
// User audit flag: \"Wyckoff eventleri neden eksik?\" Six events
// (AR, ST, LPS, PS, BU, Test) had only accumulation-side
// implementations — distribution counterparts (post-BC reaction
// LOW, LPSY = lower-high after SOW, PSY = preliminary supply,
// BU/JAC down, Test of UTAD) never fired. These six close that
// gap. All emit variant=\"bear\"; the writer'\\''s subkind suffix
// (ar_bear / st_bear / lps_bear / ps_bear / bu_bear / test_bear)
// distinguishes them in DB from their accumulation siblings.

// AR-distribution: lowest LOW within `ar_window` after BC (post-BC
// drop). Mirror of accumulation AR (highest HIGH after SC).
pub fn eval_ar_distribution(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    let window = cfg.ar_window_bars.max(3) as usize;
    if n < (cfg.volume_sma_bars as usize) + window + 2 {
        return out;
    }
    for bc in eval_buying_climax(bars, cfg) {
        let bc_idx = bc.bar_index;
        let end = (bc_idx + window).min(n - 1);
        if end <= bc_idx {
            continue;
        }
        let mut best_idx = bc_idx + 1;
        let mut best_low = bar_low(&bars[bc_idx + 1]);
        for k in (bc_idx + 1)..=end {
            let l = bar_low(&bars[k]);
            if l < best_low {
                best_low = l;
                best_idx = k;
            }
        }
        let a = atr(bars, bc_idx, 14).max(1e-9);
        if bc.reference_price - best_low < a {
            continue;
        }
        out.push(WyckoffEvent {
            kind: WyckoffEventKind::Ar,
            variant: "bear",
            score: 0.55,
            bar_index: best_idx,
            reference_price: best_low,
            volume_ratio: 0.0,
            range_ratio: 0.0,
            note: format!("AR (dist): low {:.2} after BC", best_low),
        });
    }
    out
}

// ST-distribution: low-vol retest of BC level (rejection close
// below BC high confirms supply).
pub fn eval_st_distribution(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < (cfg.volume_sma_bars as usize) + 5 {
        return out;
    }
    for bc in eval_buying_climax(bars, cfg) {
        let bc_idx = bc.bar_index;
        let bc_high = bc.reference_price;
        let proximity = bc_high * cfg.st_proximity_pct;
        let start = (bc_idx + (cfg.ar_window_bars as usize) / 2).min(n);
        for i in start..n {
            let bar = &bars[i];
            if (bar_high(bar) - bc_high).abs() > proximity {
                continue;
            }
            let avg = sma_volume(bars, i, cfg.volume_sma_bars as usize);
            if avg < 1e-9 || bar_vol(bar) > avg * cfg.st_volume_max_mult {
                continue;
            }
            if bar_close(bar) >= bc_high {
                continue;
            }
            out.push(WyckoffEvent {
                kind: WyckoffEventKind::St,
                variant: "bear",
                score: 0.55,
                bar_index: i,
                reference_price: bar_high(bar),
                volume_ratio: bar_vol(bar) / avg,
                range_ratio: 0.0,
                note: format!("ST (dist): low-vol retest of BC at {:.2}", bc_high),
            });
            break;
        }
    }
    out
}

// LPSY (LPS-distribution): lower-high after SOW with declining
// volume — Phase D markdown pullback marker.
pub fn eval_lps_distribution(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < (cfg.volume_sma_bars as usize) + 5 {
        return out;
    }
    for sow in eval_sow(bars, cfg) {
        let sow_idx = sow.bar_index;
        let scan_end = (sow_idx + cfg.lps_lookforward_bars as usize).min(n - 1);
        if scan_end <= sow_idx + 1 {
            continue;
        }
        let sow_high = bar_high(&bars[sow_idx]);
        let mut best_idx = 0usize;
        let mut best_high = f64::NEG_INFINITY;
        for k in (sow_idx + 1)..=scan_end {
            let h = bar_high(&bars[k]);
            if h >= sow_high {
                continue;
            }
            if h > best_high {
                best_high = h;
                best_idx = k;
            }
        }
        if best_idx == 0 || !best_high.is_finite() {
            continue;
        }
        let avg = sma_volume(bars, best_idx, cfg.volume_sma_bars as usize);
        if avg > 0.0 && bar_vol(&bars[best_idx]) > avg {
            continue;
        }
        out.push(WyckoffEvent {
            kind: WyckoffEventKind::Lps,
            variant: "bear",
            score: 0.55,
            bar_index: best_idx,
            reference_price: best_high,
            volume_ratio: 0.0,
            range_ratio: 0.0,
            note: format!("LPSY: lower-high {:.2} after SOW", best_high),
        });
    }
    out
}

// PSY (PS-distribution): pre-BC volume spike with upper wick.
pub fn eval_ps_distribution(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < (cfg.volume_sma_bars as usize) + 10 {
        return out;
    }
    let start = n.saturating_sub(cfg.scan_lookback);
    let psy_threshold = cfg.climax_volume_mult / 1.3;
    let mut emitted = false;
    for i in start.max(cfg.volume_sma_bars as usize)..n {
        if emitted {
            break;
        }
        let bar = &bars[i];
        let avg = sma_volume(bars, i, cfg.volume_sma_bars as usize);
        if avg < 1e-9 {
            continue;
        }
        let vr = bar_vol(bar) / avg;
        if vr < psy_threshold || vr >= cfg.climax_volume_mult {
            continue;
        }
        let upper_wick =
            bar_high(bar) - bar.open.to_f64().unwrap_or(0.0).max(bar_close(bar));
        let range = bar_range(bar).max(1e-9);
        if upper_wick < range * 0.30 {
            continue;
        }
        out.push(WyckoffEvent {
            kind: WyckoffEventKind::Ps,
            variant: "bear",
            score: 0.55,
            bar_index: i,
            reference_price: bar_high(bar),
            volume_ratio: vr,
            range_ratio: 0.0,
            note: format!("PSY: vol {vr:.1}× SMA, upper-wick rejection"),
        });
        emitted = true;
    }
    out
}

// BU-distribution: rally back to broken range LOW (creek-down).
pub fn eval_bu_distribution(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < (cfg.volume_sma_bars as usize) + 10 {
        return out;
    }
    for sow in eval_sow(bars, cfg) {
        let sow_idx = sow.bar_index;
        let scan_end = (sow_idx + cfg.lps_lookforward_bars as usize).min(n - 1);
        let creek_start = sow_idx.saturating_sub(20);
        let mut creek = f64::INFINITY;
        for k in creek_start..sow_idx {
            creek = creek.min(bar_low(&bars[k]));
        }
        if !creek.is_finite() {
            continue;
        }
        let proximity = creek * 0.005;
        for k in (sow_idx + 1)..=scan_end {
            let h = bar_high(&bars[k]);
            let c = bar_close(&bars[k]);
            if (h - creek).abs() <= proximity && c < creek {
                let avg = sma_volume(bars, k, cfg.volume_sma_bars as usize);
                let light = avg < 1e-9 || bar_vol(&bars[k]) < avg;
                if !light {
                    continue;
                }
                out.push(WyckoffEvent {
                    kind: WyckoffEventKind::Bu,
                    variant: "bear",
                    score: 0.55,
                    bar_index: k,
                    reference_price: h,
                    volume_ratio: 0.0,
                    range_ratio: 0.0,
                    note: format!("BU (dist): retest creek {:.2} from below", creek),
                });
                break;
            }
        }
    }
    out
}

// Test-distribution: low-vol retest of UTAD high.
pub fn eval_test_distribution(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    let n = bars.len();
    if n < (cfg.volume_sma_bars as usize) + 5 {
        return out;
    }
    for u in eval_utad(bars, cfg) {
        let u_idx = u.bar_index;
        let u_high = u.reference_price;
        let proximity = u_high * cfg.st_proximity_pct;
        let start = (u_idx + (cfg.ar_window_bars as usize) / 2).min(n);
        for i in start..n {
            let bar = &bars[i];
            if (bar_high(bar) - u_high).abs() > proximity {
                continue;
            }
            let avg = sma_volume(bars, i, cfg.volume_sma_bars as usize);
            if avg < 1e-9 || bar_vol(bar) > avg * cfg.st_volume_max_mult {
                continue;
            }
            if bar_close(bar) >= u_high {
                continue;
            }
            out.push(WyckoffEvent {
                kind: WyckoffEventKind::Test,
                variant: "bear",
                score: 0.60,
                bar_index: i,
                reference_price: bar_high(bar),
                volume_ratio: bar_vol(bar) / avg,
                range_ratio: 0.0,
                note: format!("Test (dist): low-vol retest of UTAD at {:.2}", u_high),
            });
            break;
        }
    }
    out
}

pub static WYCKOFF_SPECS: &[WyckoffSpec] = &[
    WyckoffSpec { name: "ps",        kind: WyckoffEventKind::Ps,     eval: eval_ps },
    WyckoffSpec { name: "sc",        kind: WyckoffEventKind::Sc,     eval: eval_selling_climax },
    WyckoffSpec { name: "ar",        kind: WyckoffEventKind::Ar,     eval: eval_ar },
    WyckoffSpec { name: "st",        kind: WyckoffEventKind::St,     eval: eval_st },
    WyckoffSpec { name: "spring",    kind: WyckoffEventKind::Spring, eval: eval_spring },
    WyckoffSpec { name: "test",      kind: WyckoffEventKind::Test,   eval: eval_test },
    WyckoffSpec { name: "sos",       kind: WyckoffEventKind::Sos,    eval: eval_sos },
    WyckoffSpec { name: "lps",       kind: WyckoffEventKind::Lps,    eval: eval_lps },
    WyckoffSpec { name: "bu",        kind: WyckoffEventKind::Bu,     eval: eval_bu },
    WyckoffSpec { name: "bc",        kind: WyckoffEventKind::Bc,     eval: eval_buying_climax },
    WyckoffSpec { name: "utad",      kind: WyckoffEventKind::Utad,   eval: eval_utad },
    WyckoffSpec { name: "sow",       kind: WyckoffEventKind::Sow,    eval: eval_sow },
    // FAZ 25.4.C — distribution-side variants.
    WyckoffSpec { name: "ar_dist",   kind: WyckoffEventKind::Ar,     eval: eval_ar_distribution },
    WyckoffSpec { name: "st_dist",   kind: WyckoffEventKind::St,     eval: eval_st_distribution },
    WyckoffSpec { name: "lpsy",      kind: WyckoffEventKind::Lps,    eval: eval_lps_distribution },
    WyckoffSpec { name: "psy",       kind: WyckoffEventKind::Ps,     eval: eval_ps_distribution },
    WyckoffSpec { name: "bu_dist",   kind: WyckoffEventKind::Bu,     eval: eval_bu_distribution },
    WyckoffSpec { name: "test_dist", kind: WyckoffEventKind::Test,   eval: eval_test_distribution },
];

/// Run every spec against the bar slice. Returns all events above
/// the configured min score.
pub fn detect_events(bars: &[Bar], cfg: &WyckoffConfig) -> Vec<WyckoffEvent> {
    let mut out = Vec::new();
    for spec in WYCKOFF_SPECS {
        for ev in (spec.eval)(bars, cfg) {
            if (ev.score as f32) >= cfg.min_structural_score {
                out.push(ev);
            }
        }
    }
    out
}

/// FAZ 25.4.E — Wyckoff context filter (Gemini + Claude audit).
///
/// Spring is a Phase-C event that lives ONLY inside an active
/// Accumulation range; UTAD is the Phase-C event for an active
/// Distribution range. The raw evaluators in this module fire on
/// any wick + reclaim geometry and don't know about the surrounding
/// schematic, so they spam at every minor sweep. This post-filter
/// drops Spring/UTAD events that fall outside their canonical
/// schematic context, AND caps each range to a single
/// strongest-scored Spring (or UTAD) — the Wyckoff doctrine says
/// Phase C contains ONE such event per range.
///
/// Other event kinds are passed through unchanged.
///
/// `ranges` should come from `detect_ranges(&events)` BEFORE this
/// filter runs — the filter operates on the raw event stream and
/// the range list together.
pub fn filter_phase_c_events_in_context(
    events: Vec<WyckoffEvent>,
    ranges: &[crate::range::WyckoffRange],
) -> Vec<WyckoffEvent> {
    use crate::phase::WyckoffBias;

    // Index the strongest Spring per Accumulation range and the
    // strongest UTAD per Distribution range. Spring/UTAD events
    // outside any range get dropped. Other events pass through.
    let accum_ranges: Vec<&crate::range::WyckoffRange> = ranges
        .iter()
        .filter(|r| r.bias == WyckoffBias::Accumulation)
        .collect();
    let dist_ranges: Vec<&crate::range::WyckoffRange> = ranges
        .iter()
        .filter(|r| r.bias == WyckoffBias::Distribution)
        .collect();

    let find_range = |bar: usize, accum: bool| -> Option<usize> {
        // Returns the index (into accum_ranges or dist_ranges) of
        // the range containing this bar, if any.
        let pool = if accum { &accum_ranges } else { &dist_ranges };
        for (i, r) in pool.iter().enumerate() {
            if bar >= r.start_bar && bar <= r.end_bar {
                return Some(i);
            }
        }
        None
    };

    // First pass — separate Spring/UTAD events from the rest.
    let mut springs: Vec<WyckoffEvent> = Vec::new();
    let mut utads: Vec<WyckoffEvent> = Vec::new();
    let mut others: Vec<WyckoffEvent> = Vec::new();
    for ev in events {
        match ev.kind {
            WyckoffEventKind::Spring => springs.push(ev),
            WyckoffEventKind::Utad => utads.push(ev),
            _ => others.push(ev),
        }
    }

    // Per-range strongest Spring (Accumulation context only).
    let mut best_spring_per_range: std::collections::HashMap<
        usize,
        WyckoffEvent,
    > = std::collections::HashMap::new();
    for ev in springs {
        let Some(idx) = find_range(ev.bar_index, true) else {
            continue;
        };
        let entry = best_spring_per_range.entry(idx);
        match entry {
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(ev);
            }
            std::collections::hash_map::Entry::Occupied(mut e) => {
                if ev.score > e.get().score {
                    e.insert(ev);
                }
            }
        }
    }

    // Per-range strongest UTAD (Distribution context only).
    let mut best_utad_per_range: std::collections::HashMap<
        usize,
        WyckoffEvent,
    > = std::collections::HashMap::new();
    for ev in utads {
        let Some(idx) = find_range(ev.bar_index, false) else {
            continue;
        };
        let entry = best_utad_per_range.entry(idx);
        match entry {
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(ev);
            }
            std::collections::hash_map::Entry::Occupied(mut e) => {
                if ev.score > e.get().score {
                    e.insert(ev);
                }
            }
        }
    }

    // Reassemble the event list in chronological order.
    others.extend(best_spring_per_range.into_values());
    others.extend(best_utad_per_range.into_values());
    others.sort_by_key(|e| e.bar_index);
    others
}

#[cfg(test)]
mod context_filter_tests {
    use super::*;
    use crate::range::WyckoffRange;
    use crate::phase::{WyckoffBias, WyckoffPhase};

    fn ev(kind: WyckoffEventKind, bar: usize, score: f64) -> WyckoffEvent {
        WyckoffEvent {
            kind,
            variant: "bull",
            score,
            bar_index: bar,
            reference_price: 100.0,
            volume_ratio: 0.0,
            range_ratio: 0.0,
            note: String::new(),
        }
    }

    fn range(bias: WyckoffBias, start: usize, end: usize) -> WyckoffRange {
        WyckoffRange {
            bias,
            phase: WyckoffPhase::B,
            start_bar: start,
            end_bar: end,
            range_high: 110.0,
            range_low: 90.0,
            event_indices: vec![],
            completed: false,
        }
    }

    #[test]
    fn spring_outside_accumulation_dropped() {
        let events = vec![ev(WyckoffEventKind::Spring, 50, 0.7)];
        let ranges = vec![range(WyckoffBias::Distribution, 40, 60)];
        let filtered = filter_phase_c_events_in_context(events, &ranges);
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn spring_inside_accumulation_kept() {
        let events = vec![ev(WyckoffEventKind::Spring, 50, 0.7)];
        let ranges = vec![range(WyckoffBias::Accumulation, 40, 60)];
        let filtered = filter_phase_c_events_in_context(events, &ranges);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].kind, WyckoffEventKind::Spring);
    }

    #[test]
    fn multiple_springs_only_strongest_kept_per_range() {
        let events = vec![
            ev(WyckoffEventKind::Spring, 45, 0.6),
            ev(WyckoffEventKind::Spring, 50, 0.9), // strongest
            ev(WyckoffEventKind::Spring, 55, 0.7),
        ];
        let ranges = vec![range(WyckoffBias::Accumulation, 40, 60)];
        let filtered = filter_phase_c_events_in_context(events, &ranges);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].bar_index, 50);
        assert_eq!(filtered[0].score, 0.9);
    }

    #[test]
    fn utad_inside_distribution_kept_one_strongest() {
        let events = vec![
            ev(WyckoffEventKind::Utad, 45, 0.6),
            ev(WyckoffEventKind::Utad, 50, 0.85),
            ev(WyckoffEventKind::Utad, 55, 0.7),
        ];
        let ranges = vec![range(WyckoffBias::Distribution, 40, 60)];
        let filtered = filter_phase_c_events_in_context(events, &ranges);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].score, 0.85);
    }

    #[test]
    fn other_events_pass_through_unchanged() {
        let events = vec![
            ev(WyckoffEventKind::Sc, 45, 0.8),
            ev(WyckoffEventKind::Bc, 55, 0.8),
            ev(WyckoffEventKind::Sos, 60, 0.7),
        ];
        let filtered = filter_phase_c_events_in_context(events, &[]);
        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn springs_per_range_isolated() {
        // Two separate accumulation ranges, each with their own
        // strongest Spring.
        let events = vec![
            ev(WyckoffEventKind::Spring, 50, 0.7), // range A
            ev(WyckoffEventKind::Spring, 55, 0.8), // range A — wins
            ev(WyckoffEventKind::Spring, 100, 0.65), // range B — wins
            ev(WyckoffEventKind::Spring, 105, 0.6),  // range B
        ];
        let ranges = vec![
            range(WyckoffBias::Accumulation, 40, 60),
            range(WyckoffBias::Accumulation, 90, 110),
        ];
        let filtered = filter_phase_c_events_in_context(events, &ranges);
        assert_eq!(filtered.len(), 2);
        let bars: Vec<usize> = filtered.iter().map(|e| e.bar_index).collect();
        assert!(bars.contains(&55));
        assert!(bars.contains(&100));
    }
}

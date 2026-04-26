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
        // SC = wide-range, high-volume bar with lower close + long
        // lower wick (classic panic flush).
        let lower_wick = bar.open.to_f64().unwrap_or(0.0).min(bar_close(bar)) - bar_low(bar);
        if body < 0.0 && lower_wick > bar_range(bar) * 0.4 {
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
        if body > 0.0 && upper_wick > bar_range(bar) * 0.4 {
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
            score: 0.45,
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

pub static WYCKOFF_SPECS: &[WyckoffSpec] = &[
    WyckoffSpec { name: "ps",     kind: WyckoffEventKind::Ps,     eval: eval_ps },
    WyckoffSpec { name: "sc",     kind: WyckoffEventKind::Sc,     eval: eval_selling_climax },
    WyckoffSpec { name: "ar",     kind: WyckoffEventKind::Ar,     eval: eval_ar },
    WyckoffSpec { name: "st",     kind: WyckoffEventKind::St,     eval: eval_st },
    WyckoffSpec { name: "spring", kind: WyckoffEventKind::Spring, eval: eval_spring },
    WyckoffSpec { name: "test",   kind: WyckoffEventKind::Test,   eval: eval_test },
    WyckoffSpec { name: "sos",    kind: WyckoffEventKind::Sos,    eval: eval_sos },
    WyckoffSpec { name: "lps",    kind: WyckoffEventKind::Lps,    eval: eval_lps },
    WyckoffSpec { name: "bu",     kind: WyckoffEventKind::Bu,     eval: eval_bu },
    WyckoffSpec { name: "bc",     kind: WyckoffEventKind::Bc,     eval: eval_buying_climax },
    WyckoffSpec { name: "utad",   kind: WyckoffEventKind::Utad,   eval: eval_utad },
    WyckoffSpec { name: "sow",    kind: WyckoffEventKind::Sow,    eval: eval_sow },
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

//! Gap specification dispatch table — each entry is a `(name, eval)`
//! pair. The detector runs the table top-to-bottom and keeps the first
//! match (priority order: more specific first). Adding a gap kind means
//! appending a row; no central `match` to edit (CLAUDE.md #1).

use crate::config::GapConfig;
use qtss_domain::v2::bar::Bar;
use rust_decimal::prelude::ToPrimitive;

/// A single gap detection candidate anchored at bar index `gap_bar`
/// (the bar that opens with the gap). `partner_bar` is optional and
/// used by multi-bar structures like island reversals.
#[derive(Debug, Clone)]
pub struct GapMatch {
    pub score: f64,
    pub variant: &'static str, // "bull" or "bear"
    pub gap_bar: usize,
    pub partner_bar: Option<usize>,
    pub gap_pct: f64,
    pub volume_ratio: f64,
}

pub struct GapSpec {
    pub name: &'static str,
    /// Evaluate on the bar window. Last bar in `bars` is the current
    /// (potentially forming) bar. Returns `Some` on match.
    pub eval: fn(&[Bar], &GapConfig) -> Option<GapMatch>,
}

/// Priority order — island reversal checked first (most specific),
/// then exhaustion / runaway / breakaway, common_gap last (fallback).
pub static GAP_SPECS: &[GapSpec] = &[
    GapSpec { name: "island_reversal", eval: eval_island_reversal },
    GapSpec { name: "exhaustion_gap",  eval: eval_exhaustion_gap },
    GapSpec { name: "runaway_gap",     eval: eval_runaway_gap },
    GapSpec { name: "breakaway_gap",   eval: eval_breakaway_gap },
    GapSpec { name: "common_gap",      eval: eval_common_gap },
];

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn f(d: rust_decimal::Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

/// Signed gap fraction at bar index `i` (>= 1): `(open_i - close_{i-1}) / close_{i-1}`.
fn gap_pct_at(bars: &[Bar], i: usize) -> f64 {
    if i == 0 {
        return 0.0;
    }
    let prev_close = f(bars[i - 1].close);
    if prev_close.abs() < f64::EPSILON {
        return 0.0;
    }
    (f(bars[i].open) - prev_close) / prev_close
}

fn volume_sma(bars: &[Bar], end_exclusive: usize, n: usize) -> f64 {
    let start = end_exclusive.saturating_sub(n);
    let slice = &bars[start..end_exclusive];
    if slice.is_empty() {
        return 0.0;
    }
    let sum: f64 = slice.iter().map(|b| f(b.volume)).sum();
    sum / slice.len() as f64
}

fn volume_ratio(bars: &[Bar], i: usize, cfg: &GapConfig) -> f64 {
    let base = volume_sma(bars, i, cfg.volume_sma_bars);
    if base <= 0.0 {
        return 0.0;
    }
    f(bars[i].volume) / base
}

/// Last-N avg range / mid-price → proxy for "flat consolidation".
fn consolidation_range_pct(bars: &[Bar], end_exclusive: usize, n: usize) -> f64 {
    let start = end_exclusive.saturating_sub(n);
    let slice = &bars[start..end_exclusive];
    if slice.len() < 2 {
        return f64::INFINITY;
    }
    let mut hi = f(slice[0].high);
    let mut lo = f(slice[0].low);
    for b in slice {
        hi = hi.max(f(b.high));
        lo = lo.min(f(b.low));
    }
    let mid = (hi + lo) * 0.5;
    if mid.abs() < f64::EPSILON {
        return f64::INFINITY;
    }
    (hi - lo) / mid
}

/// Cumulative return over last `n` bars ending at `end_exclusive`.
fn trend_return(bars: &[Bar], end_exclusive: usize, n: usize) -> f64 {
    if end_exclusive < n + 1 {
        return 0.0;
    }
    let start = end_exclusive - n;
    let first_close = f(bars[start - 1].close);
    let last_close = f(bars[end_exclusive - 1].close);
    if first_close.abs() < f64::EPSILON {
        return 0.0;
    }
    (last_close - first_close) / first_close
}

// ---------------------------------------------------------------------------
// Individual gap evals — operate on the final bar (len-1) as the gap bar.
// ---------------------------------------------------------------------------

fn base_gap_info(bars: &[Bar], cfg: &GapConfig) -> Option<(usize, f64, f64, &'static str)> {
    if bars.len() < cfg.volume_sma_bars + 2 {
        return None;
    }
    let i = bars.len() - 1;
    let gp = gap_pct_at(bars, i);
    if gp.abs() < cfg.min_gap_pct {
        return None;
    }
    let vr = volume_ratio(bars, i, cfg);
    let variant = if gp > 0.0 { "bull" } else { "bear" };
    Some((i, gp, vr, variant))
}

fn eval_common_gap(bars: &[Bar], cfg: &GapConfig) -> Option<GapMatch> {
    let (i, gp, vr, variant) = base_gap_info(bars, cfg)?;
    // Fallback — low-ish confidence score scaled by gap magnitude only.
    let score = (gp.abs() / (cfg.min_gap_pct * 5.0)).min(1.0) * 0.6;
    Some(GapMatch {
        score,
        variant,
        gap_bar: i,
        partner_bar: None,
        gap_pct: gp,
        volume_ratio: vr,
    })
}

fn eval_breakaway_gap(bars: &[Bar], cfg: &GapConfig) -> Option<GapMatch> {
    let (i, gp, vr, variant) = base_gap_info(bars, cfg)?;
    if vr < cfg.vol_mult_breakaway {
        return None;
    }
    let range_pct = consolidation_range_pct(bars, i, cfg.consolidation_lookback);
    if range_pct > cfg.range_flat_pct {
        return None;
    }
    let tightness = (cfg.range_flat_pct / range_pct.max(1e-9)).min(3.0) / 3.0;
    let vol_quality = ((vr / cfg.vol_mult_breakaway) - 1.0).clamp(0.0, 1.0);
    let score = (0.55 + 0.25 * tightness + 0.2 * vol_quality).min(1.0);
    Some(GapMatch {
        score,
        variant,
        gap_bar: i,
        partner_bar: None,
        gap_pct: gp,
        volume_ratio: vr,
    })
}

fn eval_runaway_gap(bars: &[Bar], cfg: &GapConfig) -> Option<GapMatch> {
    let (i, gp, vr, variant) = base_gap_info(bars, cfg)?;
    if vr < cfg.vol_mult_runaway {
        return None;
    }
    let ret = trend_return(bars, i, cfg.runaway_trend_bars);
    let dir_sign = if gp > 0.0 { 1.0 } else { -1.0 };
    // Trend must be in same direction as the gap and exceed min threshold.
    if ret * dir_sign < cfg.runaway_trend_min_pct {
        return None;
    }
    let trend_quality = (ret.abs() / (cfg.runaway_trend_min_pct * 3.0)).min(1.0);
    let score = (0.55 + 0.25 * trend_quality
        + 0.2 * ((vr / cfg.vol_mult_runaway) - 1.0).clamp(0.0, 1.0))
        .min(1.0);
    Some(GapMatch {
        score,
        variant,
        gap_bar: i,
        partner_bar: None,
        gap_pct: gp,
        volume_ratio: vr,
    })
}

fn eval_exhaustion_gap(bars: &[Bar], cfg: &GapConfig) -> Option<GapMatch> {
    let (i, gp, vr, _variant) = base_gap_info(bars, cfg)?;
    if vr < cfg.vol_mult_exhaustion {
        return None;
    }
    // Must follow an established trend (same direction as the gap).
    let ret = trend_return(bars, i, cfg.runaway_trend_bars);
    let dir_sign = if gp > 0.0 { 1.0 } else { -1.0 };
    if ret * dir_sign < cfg.runaway_trend_min_pct {
        return None;
    }
    // Reversal confirmation: any of the next `exhaustion_reversal_bars`
    // bars (including the gap bar itself) must close back through the
    // pre-gap close. In a live setting this may not yet be available —
    // we only emit once confirmed.
    let pre_gap_close = f(bars[i - 1].close);
    let mut reversed = false;
    let end = (i + cfg.exhaustion_reversal_bars).min(bars.len() - 1);
    for k in i..=end {
        let c = f(bars[k].close);
        let crossed = if dir_sign > 0.0 { c < pre_gap_close } else { c > pre_gap_close };
        if crossed {
            reversed = true;
            break;
        }
    }
    if !reversed {
        return None;
    }
    // Bias exhaustion above runaway when the reversal is confirmed — the
    // reversal is the whole signal.
    let score = (0.88 + 0.12 * ((vr / cfg.vol_mult_exhaustion) - 1.0).clamp(0.0, 1.0)).min(1.0);
    // Exhaustion flips direction semantically: bear-continuation gap up at
    // trend top signals reversal → label by reversal direction.
    let reversal_variant = if dir_sign > 0.0 { "bear" } else { "bull" };
    Some(GapMatch {
        score,
        variant: reversal_variant,
        gap_bar: i,
        partner_bar: None,
        gap_pct: gp,
        volume_ratio: vr,
    })
}

fn eval_island_reversal(bars: &[Bar], cfg: &GapConfig) -> Option<GapMatch> {
    let (i, gp_close, _vr_close, _variant) = base_gap_info(bars, cfg)?;
    // Scan backwards for an opposing-sign gap within `island_max_bars`.
    let min_k = i.saturating_sub(cfg.island_max_bars);
    for k in (min_k + 1..i).rev() {
        let gp_open = gap_pct_at(bars, k);
        if gp_open.abs() < cfg.min_gap_pct {
            continue;
        }
        // Opposite signs?
        if gp_open.signum() == gp_close.signum() {
            continue;
        }
        // Found island: opening gap at bar k (direction = sign(gp_open)),
        // closing gap at bar i (direction = sign(gp_close)).
        // Reversal direction = direction of the closing gap.
        let variant = if gp_close > 0.0 { "bull" } else { "bear" };
        let magnitude = (gp_open.abs() + gp_close.abs()) / (cfg.min_gap_pct * 4.0);
        let score = (0.75 + 0.25 * magnitude.min(1.0)).min(1.0);
        let vr_close = volume_ratio(bars, i, cfg);
        return Some(GapMatch {
            score,
            variant,
            gap_bar: i,
            partner_bar: Some(k),
            gap_pct: gp_close,
            volume_ratio: vr_close,
        });
    }
    None
}

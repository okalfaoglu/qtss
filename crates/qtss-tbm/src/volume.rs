//! Volume Pillar — MFI, OBV/CVD divergence, volume spike.
//!
//! P22d-div — OBV/CVD gates are no longer binary slope signs (`slope > 0
//! => +25`). That model scored poorly exactly at the turning point, when
//! the slope is still negative by inertia but new flow has stopped
//! confirming price. We now compare **half-window swings**: for a bottom
//! hypothesis, if price makes a *lower* low in the second half of the
//! window while OBV/CVD makes an *equal or higher* low, that is a textbook
//! bullish divergence (hidden accumulation). Mirrored for tops. Binary
//! sign is retained as a weaker fallback so trending-only confirmations
//! still contribute, but at a reduced weight.
//!
//! Window length is caller-chosen (typically 20 bars ending at the anchor
//! bar). Slices shorter than 6 bars skip divergence and only use the
//! fallback sign rule on the last slope.

use crate::config::TbmEffortResultTuning;
use crate::pillar::{PillarKind, PillarScore};

/// P24 — Wyckoff "effort vs result" volume-law detector. Scans the
/// last `cfg.scan_bars` bars of the supplied window and returns
/// (bonus_points, details). `opens/highs/lows/closes/vols` must be the
/// same length and end at the anchor bar. A 20-bar trailing average
/// (or shorter if the window is short) establishes the "normal" range
/// and volume baseline.
///
/// Scored bars (all three capped by `cfg.max_bonus_pts`):
///   * No-supply down-bar (is_bottom hypothesis): bearish close, small
///     range, low volume → sellers exhausted.
///   * No-demand up-bar (top hypothesis): bullish close, small range,
///     low volume → buyers exhausted.
///   * Absorption: high volume, small range, close mid-range → effort
///     without result (supply soaking demand, or vice-versa).
#[must_use]
pub fn score_effort_result(
    opens: &[f64],
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    vols: &[f64],
    is_bottom: bool,
    cfg: &TbmEffortResultTuning,
) -> (f64, Vec<String>) {
    if !cfg.enabled {
        return (0.0, Vec::new());
    }
    let n = opens
        .len()
        .min(highs.len())
        .min(lows.len())
        .min(closes.len())
        .min(vols.len());
    if n < 6 {
        return (0.0, Vec::new());
    }

    // Baseline = 20-bar (or available) trailing avg.
    let base_start = n.saturating_sub(20).min(n);
    let base_len = (n - base_start).max(1) as f64;
    let avg_range: f64 = (base_start..n)
        .map(|i| (highs[i] - lows[i]).max(0.0))
        .sum::<f64>()
        / base_len;
    let avg_vol: f64 = vols[base_start..n].iter().sum::<f64>() / base_len;
    if avg_range <= 0.0 || avg_vol <= 0.0 {
        return (0.0, Vec::new());
    }

    let scan_start = n.saturating_sub(cfg.scan_bars).max(1);
    let mut pts = 0.0_f64;
    let mut details = Vec::new();

    for i in scan_start..n {
        let range_i = (highs[i] - lows[i]).max(0.0);
        if range_i <= 0.0 {
            continue;
        }
        let range_small = range_i <= cfg.range_small_ratio * avg_range;
        let vol_low = vols[i] <= cfg.vol_low_ratio * avg_vol;
        let vol_high = vols[i] >= cfg.vol_high_ratio * avg_vol;
        let bearish = closes[i] < opens[i];
        let bullish = closes[i] > opens[i];
        let close_mid =
            (closes[i] - lows[i]).abs() >= 0.25 * range_i && (highs[i] - closes[i]).abs() >= 0.25 * range_i;

        if is_bottom && bearish && range_small && vol_low {
            pts += cfg.no_supply_demand_pts;
            details.push(format!("no-supply down-bar @ {i} (+{:.0})", cfg.no_supply_demand_pts));
        }
        if !is_bottom && bullish && range_small && vol_low {
            pts += cfg.no_supply_demand_pts;
            details.push(format!("no-demand up-bar @ {i} (+{:.0})", cfg.no_supply_demand_pts));
        }
        if vol_high && range_small && close_mid {
            pts += cfg.absorption_pts;
            details.push(format!("absorption bar @ {i} (+{:.0})", cfg.absorption_pts));
        }
        if pts >= cfg.max_bonus_pts {
            break;
        }
    }

    (pts.min(cfg.max_bonus_pts), details)
}

/// Volume pillar skoru hesaplar.
///
/// Girdiler:
/// - `mfi`: anchor barındaki Money Flow Index (0–100)
/// - `price_window`: kapanış serisi (anchor ile biten, tipik 20 bar)
/// - `obv_window`: aynı aralıkta OBV serisi
/// - `cvd_window`: aynı aralıkta CVD serisi
/// - `volume_last`, `volume_avg`: anchor bar hacmi + 20-bar ortalaması
/// - `is_bottom_search`: true = dip, false = tepe
#[must_use]
pub fn score_volume(
    mfi: f64,
    price_window: &[f64],
    obv_window: &[f64],
    cvd_window: &[f64],
    volume_last: f64,
    volume_avg: f64,
    is_bottom_search: bool,
) -> PillarScore {
    let mut score = 0.0_f64;
    let mut details = Vec::new();

    // 1) MFI (max 30 puan)
    if is_bottom_search {
        if mfi < 35.0 {
            let pts = (30.0 * (35.0 - mfi) / 35.0).clamp(0.0, 30.0);
            score += pts;
            details.push(format!("MFI oversold / dip zone {mfi:.1} (+{pts:.1})"));
        }
    } else if mfi > 65.0 {
        let pts = (30.0 * (mfi - 65.0) / 35.0).clamp(0.0, 30.0);
        score += pts;
        details.push(format!("MFI overbought / top zone {mfi:.1} (+{pts:.1})"));
    }

    // 2) OBV divergence (max 25 puan) — dogmatic swing comparison,
    //    fallback to sign-of-slope at reduced weight.
    let (obv_pts, obv_msg) = divergence_score(price_window, obv_window, is_bottom_search, "OBV");
    if obv_pts > 0.0 {
        score += obv_pts;
        details.push(obv_msg);
    }

    // 3) CVD divergence (max 25 puan)
    let (cvd_pts, cvd_msg) = divergence_score(price_window, cvd_window, is_bottom_search, "CVD");
    if cvd_pts > 0.0 {
        score += cvd_pts;
        details.push(cvd_msg);
    }

    // 4) Volume spike — climactic volume (max 20 puan)
    const VOL_SPIKE_LO: f64 = 1.5;
    const VOL_SPIKE_HI: f64 = 3.0;
    if volume_avg > 0.0 {
        let ratio = volume_last / volume_avg;
        if ratio > VOL_SPIKE_LO {
            let span = VOL_SPIKE_HI - VOL_SPIKE_LO;
            let pts = (20.0 * (ratio - VOL_SPIKE_LO) / span).clamp(0.0, 20.0);
            score += pts;
            details.push(format!("Volume spike {ratio:.1}x avg (+{pts:.1})"));
        }
    }

    PillarScore {
        kind: PillarKind::Volume,
        score: score.min(100.0),
        weight: 0.25,
        details,
    }
}

/// Half-window swing comparison between price and a flow indicator
/// (OBV or CVD). Returns (points, explanation).
///
/// Bottom search (bullish divergence):
///   price makes lower low in H2 vs H1, flow holds equal or higher low.
///   Strong div → 25 pts. Partial (price flat but flow rising) → 12 pts.
/// Top search is mirrored on highs.
///
/// Fallback (short series or no divergence): binary slope sign at a
/// reduced 10 pts so trending confirmations still show up but don't
/// over-reward mid-trend reads.
fn divergence_score(
    price: &[f64],
    flow: &[f64],
    is_bottom: bool,
    label: &str,
) -> (f64, String) {
    let n = price.len().min(flow.len());
    if n < 6 {
        return fallback_slope(flow, is_bottom, label);
    }
    let mid = n / 2;
    // H1 = [0..mid), H2 = [mid..n)
    let p_h1 = &price[..mid];
    let p_h2 = &price[mid..n];
    let f_h1 = &flow[..mid];
    let f_h2 = &flow[mid..n];

    if is_bottom {
        let p_low1 = min_f(p_h1);
        let p_low2 = min_f(p_h2);
        let f_low1 = min_f(f_h1);
        let f_low2 = min_f(f_h2);
        // Strong: price lower low AND flow higher low (classic bullish div).
        if p_low2 < p_low1 && f_low2 > f_low1 {
            return (
                25.0,
                format!("{label} bullish divergence (price LL, {label} HL)"),
            );
        }
        // Partial: price roughly equal/slightly lower low AND flow rising.
        if p_low2 <= p_low1 * 1.005 && f_low2 >= f_low1 {
            return (
                12.0,
                format!("{label} partial bullish divergence (flow holding)"),
            );
        }
    } else {
        let p_hi1 = max_f(p_h1);
        let p_hi2 = max_f(p_h2);
        let f_hi1 = max_f(f_h1);
        let f_hi2 = max_f(f_h2);
        if p_hi2 > p_hi1 && f_hi2 < f_hi1 {
            return (
                25.0,
                format!("{label} bearish divergence (price HH, {label} LH)"),
            );
        }
        if p_hi2 >= p_hi1 * 0.995 && f_hi2 <= f_hi1 {
            return (
                12.0,
                format!("{label} partial bearish divergence (flow fading)"),
            );
        }
    }

    fallback_slope(flow, is_bottom, label)
}

fn fallback_slope(flow: &[f64], is_bottom: bool, label: &str) -> (f64, String) {
    if flow.len() < 2 {
        return (0.0, String::new());
    }
    let slope = flow[flow.len() - 1] - flow[0];
    if is_bottom && slope > 0.0 {
        (10.0, format!("{label} rising (trend confirmation)"))
    } else if !is_bottom && slope < 0.0 {
        (10.0, format!("{label} falling (trend confirmation)"))
    } else {
        (0.0, String::new())
    }
}

fn min_f(xs: &[f64]) -> f64 {
    xs.iter().cloned().filter(|v| v.is_finite()).fold(f64::INFINITY, f64::min)
}
fn max_f(xs: &[f64]) -> f64 {
    xs.iter().cloned().filter(|v| v.is_finite()).fold(f64::NEG_INFINITY, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lin(from: f64, to: f64, n: usize) -> Vec<f64> {
        (0..n).map(|i| from + (to - from) * (i as f64) / ((n - 1).max(1) as f64)).collect()
    }

    #[test]
    fn bullish_divergence_scores_strong() {
        // Price makes a lower low in H2, OBV makes a higher low.
        let price = vec![100.0, 98.0, 95.0, 92.0, 90.0, 93.0,  91.0, 89.0, 88.0, 92.0];
        let obv   = vec![  0.0, -5.0,-10.0,-15.0,-20.0,-10.0, -12.0,-11.0, -8.0, -5.0];
        // price_low2=88 < price_low1=90; obv_low2=-12 > obv_low1=-20 → strong div
        let cvd = obv.clone();
        let s = score_volume(20.0, &price, &obv, &cvd, 1000.0, 1000.0, true);
        // MFI(20) contributes ~12.8 pts; OBV+CVD strong div contribute 25+25.
        assert!(s.score > 55.0, "got {}", s.score);
        assert!(s.details.iter().any(|d| d.contains("bullish divergence")));
    }

    #[test]
    fn bearish_divergence_scores_strong() {
        let price = vec![100.0, 102.0, 104.0, 106.0, 108.0, 105.0, 108.0, 110.0, 112.0, 109.0];
        let obv   = vec![  0.0,   5.0,  10.0,  15.0,  20.0,  12.0,  14.0,  13.0,  10.0,   5.0];
        let cvd = obv.clone();
        let s = score_volume(80.0, &price, &obv, &cvd, 500.0, 1000.0, false);
        assert!(s.score > 55.0, "got {}", s.score);
        assert!(s.details.iter().any(|d| d.contains("bearish divergence")));
    }

    #[test]
    fn trend_only_gets_fallback_not_full_credit() {
        // Clean uptrend in both price and flow — no divergence, just fallback.
        let price = lin(90.0, 100.0, 20);
        let obv   = lin(0.0, 200.0, 20);
        let cvd = obv.clone();
        let s = score_volume(50.0, &price, &obv, &cvd, 1000.0, 1000.0, true);
        // MFI=50 → 0 pts; OBV+CVD fallback → 10+10 = 20. No divergence.
        assert!(s.score <= 25.0, "fallback should be modest, got {}", s.score);
        assert!(s.details.iter().all(|d| !d.contains("divergence")));
    }

    #[test]
    fn mfi_lower_reading_scores_higher_on_bottom_search() {
        let deep = score_volume(12.0, &[], &[], &[], 0.0, 1.0, true);
        let mild = score_volume(30.0, &[], &[], &[], 0.0, 1.0, true);
        assert!(deep.score > mild.score);
    }

    #[test]
    fn mfi_higher_reading_scores_higher_on_top_search() {
        let strong = score_volume(92.0, &[], &[], &[], 0.0, 1.0, false);
        let weak = score_volume(68.0, &[], &[], &[], 0.0, 1.0, false);
        assert!(strong.score > weak.score);
    }

    #[test]
    fn volume_spike_smooth_ramp() {
        let just_below = score_volume(50.0, &[], &[], &[], 1990.0, 1000.0, true);
        let just_above = score_volume(50.0, &[], &[], &[], 2010.0, 1000.0, true);
        assert!(just_above.score > just_below.score);
        let step = just_above.score - just_below.score;
        assert!(step < 1.5, "smooth ramp step={step}");
    }

    #[test]
    fn volume_spike_thresholds() {
        let at_lo = score_volume(50.0, &[], &[], &[], 1500.0, 1000.0, true);
        assert!((at_lo.score - 0.0).abs() < 1e-9);
        let at_hi = score_volume(50.0, &[], &[], &[], 3000.0, 1000.0, true);
        assert!((at_hi.score - 20.0).abs() < 1e-6);
    }
}

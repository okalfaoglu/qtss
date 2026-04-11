//! Volume Profile — distributes volume across price bins for a given
//! bar window. Used by the Wyckoff structure tracker to determine POC,
//! Value Area, HVN and LVN levels.
//!
//! Inputs are simple OHLCV tuples (no Decimal dependency). The caller
//! (e.g. the Wyckoff orchestrator) converts domain types before calling.

use serde::{Deserialize, Serialize};

/// Single row of OHLCV input.
#[derive(Debug, Clone, Copy)]
pub struct OhlcvInput {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// Result of a Volume Profile computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeProfile {
    /// Price of the bin with the highest volume (Point of Control).
    pub poc: f64,
    /// Upper bound of the Value Area (70% of total volume).
    pub va_high: f64,
    /// Lower bound of the Value Area.
    pub va_low: f64,
    /// High Volume Nodes — price bins with volume > 1.5× average.
    pub hvn: Vec<f64>,
    /// Low Volume Nodes — price bins with volume < 0.5× average.
    pub lvn: Vec<f64>,
    /// Raw bins: (price_center, volume).
    pub bins: Vec<(f64, f64)>,
}

/// Compute a volume profile over a slice of OHLCV bars.
///
/// `num_bins` controls granularity (default 50). Each bar's volume is
/// distributed evenly across all bins its high–low range covers
/// (TPO-style approximation).
pub fn volume_profile(bars: &[OhlcvInput], num_bins: usize) -> Option<VolumeProfile> {
    if bars.is_empty() || num_bins == 0 {
        return None;
    }
    let num_bins = num_bins.max(10).min(500);

    // Find global high/low
    let mut g_hi = f64::MIN;
    let mut g_lo = f64::MAX;
    for b in bars {
        if b.high > g_hi { g_hi = b.high; }
        if b.low < g_lo { g_lo = b.low; }
    }
    if g_hi <= g_lo {
        return None;
    }
    let bin_size = (g_hi - g_lo) / num_bins as f64;
    if bin_size <= 0.0 {
        return None;
    }

    let mut vols = vec![0.0_f64; num_bins];

    // Distribute each bar's volume across the bins it touches
    for b in bars {
        let lo_bin = ((b.low - g_lo) / bin_size).floor() as usize;
        let hi_bin = ((b.high - g_lo) / bin_size).floor() as usize;
        let lo_bin = lo_bin.min(num_bins - 1);
        let hi_bin = hi_bin.min(num_bins - 1);
        let span = (hi_bin - lo_bin + 1) as f64;
        let vol_per_bin = b.volume / span.max(1.0);
        for idx in lo_bin..=hi_bin {
            vols[idx] += vol_per_bin;
        }
    }

    // POC
    let (poc_idx, _) = vols
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())?;
    let poc = g_lo + (poc_idx as f64 + 0.5) * bin_size;

    // Value Area (70% of total volume, expanding from POC)
    let total_vol: f64 = vols.iter().sum();
    let va_target = total_vol * 0.7;
    let mut va_lo_idx = poc_idx;
    let mut va_hi_idx = poc_idx;
    let mut va_vol = vols[poc_idx];
    while va_vol < va_target {
        let expand_down = if va_lo_idx > 0 { vols[va_lo_idx - 1] } else { 0.0 };
        let expand_up = if va_hi_idx < num_bins - 1 { vols[va_hi_idx + 1] } else { 0.0 };
        if expand_down >= expand_up && va_lo_idx > 0 {
            va_lo_idx -= 1;
            va_vol += vols[va_lo_idx];
        } else if va_hi_idx < num_bins - 1 {
            va_hi_idx += 1;
            va_vol += vols[va_hi_idx];
        } else {
            break;
        }
    }
    let va_low = g_lo + va_lo_idx as f64 * bin_size;
    let va_high = g_lo + (va_hi_idx + 1) as f64 * bin_size;

    // HVN / LVN
    let avg_vol = total_vol / num_bins as f64;
    let mut hvn = Vec::new();
    let mut lvn = Vec::new();
    for (i, &v) in vols.iter().enumerate() {
        let center = g_lo + (i as f64 + 0.5) * bin_size;
        if v > avg_vol * 1.5 {
            hvn.push(center);
        } else if v < avg_vol * 0.5 && v > 0.0 {
            lvn.push(center);
        }
    }

    let bins: Vec<(f64, f64)> = vols
        .iter()
        .enumerate()
        .map(|(i, &v)| (g_lo + (i as f64 + 0.5) * bin_size, v))
        .collect();

    Some(VolumeProfile {
        poc,
        va_high,
        va_low,
        hvn,
        lvn,
        bins,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_profile() {
        let bars = vec![
            OhlcvInput { open: 100.0, high: 110.0, low: 95.0, close: 105.0, volume: 1000.0 },
            OhlcvInput { open: 105.0, high: 112.0, low: 100.0, close: 108.0, volume: 1500.0 },
            OhlcvInput { open: 108.0, high: 115.0, low: 102.0, close: 110.0, volume: 800.0 },
        ];
        let vp = volume_profile(&bars, 20).unwrap();
        assert!(vp.poc > 95.0 && vp.poc < 115.0);
        assert!(vp.va_low <= vp.poc);
        assert!(vp.va_high >= vp.poc);
        assert!(!vp.bins.is_empty());
    }

    #[test]
    fn empty_input() {
        assert!(volume_profile(&[], 20).is_none());
    }
}

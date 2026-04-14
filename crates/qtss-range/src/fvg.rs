//! Fair Value Gap (FVG) detector.
//!
//! An FVG is a three-candle imbalance zone where the wick of candle 1 and
//! candle 3 do not overlap, leaving an unfilled price gap on candle 2.
//!
//! - **Bullish FVG**: candle_1.high < candle_3.low  (gap up)
//! - **Bearish FVG**: candle_1.low  > candle_3.high (gap down)
//!
//! The gap zone is between the non-overlapping wicks. FVGs act as
//! magnets — price tends to revisit and fill them (mean-reversion) or
//! they serve as continuation zones (momentum).

use serde::{Deserialize, Serialize};

use crate::OhlcBar;
use crate::helpers::avg_volume;

// ── Config ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FvgConfig {
    /// Minimum gap size as fraction of ATR. Gaps smaller than this are noise.
    #[serde(default = "default_min_gap_atr_frac")]
    pub min_gap_atr_frac: f64,
    /// How many recent bars to scan for FVGs.
    #[serde(default = "default_scan_lookback")]
    pub scan_lookback: usize,
    /// If true, only report unfilled (still open) FVGs.
    #[serde(default = "default_unfilled_only")]
    pub unfilled_only: bool,
    /// Volume spike multiplier: candle_2 volume >= avg * mult for high-quality FVG.
    #[serde(default = "default_volume_spike_mult")]
    pub volume_spike_mult: f64,
    /// Volume lookback for averaging.
    #[serde(default = "default_volume_lookback")]
    pub volume_lookback: usize,
}

// P18 — bumped from 0.3 (too noisy; accepted micro-gaps as FVGs on
// choppy 4h BTC). 0.5 × ATR is a meaningful imbalance.
fn default_min_gap_atr_frac() -> f64 { 0.5 }
fn default_scan_lookback() -> usize { 50 }
fn default_unfilled_only() -> bool { true }
fn default_volume_spike_mult() -> f64 { 1.2 }
fn default_volume_lookback() -> usize { 20 }

impl Default for FvgConfig {
    fn default() -> Self {
        Self {
            min_gap_atr_frac: default_min_gap_atr_frac(),
            scan_lookback: default_scan_lookback(),
            unfilled_only: default_unfilled_only(),
            volume_spike_mult: default_volume_spike_mult(),
            volume_lookback: default_volume_lookback(),
        }
    }
}

// ── Output ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FvgMatch {
    /// "bullish_fvg" or "bearish_fvg"
    pub subkind: String,
    /// Bar index of the middle candle (candle 2).
    pub bar_index: i64,
    /// Upper edge of the gap zone.
    pub gap_high: f64,
    /// Lower edge of the gap zone.
    pub gap_low: f64,
    /// Gap size in price.
    pub gap_size: f64,
    /// Gap size / ATR ratio.
    pub gap_atr_ratio: f64,
    /// Whether the gap has been filled by subsequent price action.
    pub filled: bool,
    /// Fraction of gap filled (0.0 = untouched, 1.0 = fully filled).
    pub fill_pct: f64,
    /// Quality score 0.0–1.0 based on gap size + volume.
    pub score: f64,
    /// Whether candle 2 had above-average volume.
    pub volume_confirmed: bool,
}

// ── Detection ───────────────────────────────────────────────────────

/// Scan bars for Fair Value Gaps and return all matches.
pub fn detect_fvg(bars: &[OhlcBar], atr_value: f64, cfg: &FvgConfig) -> Vec<FvgMatch> {
    let n = bars.len();
    if n < 4 {
        return Vec::new();
    }

    let scan_start = n.saturating_sub(cfg.scan_lookback).max(2);
    let mut results = Vec::new();

    for i in scan_start..(n - 1) {
        // candle_1 = bars[i-2], candle_2 = bars[i-1], candle_3 = bars[i]
        // But we need i >= 2
        if i < 2 {
            continue;
        }
        let c1 = &bars[i - 2];
        let c2 = &bars[i - 1];
        let c3 = &bars[i];

        // Bullish FVG: c1.high < c3.low → gap between c1.high and c3.low
        let bull_gap = c3.low - c1.high;
        // Bearish FVG: c1.low > c3.high → gap between c3.high and c1.low
        let bear_gap = c1.low - c3.high;

        let min_gap = atr_value * cfg.min_gap_atr_frac;

        let (subkind, gap_high, gap_low, gap_size) = if bull_gap > min_gap {
            ("bullish_fvg", c3.low, c1.high, bull_gap)
        } else if bear_gap > min_gap {
            ("bearish_fvg", c1.low, c3.high, bear_gap)
        } else {
            continue;
        };

        let gap_atr_ratio = if atr_value > 1e-12 { gap_size / atr_value } else { 0.0 };

        // Check fill status: any bar after c3 that enters the gap
        let (filled, fill_pct) = check_fill(bars, i + 1, gap_high, gap_low, subkind);

        if cfg.unfilled_only && filled {
            continue;
        }

        // Volume confirmation
        let vol_avg = avg_volume(bars, i - 1, cfg.volume_lookback);
        let volume_confirmed = match (c2.volume, vol_avg) {
            (Some(v), Some(avg)) if avg > 1e-12 => v >= avg * cfg.volume_spike_mult,
            _ => false,
        };

        // Score: gap_atr_ratio contributes 60%, volume 20%, unfilled 20%
        let gap_score = (gap_atr_ratio / 2.0).clamp(0.0, 0.6);
        let vol_score = if volume_confirmed { 0.2 } else { 0.05 };
        let fill_score = if !filled { 0.2 } else { 0.2 * (1.0 - fill_pct) };
        let score = (gap_score + vol_score + fill_score).clamp(0.0, 1.0);

        results.push(FvgMatch {
            subkind: subkind.to_string(),
            bar_index: c2.bar_index,
            gap_high,
            gap_low,
            gap_size,
            gap_atr_ratio,
            filled,
            fill_pct,
            score,
            volume_confirmed,
        });
    }

    results
}

fn check_fill(bars: &[OhlcBar], from: usize, gap_high: f64, gap_low: f64, subkind: &str) -> (bool, f64) {
    let gap_size = gap_high - gap_low;
    if gap_size <= 1e-12 {
        return (true, 1.0);
    }

    let mut max_penetration = 0.0_f64;

    for b in bars.iter().skip(from) {
        let pen = match subkind {
            "bullish_fvg" => {
                // Fill = price drops into gap from above. gap is between gap_low..gap_high
                if b.low < gap_high {
                    (gap_high - b.low.max(gap_low)) / gap_size
                } else {
                    0.0
                }
            }
            "bearish_fvg" => {
                // Fill = price rises into gap from below.
                if b.high > gap_low {
                    (b.high.min(gap_high) - gap_low) / gap_size
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        max_penetration = max_penetration.max(pen);
        if max_penetration >= 1.0 {
            return (true, 1.0);
        }
    }

    (max_penetration >= 1.0, max_penetration.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(i: i64, o: f64, h: f64, l: f64, c: f64) -> OhlcBar {
        OhlcBar { open: o, high: h, low: l, close: c, bar_index: i, volume: Some(1000.0) }
    }

    #[test]
    fn detects_bullish_fvg() {
        // c1: high=100, c2: big up candle, c3: low=102 → gap 100..102
        let bars = vec![
            bar(0, 98.0, 100.0, 97.0, 99.0),  // c1
            bar(1, 100.0, 105.0, 99.5, 104.0), // c2 (impulse)
            bar(2, 104.0, 106.0, 102.0, 105.0),// c3 low=102 > c1 high=100 → gap
            bar(3, 105.0, 107.0, 104.0, 106.0),// no fill
        ];
        let fvgs = detect_fvg(&bars, 2.0, &FvgConfig { min_gap_atr_frac: 0.1, unfilled_only: false, scan_lookback: 50, ..Default::default() });
        assert_eq!(fvgs.len(), 1);
        assert_eq!(fvgs[0].subkind, "bullish_fvg");
        assert!((fvgs[0].gap_low - 100.0).abs() < 0.01);
        assert!((fvgs[0].gap_high - 102.0).abs() < 0.01);
    }

    #[test]
    fn detects_bearish_fvg() {
        let bars = vec![
            bar(0, 105.0, 106.0, 103.0, 104.0), // c1 low=103
            bar(1, 103.0, 103.5, 98.0, 99.0),    // c2 (sell-off)
            bar(2, 99.0, 100.0, 97.0, 98.0),     // c3 high=100 < c1 low=103 → gap
            bar(3, 98.0, 99.0, 96.0, 97.0),
        ];
        let fvgs = detect_fvg(&bars, 2.0, &FvgConfig { min_gap_atr_frac: 0.1, unfilled_only: false, scan_lookback: 50, ..Default::default() });
        assert_eq!(fvgs.len(), 1);
        assert_eq!(fvgs[0].subkind, "bearish_fvg");
    }
}

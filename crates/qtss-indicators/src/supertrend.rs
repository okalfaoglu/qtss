//! SuperTrend — ATR-banded trend follower.
//!
//! Classic Olivier Seban / ta.supertrend port. Produces one upper-band
//! and one lower-band series, plus a `trend` series (-1 = bear, +1 =
//! bull). The line a chart draws is `trend == 1 ? lower : upper`.
//!
//! Uses `crate::volatility::atr` for ATR; factor is usually 3.0.

use crate::volatility::atr;

#[derive(Debug, Clone)]
pub struct SuperTrendResult {
    pub upper: Vec<f64>,
    pub lower: Vec<f64>,
    pub trend: Vec<i8>, // +1 bull, -1 bear, 0 warmup
}

pub fn supertrend(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    period: usize,
    factor: f64,
) -> SuperTrendResult {
    let n = closes.len();
    let atr_series = atr(highs, lows, closes, period);
    let mut upper = vec![f64::NAN; n];
    let mut lower = vec![f64::NAN; n];
    let mut trend = vec![0i8; n];
    for i in 0..n {
        if atr_series[i].is_nan() {
            continue;
        }
        let mid = (highs[i] + lows[i]) / 2.0;
        let mut up = mid + factor * atr_series[i];
        let mut lo = mid - factor * atr_series[i];
        if i > 0 && !upper[i - 1].is_nan() {
            // Pine-style "firm" bands: tighten only, never widen back.
            if up > upper[i - 1] && closes[i - 1] <= upper[i - 1] {
                up = upper[i - 1];
            }
            if lo < lower[i - 1] && closes[i - 1] >= lower[i - 1] {
                lo = lower[i - 1];
            }
        }
        upper[i] = up;
        lower[i] = lo;
        // Trend flip rules.
        let prev = if i == 0 { 0 } else { trend[i - 1] };
        trend[i] = match prev {
            1 if closes[i] < lo => -1,
            -1 if closes[i] > up => 1,
            0 => {
                if closes[i] > up {
                    1
                } else if closes[i] < lo {
                    -1
                } else {
                    0
                }
            }
            other => other,
        };
    }
    SuperTrendResult {
        upper,
        lower,
        trend,
    }
}

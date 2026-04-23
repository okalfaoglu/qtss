//! Keltner channel — EMA midline ± (ATR × multiplier) bands.
//!
//! Chester Keltner's volatility channel. Pairs with Bollinger for the
//! TTM Squeeze: when BB sits entirely inside Keltner, volatility is
//! compressed and a breakout is near.

use crate::ema::ema;
use crate::volatility::atr;

#[derive(Debug, Clone)]
pub struct Keltner {
    pub upper: Vec<f64>,
    pub mid: Vec<f64>,
    pub lower: Vec<f64>,
}

pub fn keltner(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    ema_period: usize,
    atr_period: usize,
    mult: f64,
) -> Keltner {
    let n = closes.len();
    let mid = ema(closes, ema_period);
    let atr_series = atr(highs, lows, closes, atr_period);
    let mut upper = vec![f64::NAN; n];
    let mut lower = vec![f64::NAN; n];
    for i in 0..n {
        if !mid[i].is_nan() && !atr_series[i].is_nan() {
            upper[i] = mid[i] + mult * atr_series[i];
            lower[i] = mid[i] - mult * atr_series[i];
        }
    }
    Keltner { upper, mid, lower }
}

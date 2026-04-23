//! TTM Squeeze — John Carter's volatility-compression gate.
//!
//! Fires when the Bollinger Band sits inside the Keltner Channel
//! (standard deviations compressed below ATR envelope). Output is a
//! boolean per bar: `true` = squeeze on (compression), `false` = off
//! (normal / expanding). The classic trade is to enter on the first
//! `true → false` flip in the direction of a separate momentum
//! oscillator.

use crate::bollinger::bollinger;
use crate::keltner::keltner;

pub fn ttm_squeeze(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    bb_period: usize,
    bb_stdev: f64,
    kc_period: usize,
    kc_atr_period: usize,
    kc_mult: f64,
) -> Vec<bool> {
    let bb = bollinger(closes, bb_period, bb_stdev);
    let kc = keltner(highs, lows, closes, kc_period, kc_atr_period, kc_mult);
    let n = closes.len();
    let mut out = vec![false; n];
    for i in 0..n {
        if bb.upper[i].is_nan() || kc.upper[i].is_nan() {
            continue;
        }
        out[i] = bb.upper[i] < kc.upper[i] && bb.lower[i] > kc.lower[i];
    }
    out
}

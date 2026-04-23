//! Williams %R — inverse Stochastic, scaled to [-100, 0].
//!
//! `%R = -100 * (highest_high - close) / (highest_high - lowest_low)`
//! over the last N bars. Reads like Stoch flipped upside-down; the
//! same overbought (> -20) / oversold (< -80) zones apply.

pub fn williams_r(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<f64> {
    let n = closes.len().min(highs.len()).min(lows.len());
    let mut out = vec![f64::NAN; n];
    if period == 0 || n < period {
        return out;
    }
    for i in (period - 1)..n {
        let mut hh = f64::NEG_INFINITY;
        let mut ll = f64::INFINITY;
        for j in (i + 1 - period)..=i {
            if highs[j] > hh {
                hh = highs[j];
            }
            if lows[j] < ll {
                ll = lows[j];
            }
        }
        let range = hh - ll;
        out[i] = if range > 0.0 {
            -100.0 * (hh - closes[i]) / range
        } else {
            0.0
        };
    }
    out
}

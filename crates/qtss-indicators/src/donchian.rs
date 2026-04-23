//! Donchian channel — rolling N-period high/low + midline.
//!
//! Foundation of the Turtle breakout system: long entry when price
//! closes above the upper channel, short when it closes below the lower.

#[derive(Debug, Clone)]
pub struct Donchian {
    pub upper: Vec<f64>,
    pub lower: Vec<f64>,
    pub mid: Vec<f64>,
}

pub fn donchian(highs: &[f64], lows: &[f64], period: usize) -> Donchian {
    let n = highs.len().min(lows.len());
    let mut upper = vec![f64::NAN; n];
    let mut lower = vec![f64::NAN; n];
    let mut mid = vec![f64::NAN; n];
    if period == 0 || n < period {
        return Donchian { upper, lower, mid };
    }
    for i in (period - 1)..n {
        let mut hi = f64::NEG_INFINITY;
        let mut lo = f64::INFINITY;
        for j in (i + 1 - period)..=i {
            if highs[j] > hi {
                hi = highs[j];
            }
            if lows[j] < lo {
                lo = lows[j];
            }
        }
        upper[i] = hi;
        lower[i] = lo;
        mid[i] = (hi + lo) / 2.0;
    }
    Donchian { upper, lower, mid }
}

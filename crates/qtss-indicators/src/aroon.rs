//! Aroon oscillator — identifies trend starts/ends via pivot recency.
//!
//! `AroonUp   = 100 * (period - bars_since_highest_high) / period`
//! `AroonDown = 100 * (period - bars_since_lowest_low)  / period`
//! `AroonOsc  = AroonUp - AroonDown` (range -100..100).

#[derive(Debug, Clone)]
pub struct AroonResult {
    pub up: Vec<f64>,
    pub down: Vec<f64>,
    pub osc: Vec<f64>,
}

pub fn aroon(highs: &[f64], lows: &[f64], period: usize) -> AroonResult {
    let n = highs.len().min(lows.len());
    let mut up = vec![f64::NAN; n];
    let mut down = vec![f64::NAN; n];
    let mut osc = vec![f64::NAN; n];
    if period == 0 || n <= period {
        return AroonResult { up, down, osc };
    }
    let p = period as f64;
    for i in period..n {
        let mut hi_idx = i - period;
        let mut lo_idx = i - period;
        for j in (i - period)..=i {
            if highs[j] >= highs[hi_idx] {
                hi_idx = j;
            }
            if lows[j] <= lows[lo_idx] {
                lo_idx = j;
            }
        }
        let since_hi = (i - hi_idx) as f64;
        let since_lo = (i - lo_idx) as f64;
        up[i] = 100.0 * (p - since_hi) / p;
        down[i] = 100.0 * (p - since_lo) / p;
        osc[i] = up[i] - down[i];
    }
    AroonResult { up, down, osc }
}

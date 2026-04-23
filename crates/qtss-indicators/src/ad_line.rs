//! Accumulation / Distribution Line — Larry Williams (pre-CMF) cumulative
//! volume flow.
//!
//! `MFM = ((close-low) - (high-close)) / (high-low)`
//! `MFV = MFM * volume`
//! `AD  = running_sum(MFV)`
//!
//! Unlike CMF it is unbounded; divergences vs price are the usual read.

pub fn ad_line(highs: &[f64], lows: &[f64], closes: &[f64], volumes: &[f64]) -> Vec<f64> {
    let n = closes.len().min(volumes.len());
    let mut out = vec![0.0f64; n];
    let mut cum = 0.0;
    for i in 0..n {
        let range = highs[i] - lows[i];
        let mfv = if range > 0.0 {
            volumes[i] * (((closes[i] - lows[i]) - (highs[i] - closes[i])) / range)
        } else {
            0.0
        };
        cum += mfv;
        out[i] = cum;
    }
    out
}

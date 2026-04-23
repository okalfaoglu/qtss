//! Chaikin Money Flow — volume-weighted oscillator in [-1, 1].
//!
//! CMF = sum(MFV, N) / sum(volume, N), where
//! MFV = volume * ((close-low) - (high-close)) / (high-low)

pub fn cmf(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    volumes: &[f64],
    period: usize,
) -> Vec<f64> {
    let n = closes.len().min(volumes.len());
    let mut out = vec![f64::NAN; n];
    if period == 0 || n < period {
        return out;
    }
    let mut mfv = vec![0.0; n];
    for i in 0..n {
        let range = highs[i] - lows[i];
        mfv[i] = if range > 0.0 {
            volumes[i] * (((closes[i] - lows[i]) - (highs[i] - closes[i])) / range)
        } else {
            0.0
        };
    }
    for i in (period - 1)..n {
        let mut mfv_sum = 0.0;
        let mut vol_sum = 0.0;
        for j in (i + 1 - period)..=i {
            mfv_sum += mfv[j];
            vol_sum += volumes[j];
        }
        out[i] = if vol_sum > 0.0 { mfv_sum / vol_sum } else { 0.0 };
    }
    out
}

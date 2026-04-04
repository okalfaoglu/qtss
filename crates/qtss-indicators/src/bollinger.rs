//! Bollinger Bands, %B, BandWidth, Squeeze detection.

use crate::ema::sma;

#[derive(Debug, Clone)]
pub struct BollingerResult {
    pub upper: Vec<f64>,
    pub middle: Vec<f64>,
    pub lower: Vec<f64>,
    /// %B = (price − lower) / (upper − lower).
    pub percent_b: Vec<f64>,
    /// BandWidth = (upper − lower) / middle.
    pub bandwidth: Vec<f64>,
}

/// Bollinger Bands hesaplar.  `mult` genelde 2.0.
#[must_use]
pub fn bollinger(values: &[f64], period: usize, mult: f64) -> BollingerResult {
    let n = values.len();
    let mid = sma(values, period);
    let mut upper = vec![f64::NAN; n];
    let mut lower = vec![f64::NAN; n];
    let mut percent_b = vec![f64::NAN; n];
    let mut bandwidth = vec![f64::NAN; n];

    for i in (period.saturating_sub(1))..n {
        if mid[i].is_nan() {
            continue;
        }
        let slice = &values[i + 1 - period..=i];
        let mean = mid[i];
        let variance: f64 = slice.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / period as f64;
        let sd = variance.sqrt();
        upper[i] = mean + mult * sd;
        lower[i] = mean - mult * sd;
        let bw = upper[i] - lower[i];
        if bw.abs() > 1e-15 {
            percent_b[i] = (values[i] - lower[i]) / bw;
            bandwidth[i] = bw / mean;
        }
    }

    BollingerResult { upper, middle: mid, lower, percent_b, bandwidth }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bollinger_basic() {
        let data: Vec<f64> = (1..=20).map(|x| x as f64).collect();
        let r = bollinger(&data, 5, 2.0);
        assert!(r.upper[3].is_nan());
        assert!(!r.upper[4].is_nan());
        // Middle band = SMA
        assert!((r.middle[4] - 3.0).abs() < 1e-9);
        assert!(r.upper[4] > r.middle[4]);
        assert!(r.lower[4] < r.middle[4]);
    }
}

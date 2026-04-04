//! Stochastic Oscillator (%K, %D).

use crate::ema::sma;

#[derive(Debug, Clone)]
pub struct StochasticResult {
    pub k: Vec<f64>,
    pub d: Vec<f64>,
}

/// Stochastic Oscillator hesaplar.
/// `k_period` = look-back (genelde 14), `d_period` = smoothing (genelde 3).
/// `highs`, `lows`, `closes` aynı uzunlukta olmalı.
#[must_use]
pub fn stochastic(highs: &[f64], lows: &[f64], closes: &[f64], k_period: usize, d_period: usize) -> StochasticResult {
    let n = closes.len();
    let mut k = vec![f64::NAN; n];
    if k_period == 0 || n < k_period {
        return StochasticResult { k: k.clone(), d: k };
    }
    for i in (k_period - 1)..n {
        let hh = highs[i + 1 - k_period..=i].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let ll = lows[i + 1 - k_period..=i].iter().cloned().fold(f64::INFINITY, f64::min);
        let range = hh - ll;
        k[i] = if range.abs() < 1e-15 { 50.0 } else { (closes[i] - ll) / range * 100.0 };
    }
    let d = sma(&k.iter().map(|v| if v.is_nan() { f64::NAN } else { *v }).collect::<Vec<_>>(), d_period);
    // SMA of k skips NaN values naturally since our sma treats NaN inputs.
    // Actually our sma sums NaN → NaN, which is fine: first k_period+d_period-2 are NaN.
    StochasticResult { k, d }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stoch_basic() {
        let h: Vec<f64> = (1..=20).map(|x| x as f64 + 1.0).collect();
        let l: Vec<f64> = (1..=20).map(|x| x as f64 - 1.0).collect();
        let c: Vec<f64> = (1..=20).map(|x| x as f64).collect();
        let r = stochastic(&h, &l, &c, 5, 3);
        assert_eq!(r.k.len(), 20);
        assert!(r.k[3].is_nan());
        assert!(!r.k[4].is_nan());
    }
}

//! MACD (Moving Average Convergence Divergence).

use crate::ema::ema;

/// MACD hesaplama sonucu.
#[derive(Debug, Clone)]
pub struct MacdResult {
    /// MACD line = EMA(fast) − EMA(slow).  İlk `slow-1` eleman `NaN`.
    pub macd_line: Vec<f64>,
    /// Signal line = EMA(signal_period) of macd_line.
    pub signal_line: Vec<f64>,
    /// Histogram = macd_line − signal_line.
    pub histogram: Vec<f64>,
}

/// Klasik MACD hesaplar.  Varsayılan (12, 26, 9).
#[must_use]
pub fn macd(values: &[f64], fast: usize, slow: usize, signal_period: usize) -> MacdResult {
    let n = values.len();
    let ema_fast = ema(values, fast);
    let ema_slow = ema(values, slow);

    let mut macd_line = vec![f64::NAN; n];
    for i in 0..n {
        if ema_fast[i].is_nan() || ema_slow[i].is_nan() {
            continue;
        }
        macd_line[i] = ema_fast[i] - ema_slow[i];
    }

    // Signal line: MACD line'ın geçerli (non-NaN) değerlerinden EMA al.
    let valid_start = slow.saturating_sub(1);
    let valid_vals: Vec<f64> = macd_line[valid_start..].iter().copied().collect();
    let sig_raw = ema(&valid_vals, signal_period);

    let mut signal_line = vec![f64::NAN; n];
    for (j, &v) in sig_raw.iter().enumerate() {
        signal_line[valid_start + j] = v;
    }

    let mut histogram = vec![f64::NAN; n];
    for i in 0..n {
        if !macd_line[i].is_nan() && !signal_line[i].is_nan() {
            histogram[i] = macd_line[i] - signal_line[i];
        }
    }

    MacdResult { macd_line, signal_line, histogram }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macd_basic() {
        let data: Vec<f64> = (1..=50).map(|x| x as f64).collect();
        let r = macd(&data, 12, 26, 9);
        assert_eq!(r.macd_line.len(), 50);
        // İlk 25 eleman NaN olmalı (slow-1 = 25)
        assert!(r.macd_line[24].is_nan());
        assert!(!r.macd_line[25].is_nan());
    }
}

//! ATR, Bollinger squeeze, volatility compression detection.

use crate::ema::sma;

/// Average True Range hesaplar. İlk `period` eleman `NaN`.
#[must_use]
pub fn atr(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<f64> {
    let n = closes.len();
    let mut tr = vec![0.0_f64; n];
    if n == 0 {
        return vec![];
    }
    tr[0] = highs[0] - lows[0];
    for i in 1..n {
        let hl = highs[i] - lows[i];
        let hc = (highs[i] - closes[i - 1]).abs();
        let lc = (lows[i] - closes[i - 1]).abs();
        tr[i] = hl.max(hc).max(lc);
    }
    sma(&tr, period)
}

/// Bollinger Bands squeeze detection.
/// BandWidth < threshold ise squeeze.  Varsayılan threshold ~0.03 (piyasaya göre ayarlanır).
#[must_use]
pub fn bb_squeeze(bandwidths: &[f64], threshold: f64) -> Vec<bool> {
    bandwidths.iter().map(|&bw| !bw.is_nan() && bw < threshold).collect()
}

/// Volatilite sıkışma dedektörü: ATR'nin SMA'sı düşüyorsa sıkışma var.
/// Son `lookback` bar boyunca ATR SMA eğimi negatifse `true`.
#[must_use]
pub fn compression_detector(atr_values: &[f64], lookback: usize) -> Vec<bool> {
    let n = atr_values.len();
    let mut out = vec![false; n];
    if lookback < 2 || n < lookback {
        return out;
    }
    for i in (lookback - 1)..n {
        let start_val = atr_values[i + 1 - lookback];
        let end_val = atr_values[i];
        if !start_val.is_nan() && !end_val.is_nan() && end_val < start_val {
            out[i] = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atr_basic() {
        let h = vec![12.0, 13.0, 14.0, 13.0, 15.0];
        let l = vec![10.0, 11.0, 12.0, 11.0, 13.0];
        let c = vec![11.0, 12.0, 13.0, 12.0, 14.0];
        let r = atr(&h, &l, &c, 3);
        assert!(r[1].is_nan());
        assert!(!r[2].is_nan());
    }

    #[test]
    fn squeeze_basic() {
        let bw = vec![0.05, 0.02, 0.01, 0.04];
        let s = bb_squeeze(&bw, 0.03);
        assert!(!s[0]);
        assert!(s[1]);
        assert!(s[2]);
        assert!(!s[3]);
    }
}

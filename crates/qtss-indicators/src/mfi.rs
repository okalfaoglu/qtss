//! Money Flow Index (volume-weighted RSI).

/// MFI hesaplar.  `typical = (H+L+C)/3`, money flow = typical × volume.
/// Döndürülen vektör girdi ile aynı uzunlukta; ilk `period` eleman `NaN`.
#[must_use]
pub fn mfi(highs: &[f64], lows: &[f64], closes: &[f64], volumes: &[f64], period: usize) -> Vec<f64> {
    let n = closes.len();
    let mut out = vec![f64::NAN; n];
    if period == 0 || n <= period {
        return out;
    }
    let tp: Vec<f64> = (0..n).map(|i| (highs[i] + lows[i] + closes[i]) / 3.0).collect();

    for i in period..n {
        let mut pos = 0.0_f64;
        let mut neg = 0.0_f64;
        for j in (i + 1 - period)..=i {
            let mf = tp[j] * volumes[j];
            if j > 0 && tp[j] > tp[j - 1] {
                pos += mf;
            } else if j > 0 && tp[j] < tp[j - 1] {
                neg += mf;
            }
        }
        out[i] = if neg.abs() < 1e-15 { 100.0 } else { 100.0 - 100.0 / (1.0 + pos / neg) };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mfi_basic() {
        let h = vec![10.0, 11.0, 12.0, 11.0, 13.0, 14.0];
        let l = vec![9.0, 10.0, 11.0, 10.0, 12.0, 13.0];
        let c = vec![9.5, 10.5, 11.5, 10.5, 12.5, 13.5];
        let v = vec![100.0, 150.0, 200.0, 180.0, 220.0, 250.0];
        let r = mfi(&h, &l, &c, &v, 3);
        assert!(r[2].is_nan());
        assert!(!r[3].is_nan());
        assert!(r[3] >= 0.0 && r[3] <= 100.0);
    }
}

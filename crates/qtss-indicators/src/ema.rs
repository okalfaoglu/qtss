//! Exponential Moving Average — MACD, signal line ve diğer EMA tabanlı hesaplamaların temeli.

/// EMA serisini hesaplar. İlk değer SMA ile başlar (Wilder yöntemi).
/// Döndürülen vektör girdi ile aynı uzunlukta; ilk `period-1` eleman `NaN`.
#[must_use]
pub fn ema(values: &[f64], period: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if period == 0 || n < period {
        return out;
    }
    let k = 2.0 / (period as f64 + 1.0);
    // İlk EMA = SMA(period)
    let sma: f64 = values[..period].iter().sum::<f64>() / period as f64;
    out[period - 1] = sma;
    for i in period..n {
        out[i] = values[i] * k + out[i - 1] * (1.0 - k);
    }
    out
}

/// Tek değerli EMA güncelleme (streaming için).
#[inline]
pub fn ema_step(prev_ema: f64, value: f64, period: usize) -> f64 {
    let k = 2.0 / (period as f64 + 1.0);
    value * k + prev_ema * (1.0 - k)
}

/// SMA serisini hesaplar.
#[must_use]
pub fn sma(values: &[f64], period: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if period == 0 || n < period {
        return out;
    }
    let mut sum: f64 = values[..period].iter().sum();
    out[period - 1] = sum / period as f64;
    for i in period..n {
        sum += values[i] - values[i - period];
        out[i] = sum / period as f64;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ema_basic() {
        let data: Vec<f64> = (1..=10).map(|x| x as f64).collect();
        let e = ema(&data, 3);
        assert!(e[0].is_nan());
        assert!(e[1].is_nan());
        assert!((e[2] - 2.0).abs() < 1e-9); // SMA(3) of [1,2,3] = 2.0
        // EMA(3) k=0.5: e[3] = 4*0.5 + 2*0.5 = 3.0
        assert!((e[3] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn sma_basic() {
        let data = vec![2.0, 4.0, 6.0, 8.0, 10.0];
        let s = sma(&data, 3);
        assert!(s[0].is_nan());
        assert!((s[2] - 4.0).abs() < 1e-9);
        assert!((s[3] - 6.0).abs() < 1e-9);
        assert!((s[4] - 8.0).abs() < 1e-9);
    }

    #[test]
    fn empty_input() {
        assert!(ema(&[], 3).is_empty());
        assert!(sma(&[], 3).is_empty());
    }
}

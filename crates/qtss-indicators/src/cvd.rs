//! Cumulative Volume Delta.
//! Bar-level tahmini delta: close barın üst yarısındaysa alıcı ağırlıklı.

/// CVD hesaplar.  delta[i] = volume[i] × ((close-low)-(high-close)) / (high-low).
/// Eğer high==low ise delta = 0.
#[must_use]
pub fn cvd(highs: &[f64], lows: &[f64], closes: &[f64], volumes: &[f64]) -> Vec<f64> {
    let n = closes.len();
    if n == 0 {
        return vec![];
    }
    let mut out = vec![0.0; n];
    let mut cum = 0.0_f64;
    for i in 0..n {
        let range = highs[i] - lows[i];
        let delta = if range.abs() < 1e-15 {
            0.0
        } else {
            volumes[i] * ((closes[i] - lows[i]) - (highs[i] - closes[i])) / range
        };
        cum += delta;
        out[i] = cum;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cvd_basic() {
        // Close at high → full buy, close at low → full sell
        let h = vec![12.0, 12.0];
        let l = vec![10.0, 10.0];
        let c = vec![12.0, 10.0]; // first all buy, second all sell
        let v = vec![100.0, 100.0];
        let r = cvd(&h, &l, &c, &v);
        assert!((r[0] - 100.0).abs() < 1e-9);
        assert!((r[1] - 0.0).abs() < 1e-9);
    }
}

//! Wilder RSI — the canonical momentum oscillator.
//!
//! Uses Wilder's smoothing (ta.rma in Pine, `prev * (n-1)/n + curr/n`).
//! Returns an N-length Vec padded with NaN up to `period` so indices
//! line up with the input close series. Missing warm-up period → NaN.
//!
//! Default period 14 comes from config (`system_config.rsi.period`), not
//! a hard-coded constant (CLAUDE.md #2 — the caller decides).

pub fn rsi(closes: &[f64], period: usize) -> Vec<f64> {
    let n = closes.len();
    let mut out = vec![f64::NAN; n];
    if period == 0 || n <= period {
        return out;
    }
    let mut avg_gain = 0.0;
    let mut avg_loss = 0.0;
    for i in 1..=period {
        let d = closes[i] - closes[i - 1];
        if d >= 0.0 {
            avg_gain += d;
        } else {
            avg_loss -= d;
        }
    }
    avg_gain /= period as f64;
    avg_loss /= period as f64;
    out[period] = if avg_loss == 0.0 {
        100.0
    } else {
        100.0 - 100.0 / (1.0 + avg_gain / avg_loss)
    };
    let p = period as f64;
    for i in (period + 1)..n {
        let d = closes[i] - closes[i - 1];
        let (g, l) = if d >= 0.0 { (d, 0.0) } else { (0.0, -d) };
        avg_gain = (avg_gain * (p - 1.0) + g) / p;
        avg_loss = (avg_loss * (p - 1.0) + l) / p;
        out[i] = if avg_loss == 0.0 {
            100.0
        } else {
            100.0 - 100.0 / (1.0 + avg_gain / avg_loss)
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn warmup_is_nan() {
        let c = (1..=20).map(|x| x as f64).collect::<Vec<_>>();
        let r = rsi(&c, 14);
        assert!(r[0..14].iter().all(|x| x.is_nan()));
        assert!(!r[14].is_nan());
    }
    #[test]
    fn all_gains_saturates_at_100() {
        let c = (1..=30).map(|x| x as f64).collect::<Vec<_>>();
        let r = rsi(&c, 14);
        assert!((r[20] - 100.0).abs() < 0.01);
    }
}

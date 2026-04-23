//! Ichimoku Kinko Hyo — full 5-line system.
//!
//! tenkan   = midpoint of last 9 bars (H+L)/2
//! kijun    = midpoint of last 26 bars
//! senkou_a = (tenkan + kijun) / 2  shifted 26 forward (cloud top/bottom)
//! senkou_b = midpoint of last 52 bars shifted 26 forward
//! chikou   = close shifted 26 back
//!
//! Periods are all caller-supplied so non-crypto markets (forex uses
//! 9/26/52, BIST can retune) stay configurable (CLAUDE.md #2).

#[derive(Debug, Clone)]
pub struct Ichimoku {
    pub tenkan: Vec<f64>,
    pub kijun: Vec<f64>,
    pub senkou_a: Vec<f64>,
    pub senkou_b: Vec<f64>,
    pub chikou: Vec<f64>,
}

fn donchian_mid(highs: &[f64], lows: &[f64], n: usize, period: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; n];
    if period == 0 || n < period {
        return out;
    }
    for i in (period - 1)..n {
        let mut hi = f64::NEG_INFINITY;
        let mut lo = f64::INFINITY;
        for j in (i + 1 - period)..=i {
            if highs[j] > hi {
                hi = highs[j];
            }
            if lows[j] < lo {
                lo = lows[j];
            }
        }
        out[i] = (hi + lo) / 2.0;
    }
    out
}

pub fn ichimoku(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    tenkan_p: usize,
    kijun_p: usize,
    senkou_b_p: usize,
    shift: usize,
) -> Ichimoku {
    let n = closes.len();
    let tenkan = donchian_mid(highs, lows, n, tenkan_p);
    let kijun = donchian_mid(highs, lows, n, kijun_p);
    // Senkou A = (tenkan + kijun)/2 shifted forward by `shift`. Forward
    // shift = leave first `shift` NaN, then copy values shifted right.
    let mut senkou_a = vec![f64::NAN; n];
    for i in 0..n {
        if i >= shift && !tenkan[i - shift].is_nan() && !kijun[i - shift].is_nan() {
            senkou_a[i] = (tenkan[i - shift] + kijun[i - shift]) / 2.0;
        }
    }
    let sen_b_base = donchian_mid(highs, lows, n, senkou_b_p);
    let mut senkou_b = vec![f64::NAN; n];
    for i in 0..n {
        if i >= shift && !sen_b_base[i - shift].is_nan() {
            senkou_b[i] = sen_b_base[i - shift];
        }
    }
    // Chikou = close shifted back by `shift` (plotted `shift` bars back).
    let mut chikou = vec![f64::NAN; n];
    for i in 0..n {
        if i + shift < n {
            chikou[i] = closes[i + shift];
        }
    }
    Ichimoku {
        tenkan,
        kijun,
        senkou_a,
        senkou_b,
        chikou,
    }
}

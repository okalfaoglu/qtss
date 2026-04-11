//! Shared indicator helpers: ATR, RSI, pivot detection.

use crate::OhlcBar;

pub fn true_range(h: f64, l: f64, prev_close: f64) -> f64 {
    let a = h - l;
    let b = (h - prev_close).abs();
    let c = (l - prev_close).abs();
    a.max(b).max(c)
}

/// Wilder ATR; `out[i]` defined for `i >= period`.
pub fn wilder_atr(bars: &[OhlcBar], period: usize) -> Vec<f64> {
    let n = bars.len();
    let mut out = vec![f64::NAN; n];
    if n < period + 1 || period < 1 {
        return out;
    }
    let tr: Vec<f64> = (1..n)
        .map(|i| true_range(bars[i].high, bars[i].low, bars[i - 1].close))
        .collect();
    let mut sum = 0.0;
    for j in 0..period {
        sum += tr[j];
    }
    out[period] = sum / period as f64;
    for i in (period + 1)..n {
        let t = tr[i - 1];
        out[i] = (out[i - 1] * (period as f64 - 1.0) + t) / period as f64;
    }
    out
}

pub fn last_finite_sma(slice: &[f64], len: usize) -> Option<f64> {
    if len < 1 {
        return None;
    }
    let acc: Vec<f64> = slice.iter().copied().filter(|x| x.is_finite()).collect();
    if acc.len() < len {
        return None;
    }
    let tail = &acc[acc.len().saturating_sub(len)..];
    let s: f64 = tail.iter().sum();
    Some(s / len as f64)
}

/// Wilder RSI
pub fn wilder_rsi(closes: &[f64], period: usize) -> Vec<f64> {
    let n = closes.len();
    let mut out = vec![f64::NAN; n];
    if n < period + 1 || period < 2 {
        return out;
    }
    let mut gains = vec![0.0_f64; n];
    let mut losses = vec![0.0_f64; n];
    for i in 1..n {
        let ch = closes[i] - closes[i - 1];
        if ch >= 0.0 {
            gains[i] = ch;
        } else {
            losses[i] = -ch;
        }
    }
    let mut avg_g: f64 = gains[1..=period].iter().sum::<f64>() / period as f64;
    let mut avg_l: f64 = losses[1..=period].iter().sum::<f64>() / period as f64;
    let rs = if avg_l <= 1e-12 { 100.0 } else { avg_g / avg_l };
    out[period] = 100.0 - (100.0 / (1.0 + rs));
    for i in (period + 1)..n {
        avg_g = (avg_g * (period as f64 - 1.0) + gains[i]) / period as f64;
        avg_l = (avg_l * (period as f64 - 1.0) + losses[i]) / period as f64;
        let rs = if avg_l <= 1e-12 { 100.0 } else { avg_g / avg_l };
        out[i] = 100.0 - (100.0 / (1.0 + rs));
    }
    out
}

/// Fractal pivot low detection.
pub fn is_pivot_low(bars: &[OhlcBar], i: usize, w: usize) -> bool {
    if w < 1 || i < w || i + w >= bars.len() {
        return false;
    }
    let p = bars[i].low;
    (i.saturating_sub(w)..=(i + w)).all(|j| j == i || bars[j].low >= p)
}

/// Fractal pivot high detection.
pub fn is_pivot_high(bars: &[OhlcBar], i: usize, w: usize) -> bool {
    if w < 1 || i < w || i + w >= bars.len() {
        return false;
    }
    let p = bars[i].high;
    (i.saturating_sub(w)..=(i + w)).all(|j| j == i || bars[j].high <= p)
}

/// Average volume over a lookback window ending at `end_idx` (exclusive).
pub fn avg_volume(bars: &[OhlcBar], end_idx: usize, lookback: usize) -> Option<f64> {
    let start = end_idx.saturating_sub(lookback);
    let mut sum = 0.0;
    let mut cnt = 0usize;
    for b in &bars[start..end_idx] {
        if let Some(v) = b.volume {
            if v.is_finite() && v >= 0.0 {
                sum += v;
                cnt += 1;
            }
        }
    }
    if cnt >= lookback * 7 / 10 {
        Some(sum / cnt as f64)
    } else {
        None
    }
}

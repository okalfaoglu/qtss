//! Yatay aralık (trading range) — üst/alt bant ve isteğe bağlı likidite süpürme sinyali.
//!
//! Sınır seviyeleri **son mum hariç** `lookback` penceresinden hesaplanır (lookahead önlenir).

use serde::{Deserialize, Serialize};

use crate::OhlcBar;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingRangeParams {
    #[serde(default = "default_lookback")]
    pub lookback: usize,
    #[serde(default = "default_atr_period")]
    pub atr_period: usize,
    #[serde(default = "default_atr_sma_period")]
    pub atr_sma_period: usize,
    #[serde(default)]
    pub require_range_regime: bool,
}

fn default_lookback() -> usize {
    50
}
fn default_atr_period() -> usize {
    14
}
fn default_atr_sma_period() -> usize {
    50
}

impl Default for TradingRangeParams {
    fn default() -> Self {
        Self {
            lookback: default_lookback(),
            atr_period: default_atr_period(),
            atr_sma_period: default_atr_sma_period(),
            require_range_regime: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingRangeResult {
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub bar_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_high: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_low: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mid: Option<f64>,
    pub is_range_regime: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atr: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atr_sma: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_bar_first: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_bar_last: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_bar_index: Option<i64>,
    pub long_sweep_signal: bool,
    pub short_sweep_signal: bool,
}

fn true_range(h: f64, l: f64, prev_close: f64) -> f64 {
    let a = h - l;
    let b = (h - prev_close).abs();
    let c = (l - prev_close).abs();
    a.max(b).max(c)
}

/// Wilder ATR; `out[i]` yalnızca `i >= period` için dolu.
fn wilder_atr(bars: &[OhlcBar], period: usize) -> Vec<f64> {
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

fn last_finite_sma(slice: &[f64], len: usize) -> Option<f64> {
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

/// `bars`: `bar_index` artan sırada.
#[must_use]
pub fn analyze_trading_range(bars: &[OhlcBar], params: &TradingRangeParams) -> TradingRangeResult {
    let n = bars.len();
    let lookback = params.lookback.max(5);
    let atr_p = params.atr_period.max(2);
    let atr_sma_p = params.atr_sma_period.max(2);

    if n < lookback + 2 {
        return TradingRangeResult {
            valid: false,
            reason: Some(format!("en az {} mum gerekli (lookback+2)", lookback + 2)),
            bar_count: n,
            range_high: None,
            range_low: None,
            mid: None,
            is_range_regime: false,
            atr: None,
            atr_sma: None,
            window_bar_first: None,
            window_bar_last: None,
            last_bar_index: None,
            long_sweep_signal: false,
            short_sweep_signal: false,
        };
    }

    let atr_series = wilder_atr(bars, atr_p);
    let last_idx = n - 1;
    let atr_last = atr_series[last_idx];
    let atr_sma_last = if atr_last.is_finite() {
        last_finite_sma(&atr_series[..=last_idx], atr_sma_p)
    } else {
        None
    };

    let is_range_regime = match (atr_last.is_finite(), atr_sma_last) {
        (true, Some(sma)) => atr_last < sma,
        _ => false,
    };

    let win_start = n - 1 - lookback;
    let win_end = n - 2;
    let window = &bars[win_start..=win_end];
    let mut rh = window[0].high;
    let mut rl = window[0].low;
    let mut bf = window[0].bar_index;
    let mut bl = window[0].bar_index;
    for b in window.iter().skip(1) {
        rh = rh.max(b.high);
        rl = rl.min(b.low);
        bf = bf.min(b.bar_index);
        bl = bl.max(b.bar_index);
    }

    let mid = (rh + rl) * 0.5;
    let last = &bars[last_idx];

    let long_sweep_signal = last.low < rl && last.close > rl;
    let short_sweep_signal = last.high > rh && last.close < rh;

    let regime_ok = !params.require_range_regime || is_range_regime;
    let valid = regime_ok;

    TradingRangeResult {
        valid,
        reason: if regime_ok {
            None
        } else {
            Some("require_range_regime: ATR >= ATR_SMA".to_string())
        },
        bar_count: n,
        range_high: Some(rh),
        range_low: Some(rl),
        mid: Some(mid),
        is_range_regime,
        atr: atr_last.is_finite().then_some(atr_last),
        atr_sma: atr_sma_last,
        window_bar_first: Some(bf),
        window_bar_last: Some(bl),
        last_bar_index: Some(last.bar_index),
        long_sweep_signal: regime_ok && long_sweep_signal,
        short_sweep_signal: regime_ok && short_sweep_signal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(i: i64, o: f64, h: f64, l: f64, c: f64) -> OhlcBar {
        OhlcBar {
            open: o,
            high: h,
            low: l,
            close: c,
            bar_index: i,
        }
    }

    #[test]
    fn flat_range_sweep_long() {
        let mut v: Vec<OhlcBar> = Vec::new();
        for i in 0..60i64 {
            v.push(bar(i, 100.0, 101.0, 99.0, 100.5));
        }
        let last = v.len() - 1;
        v[last] = bar(last as i64, 100.0, 100.0, 98.5, 99.2);
        let p = TradingRangeParams {
            lookback: 50,
            atr_period: 14,
            atr_sma_period: 20,
            require_range_regime: false,
        };
        let r = analyze_trading_range(&v, &p);
        assert!(r.valid);
        assert!(r.long_sweep_signal);
    }
}

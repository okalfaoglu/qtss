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
    /// Pivot temas sayımı için fraktal pencere (her iki yanda mum sayısı).
    #[serde(default = "default_pivot_window")]
    pub pivot_window: usize,
    /// Destek temas minimumu (pivot low).
    #[serde(default = "default_min_support_touches")]
    pub min_support_touches: usize,
    /// Direnç temas minimumu (pivot high).
    #[serde(default = "default_min_resistance_touches")]
    pub min_resistance_touches: usize,
    /// Temas toleransı (fiyat): `ATR × mult` (ATR yoksa range_width tabanlı fallback).
    #[serde(default = "default_touch_tolerance_atr_mult")]
    pub touch_tolerance_atr_mult: f64,
    /// Close bazlı breakout filtresi: son `N` kapanıştan biri band dışına çıktıysa setup iptal.
    #[serde(default = "default_close_breakout_lookback")]
    pub close_breakout_lookback: usize,
    /// Range genişliği filtresi (ATR birimi): çok dar → skip.
    #[serde(default = "default_range_width_min_atr_mult")]
    pub range_width_min_atr_mult: f64,
    /// Range genişliği filtresi (ATR birimi): çok geniş → skip.
    #[serde(default = "default_range_width_max_atr_mult")]
    pub range_width_max_atr_mult: f64,
    /// Setup skoru (0–100) için minimum eşik: `durum` üretir.
    #[serde(default = "default_setup_score_threshold")]
    pub setup_score_threshold: i32,
    /// “Strong” eşik (bilgi amaçlı).
    #[serde(default = "default_setup_score_strong_threshold")]
    pub setup_score_strong_threshold: i32,
    /// Üst/alt “A+” bant kalınlığı: `range_width * zone_edge_fraction` (Pine `zone_perc` ile aynı fikir).
    #[serde(default = "default_zone_edge_fraction")]
    pub zone_edge_fraction: f64,
    /// Üst % / alt % / orta no-trade + çift yönlü kısıt (üstte LONG yok, altta SHORT yok).
    #[serde(default = "default_enable_range_zone_filter")]
    pub enable_range_zone_filter: bool,
    /// Kenar bölgede yön seçilirken likidite süpürme + reclaim (`fake_breakout_*` veya `*_sweep_latent`) zorunlu.
    #[serde(default = "default_require_edge_reclaim_for_setup")]
    pub require_edge_reclaim_for_setup: bool,
    /// Wilder RSI periyodu (skor bileşeni).
    #[serde(default = "default_rsi_period")]
    pub rsi_period: usize,
    /// RSI bu eşiğin altındayken long tarafı “aşırı satım” skoru alır.
    #[serde(default = "default_rsi_oversold")]
    pub rsi_oversold: f64,
    /// RSI bu eşiğin üstündeyken short tarafı “aşırı alım” skoru alır.
    #[serde(default = "default_rsi_overbought")]
    pub rsi_overbought: f64,
    /// Son mum hacmini kıyaslamak için önceki mumların ortalama hacmi (pencere uzunluğu).
    #[serde(default = "default_volume_avg_lookback")]
    pub volume_avg_lookback: usize,
    /// `last_volume / avg_prior_volume` ≥ bu değer → tam hacim skoru (15).
    #[serde(default = "default_volume_spike_ratio_full")]
    pub volume_spike_ratio_full: f64,
    /// ≥ bu değer (ve < full) → kısmi hacim skoru (8).
    #[serde(default = "default_volume_spike_ratio_partial")]
    pub volume_spike_ratio_partial: f64,
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
fn default_pivot_window() -> usize {
    3
}
fn default_min_support_touches() -> usize {
    2
}
fn default_min_resistance_touches() -> usize {
    2
}
fn default_touch_tolerance_atr_mult() -> f64 {
    0.25
}
fn default_close_breakout_lookback() -> usize {
    20
}
fn default_range_width_min_atr_mult() -> f64 {
    1.0
}
fn default_range_width_max_atr_mult() -> f64 {
    6.0
}
fn default_setup_score_threshold() -> i32 {
    60
}
fn default_setup_score_strong_threshold() -> i32 {
    75
}
fn default_zone_edge_fraction() -> f64 {
    0.25
}
fn default_enable_range_zone_filter() -> bool {
    true
}
fn default_require_edge_reclaim_for_setup() -> bool {
    true
}
fn default_rsi_period() -> usize {
    14
}
fn default_rsi_oversold() -> f64 {
    30.0
}
fn default_rsi_overbought() -> f64 {
    70.0
}
fn default_volume_avg_lookback() -> usize {
    20
}
fn default_volume_spike_ratio_full() -> f64 {
    1.5
}
fn default_volume_spike_ratio_partial() -> f64 {
    1.15
}

impl Default for TradingRangeParams {
    fn default() -> Self {
        Self {
            lookback: default_lookback(),
            atr_period: default_atr_period(),
            atr_sma_period: default_atr_sma_period(),
            require_range_regime: false,
            pivot_window: default_pivot_window(),
            min_support_touches: default_min_support_touches(),
            min_resistance_touches: default_min_resistance_touches(),
            touch_tolerance_atr_mult: default_touch_tolerance_atr_mult(),
            close_breakout_lookback: default_close_breakout_lookback(),
            range_width_min_atr_mult: default_range_width_min_atr_mult(),
            range_width_max_atr_mult: default_range_width_max_atr_mult(),
            setup_score_threshold: default_setup_score_threshold(),
            setup_score_strong_threshold: default_setup_score_strong_threshold(),
            zone_edge_fraction: default_zone_edge_fraction(),
            enable_range_zone_filter: default_enable_range_zone_filter(),
            require_edge_reclaim_for_setup: default_require_edge_reclaim_for_setup(),
            rsi_period: default_rsi_period(),
            rsi_oversold: default_rsi_oversold(),
            rsi_overbought: default_rsi_overbought(),
            volume_avg_lookback: default_volume_avg_lookback(),
            volume_spike_ratio_full: default_volume_spike_ratio_full(),
            volume_spike_ratio_partial: default_volume_spike_ratio_partial(),
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
    /// Rejim / `require_range_regime` sonrası etkin süpürme (strateji & bildirim).
    pub long_sweep_signal: bool,
    pub short_sweep_signal: bool,
    /// Fiyat aralığı süpürmesi (son mum): `ATR` rejimi uygun olmasa bile grafik için.
    pub long_sweep_latent: bool,
    pub short_sweep_latent: bool,
    /// Hard: pivot temas sayıları (wick dahil).
    pub support_touches: usize,
    pub resistance_touches: usize,
    /// Hard: kapanış breakout filtresi (son N kapanış band dışında mı?).
    pub close_breakout: bool,
    /// Range metrikleri.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_width_atr: Option<f64>,
    pub range_too_narrow: bool,
    pub range_too_wide: bool,
    /// Soft: wick rejection (son mum) — support/resistance üzerinden.
    pub wick_rejection_long: bool,
    pub wick_rejection_short: bool,
    /// Soft: fake breakout (liquidity grab) — wick dışarı taşar, close geri döner.
    pub fake_breakout_long: bool,
    pub fake_breakout_short: bool,
    /// Skor tabanlı setup çıktısı.
    pub setup_score_long: i32,
    pub setup_score_short: i32,
    pub setup_score_best: i32,
    /// Guardrails geçildi mi? (touch + close-breakout + width + rejim vb.)
    pub guardrails_pass: bool,
    /// `LONG` / `SHORT` / `NOTR`
    pub setup_side: String,
    /// Skor kırılımı (0–100).
    pub score_touch_long: i32,
    pub score_touch_short: i32,
    pub score_rejection_long: i32,
    pub score_rejection_short: i32,
    pub score_oscillator_long: i32,
    pub score_oscillator_short: i32,
    pub score_volume_long: i32,
    pub score_volume_short: i32,
    pub score_breakout_long: i32,
    pub score_breakout_short: i32,
    /// `true` ise son mumda veya kıyas penceresinde yeterli `OhlcBar.volume` yok (hacim skoru 0).
    pub volume_unavailable: bool,
    /// Son mum kapanışının range içindeki bölgesi: `upper` | `mid` | `lower`.
    pub range_zone: String,
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

/// Wilder RSI; `out[i]` yalnız `i >= period` için anlamlı.
fn wilder_rsi(closes: &[f64], period: usize) -> Vec<f64> {
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
    let mut avg_g = 0.0_f64;
    let mut avg_l = 0.0_f64;
    for i in 1..=period {
        avg_g += gains[i];
        avg_l += losses[i];
    }
    avg_g /= period as f64;
    avg_l /= period as f64;
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

/// Hacim skoru (0–15 / yön): son mum hacmi, önceki penceredeki ortalamaya göre; yalnız ilgili kenar bölgesindeyken uygulanır.
fn volume_scores_by_zone(
    bars: &[OhlcBar],
    last_idx: usize,
    params: &TradingRangeParams,
    long_in_zone: bool,
    short_in_zone: bool,
) -> (i32, i32, bool) {
    let lb = params
        .volume_avg_lookback
        .max(3)
        .min(last_idx.max(1));
    if last_idx == 0 {
        return (0, 0, true);
    }
    let last = &bars[last_idx];
    let Some(last_v) = last.volume.filter(|v| v.is_finite() && *v >= 0.0) else {
        return (0, 0, true);
    };
    let start = last_idx.saturating_sub(lb);
    let mut sum = 0.0_f64;
    let mut cnt = 0usize;
    for b in &bars[start..last_idx] {
        if let Some(v) = b.volume {
            if v.is_finite() && v >= 0.0 {
                sum += v;
                cnt += 1;
            }
        }
    }
    let min_needed = (lb * 7 / 10).max(3).min(lb);
    if cnt < min_needed {
        return (0, 0, true);
    }
    let avg = sum / cnt as f64;
    if avg <= 1e-18 {
        return (0, 0, false);
    }
    let ratio = last_v / avg;
    let r_hi = params.volume_spike_ratio_full.max(1.000_001);
    let r_lo = params.volume_spike_ratio_partial.clamp(1.0, r_hi);
    let pts = if ratio >= r_hi {
        15
    } else if ratio >= r_lo {
        8
    } else {
        0
    };
    let v_long = if long_in_zone { pts } else { 0 };
    let v_short = if short_in_zone { pts } else { 0 };
    (v_long, v_short, false)
}

fn is_pivot_low(bars: &[OhlcBar], i: usize, w: usize) -> bool {
    if w < 1 {
        return false;
    }
    if i < w || i + w >= bars.len() {
        return false;
    }
    let p = bars[i].low;
    for j in (i - w)..=(i + w) {
        if j == i {
            continue;
        }
        if bars[j].low < p {
            return false;
        }
    }
    true
}

/// Son kapanışın range içindeki konumu (üst/alt %25 benzeri bantlar, orta = no-trade bölgesi).
fn classify_range_zone(close: f64, rh: f64, rl: f64, edge_fraction: f64) -> &'static str {
    let w = rh - rl;
    if !close.is_finite() || !w.is_finite() || w <= 1e-12 {
        return "mid";
    }
    let f = edge_fraction.clamp(0.05, 0.45);
    let upper_band = rh - w * f;
    let lower_band = rl + w * f;
    if close >= upper_band {
        "upper"
    } else if close <= lower_band {
        "lower"
    } else {
        "mid"
    }
}

/// Orta bölgede çift yönlü setup kapalı; kenarda yalnız uygun yön; reclaim/sweep şartı isteğe bağlı.
fn apply_range_zone_and_reclaim(
    params: &TradingRangeParams,
    close: f64,
    rh: f64,
    rl: f64,
    long_sweep_latent: bool,
    fake_breakout_long: bool,
    short_sweep_latent: bool,
    fake_breakout_short: bool,
    score_long: &mut i32,
    score_short: &mut i32,
) -> &'static str {
    let zone = classify_range_zone(close, rh, rl, params.zone_edge_fraction);
    if !params.enable_range_zone_filter {
        return zone;
    }
    match zone {
        "mid" => {
            *score_long = 0;
            *score_short = 0;
        }
        "upper" => {
            *score_long = 0;
            if params.require_edge_reclaim_for_setup
                && *score_short > 0
                && !(short_sweep_latent || fake_breakout_short)
            {
                *score_short = 0;
            }
        }
        "lower" => {
            *score_short = 0;
            if params.require_edge_reclaim_for_setup
                && *score_long > 0
                && !(long_sweep_latent || fake_breakout_long)
            {
                *score_long = 0;
            }
        }
        _ => {}
    }
    zone
}

fn is_pivot_high(bars: &[OhlcBar], i: usize, w: usize) -> bool {
    if w < 1 {
        return false;
    }
    if i < w || i + w >= bars.len() {
        return false;
    }
    let p = bars[i].high;
    for j in (i - w)..=(i + w) {
        if j == i {
            continue;
        }
        if bars[j].high > p {
            return false;
        }
    }
    true
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
            reason: Some(format!(
                "en az {} mum gerekli (lookback+2)",
                lookback + 2
            )),
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
            long_sweep_latent: false,
            short_sweep_latent: false,
            support_touches: 0,
            resistance_touches: 0,
            close_breakout: false,
            range_width: None,
            range_width_atr: None,
            range_too_narrow: false,
            range_too_wide: false,
            wick_rejection_long: false,
            wick_rejection_short: false,
            fake_breakout_long: false,
            fake_breakout_short: false,
            setup_score_long: 0,
            setup_score_short: 0,
            setup_score_best: 0,
            guardrails_pass: false,
            setup_side: "NOTR".to_string(),
            score_touch_long: 0,
            score_touch_short: 0,
            score_rejection_long: 0,
            score_rejection_short: 0,
            score_oscillator_long: 0,
            score_oscillator_short: 0,
            score_volume_long: 0,
            score_volume_short: 0,
            score_breakout_long: 0,
            score_breakout_short: 0,
            volume_unavailable: true,
            range_zone: "unknown".to_string(),
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

    let long_sweep_latent = last.low < rl && last.close > rl;
    let short_sweep_latent = last.high > rh && last.close < rh;

    let range_width = (rh - rl).abs();
    let tol_px = if atr_last.is_finite() {
        (atr_last * params.touch_tolerance_atr_mult).abs()
    } else {
        (range_width * 0.01).abs()
    };

    // Pivot temas sayımı (wick dahil): lookback penceresinde pivot low/high kümeleri.
    let w = params.pivot_window.max(1).min(20);
    let mut pivot_lows: Vec<f64> = Vec::new();
    let mut pivot_highs: Vec<f64> = Vec::new();
    for i in win_start..=win_end {
        if is_pivot_low(bars, i, w) {
            pivot_lows.push(bars[i].low);
        }
        if is_pivot_high(bars, i, w) {
            pivot_highs.push(bars[i].high);
        }
    }
    let support_touches = pivot_lows
        .iter()
        .filter(|x| (**x - rl).abs() <= tol_px)
        .count();
    let resistance_touches = pivot_highs
        .iter()
        .filter(|x| (**x - rh).abs() <= tol_px)
        .count();

    // Close bazlı breakout filtresi: son N kapanıştan biri band dışında mı?
    let ncl = params.close_breakout_lookback.max(3).min(200);
    let from = n.saturating_sub(ncl);
    let mut close_breakout = false;
    for b in &bars[from..] {
        if b.close > rh + tol_px || b.close < rl - tol_px {
            close_breakout = true;
            break;
        }
    }

    let range_width_atr = if atr_last.is_finite() && atr_last.abs() > 1e-12 {
        Some(range_width / atr_last.abs())
    } else {
        None
    };
    let (range_too_narrow, range_too_wide) = match range_width_atr {
        Some(x) => (
            x < params.range_width_min_atr_mult.max(0.05),
            x > params.range_width_max_atr_mult.max(params.range_width_min_atr_mult.max(0.05) + 0.05),
        ),
        None => (false, false),
    };

    // Wick rejection / fake breakout: son mum için support/resistance çevresinde.
    let wick_rejection_long = last.low < rl && last.close > rl;
    let wick_rejection_short = last.high > rh && last.close < rh;
    let fb_wick_px = if atr_last.is_finite() {
        atr_last.abs() * 0.25
    } else {
        tol_px.max(range_width * 0.01)
    };
    let fake_breakout_long = last.low < rl - fb_wick_px && last.close >= rl;
    let fake_breakout_short = last.high > rh + fb_wick_px && last.close <= rh;

    // Hard kurallar: temas + close breakout + width filtresi (ATR yoksa width filtreleri es geçilir).
    let touches_ok = support_touches >= params.min_support_touches.max(1)
        && resistance_touches >= params.min_resistance_touches.max(1);
    let hard_ok = touches_ok && !close_breakout && !range_too_narrow && !range_too_wide;

    let regime_ok = !params.require_range_regime || is_range_regime;
    let valid = regime_ok && hard_ok;
    let reason = if !regime_ok {
        Some("require_range_regime: ATR >= ATR_SMA".to_string())
    } else if !touches_ok {
        Some("touches: support/resistance < min".to_string())
    } else if close_breakout {
        Some("breakout: close outside range".to_string())
    } else if range_too_narrow {
        Some("range_width: too narrow".to_string())
    } else if range_too_wide {
        Some("range_width: too wide".to_string())
    } else {
        None
    };

    // === Full-score decision engine ===
    // Minimal guardrails + weighted scoring:
    // touch_score (0–30) + rejection (0–20) + oscillator (0–15) + volume (0–15) + breakout_behavior (0–20)

    let guardrails_pass = regime_ok && hard_ok;

    // Touch score: support/resistance oranlarına göre ölçeklenir (0–30).
    let sup_ratio = (support_touches as f64) / (params.min_support_touches.max(1) as f64);
    let res_ratio = (resistance_touches as f64) / (params.min_resistance_touches.max(1) as f64);
    let touch_score = ((sup_ratio.min(1.0) * 15.0) + (res_ratio.min(1.0) * 15.0)).round() as i32;
    let score_touch_long = touch_score.clamp(0, 30);
    let score_touch_short = score_touch_long;

    // Location (zone): son mum band sınırına yakın mı? (wick dahil)
    let long_in_zone = (last.low - rl).abs() <= tol_px || (last.close - rl).abs() <= tol_px;
    let short_in_zone = (last.high - rh).abs() <= tol_px || (last.close - rh).abs() <= tol_px;

    // RSI (Wilder) — skor bileşeni; periyot ve bantlar `TradingRangeParams` üzerinden.
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let rsi_p = params.rsi_period.max(2).min(200);
    let mut rsi_low = params.rsi_oversold;
    let mut rsi_high = params.rsi_overbought;
    if !(rsi_low.is_finite() && rsi_high.is_finite()) || rsi_low >= rsi_high {
        rsi_low = default_rsi_oversold();
        rsi_high = default_rsi_overbought();
    }
    let rsi_s = wilder_rsi(&closes, rsi_p);
    let rsi_last = rsi_s.last().copied().unwrap_or(f64::NAN);
    let rsi_long_extreme = rsi_last.is_finite() && rsi_last < rsi_low;
    let rsi_short_extreme = rsi_last.is_finite() && rsi_last > rsi_high;

    // Rejection score (0–20): zone + wick rejection + (bonus) fake breakout
    let mut rej_long = 0;
    let mut rej_short = 0;
    if long_in_zone {
        rej_long += 5;
    }
    if short_in_zone {
        rej_short += 5;
    }
    if wick_rejection_long {
        rej_long += 10;
    }
    if wick_rejection_short {
        rej_short += 10;
    }
    if fake_breakout_long {
        rej_long += 5;
    }
    if fake_breakout_short {
        rej_short += 5;
    }
    let score_rejection_long = rej_long.clamp(0, 20);
    let score_rejection_short = rej_short.clamp(0, 20);

    // Oscillator score (0–15): RSI extreme
    let score_oscillator_long = (if rsi_long_extreme { 15 } else { 0 }).clamp(0, 15);
    let score_oscillator_short = (if rsi_short_extreme { 15 } else { 0 }).clamp(0, 15);

    let (score_volume_long, score_volume_short, volume_unavailable) =
        volume_scores_by_zone(bars, last_idx, params, long_in_zone, short_in_zone);

    // Breakout behavior (0–20): fake breakout en güçlü; aksi halde wick rejection orta.
    let score_breakout_long = if fake_breakout_long {
        20
    } else if wick_rejection_long || long_sweep_latent {
        10
    } else {
        0
    };
    let score_breakout_short = if fake_breakout_short {
        20
    } else if wick_rejection_short || short_sweep_latent {
        10
    } else {
        0
    };

    // Full decision score
    let mut score_long: i32 = score_touch_long
        + score_rejection_long
        + score_oscillator_long
        + score_volume_long
        + score_breakout_long;
    let mut score_short: i32 = score_touch_short
        + score_rejection_short
        + score_oscillator_short
        + score_volume_short
        + score_breakout_short;

    // Guardrails FAIL → reject (score forced to 0)
    if !guardrails_pass {
        score_long = 0;
        score_short = 0;
    }

    let range_zone = apply_range_zone_and_reclaim(
        params,
        last.close,
        rh,
        rl,
        long_sweep_latent,
        fake_breakout_long,
        short_sweep_latent,
        fake_breakout_short,
        &mut score_long,
        &mut score_short,
    )
    .to_string();

    // Dynamic threshold: range rejiminde daha düşük, değilse daha yüksek.
    let threshold = if is_range_regime { params.setup_score_threshold } else { params.setup_score_strong_threshold };

    let (setup_side, setup_score_best) = if score_long >= threshold && score_long >= score_short {
        ("LONG".to_string(), score_long)
    } else if score_short >= threshold {
        ("SHORT".to_string(), score_short)
    } else {
        ("NOTR".to_string(), score_long.max(score_short))
    };

    TradingRangeResult {
        valid,
        reason,
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
        long_sweep_signal: regime_ok && long_sweep_latent,
        short_sweep_signal: regime_ok && short_sweep_latent,
        long_sweep_latent,
        short_sweep_latent,
        support_touches,
        resistance_touches,
        close_breakout,
        range_width: Some(range_width),
        range_width_atr,
        range_too_narrow,
        range_too_wide,
        wick_rejection_long,
        wick_rejection_short,
        fake_breakout_long,
        fake_breakout_short,
        setup_score_long: score_long,
        setup_score_short: score_short,
        setup_score_best,
        guardrails_pass,
        setup_side,
        score_touch_long,
        score_touch_short,
        score_rejection_long,
        score_rejection_short,
        score_oscillator_long,
        score_oscillator_short,
        score_volume_long,
        score_volume_short,
        score_breakout_long,
        score_breakout_short,
        volume_unavailable,
        range_zone,
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
            volume: None,
        }
    }

    #[test]
    fn classify_range_zones_three_bands() {
        assert_eq!(classify_range_zone(103.0, 104.0, 100.0, 0.25), "upper");
        assert_eq!(classify_range_zone(100.5, 104.0, 100.0, 0.25), "lower");
        assert_eq!(classify_range_zone(102.0, 104.0, 100.0, 0.25), "mid");
    }

    #[test]
    fn volume_spike_at_support_zones_long_only() {
        let mut v: Vec<OhlcBar> = Vec::new();
        for i in 0..25i64 {
            v.push(OhlcBar {
                open: 100.0,
                high: 101.0,
                low: 99.0,
                close: 100.0,
                bar_index: i,
                volume: Some(1000.0),
            });
        }
        let last = v.len() - 1;
        v[last] = OhlcBar {
            open: 100.0,
            high: 100.5,
            low: 98.5,
            close: 99.0,
            bar_index: last as i64,
            volume: Some(3000.0),
        };
        let p = TradingRangeParams {
            volume_avg_lookback: 20,
            ..TradingRangeParams::default()
        };
        let (lo, sh, unavail) =
            volume_scores_by_zone(&v, last, &p, true, false);
        assert!(!unavail);
        assert_eq!(lo, 15);
        assert_eq!(sh, 0);
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
            ..TradingRangeParams::default()
        };
        let r = analyze_trading_range(&v, &p);
        // Valid artık hard kurallara da bağlı; bu basit test yalnız sweep tespitini doğrular.
        assert!(r.long_sweep_latent);
        // require_range_regime kapalıysa signal true olur; hard kural fail olsa da latent/signal üretimi devam edebilir.
        assert!(r.long_sweep_signal);
    }
}

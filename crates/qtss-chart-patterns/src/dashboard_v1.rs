//! Sinyal paneli (v1) — OHLC + [`TradingRangeResult`] ile üretilir; `analysis_snapshots.engine_kind = signal_dashboard`.

use serde::{Deserialize, Serialize};

use crate::trading_range::TradingRangeResult;
use crate::OhlcBar;

/// Range + trend modelinin **yürütülebilir** yönü: spot’ta çoğunlukla `LongOnly`, USDT-M’de `Both`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalDirectionPolicy {
    /// LONG ve SHORT sinyalleri (vadeli / hedge).
    #[default]
    Both,
    /// SHORT model çıktısı `NOTR`’a indirgenir; kısa sweep giriş planı temizlenir.
    LongOnly,
    /// LONG model çıktısı `NOTR`’a indirgenir (nadir senaryolar).
    ShortOnly,
}

impl SignalDirectionPolicy {
    const fn allows_long(self) -> bool {
        matches!(self, Self::Both | Self::LongOnly)
    }
    const fn allows_short(self) -> bool {
        matches!(self, Self::Both | Self::ShortOnly)
    }

    /// API / JSON snapshot alanları (`signal_direction_effective`).
    #[must_use]
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::Both => "both",
            Self::LongOnly => "long_only",
            Self::ShortOnly => "short_only",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalDashboardV1 {
    pub schema_version: i32,
    /// LONG | SHORT | NOTR — yön politikası sonrası (F1 `durum` kenarı bunu kullanır).
    pub durum: String,
    /// Ham model çıktısı (politika öncesi).
    pub durum_model_raw: String,
    /// YUKARI | ASAGI | YATAY
    pub yerel_trend: String,
    /// YUKARI | ASAGI | YATAY | KAPALI (veri kısa)
    pub global_trend: String,
    /// RANGE | KOPUS | TREND | BELIRSIZ
    pub piyasa_modu: String,
    /// DONUS | TAKIP | —
    pub giris_modu: String,
    pub oynaklik_pct: f64,
    /// POZITIF | NEGATIF | NOTR
    pub momentum_1: String,
    pub momentum_2: String,
    pub giris_gercek: Option<f64>,
    pub stop_ilk: Option<f64>,
    pub kar_al_ilk: Option<f64>,
    pub stop_trail_aktif: Option<f64>,
    pub kar_al_dinamik: Option<f64>,
    pub sinyal_kaynagi: String,
    pub trend_tukenmesi: bool,
    pub yapi_kaymasi: bool,
    /// 0–10
    pub pozisyon_gucu_10: u8,
    pub sistem_aktif: bool,
    /// Wilder RSI(14) on the last closed bar; UI may show as `KOPUŞ (57.26)`-style context (independent of range-setup string).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rsi_14_last: Option<f64>,
}

fn closes(bars: &[OhlcBar]) -> Vec<f64> {
    bars.iter().map(|b| b.close).collect()
}

fn sma_last(vals: &[f64], period: usize) -> Option<f64> {
    if vals.len() < period {
        return None;
    }
    let s: f64 = vals[vals.len() - period..].iter().sum();
    Some(s / period as f64)
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

fn rsi_label(v: f64) -> &'static str {
    if v > 55.0 {
        "POZITIF"
    } else if v < 45.0 {
        "NEGATIF"
    } else {
        "NOTR"
    }
}

fn roc_pct(closes: &[f64], lookback: usize) -> f64 {
    let n = closes.len();
    if n <= lookback {
        return 0.0;
    }
    let a = closes[n - 1];
    let b = closes[n - 1 - lookback];
    if b.abs() < 1e-12 {
        return 0.0;
    }
    (a / b - 1.0) * 100.0
}

fn roc_label(pct: f64) -> &'static str {
    if pct > 0.15 {
        "POZITIF"
    } else if pct < -0.15 {
        "NEGATIF"
    } else {
        "NOTR"
    }
}

/// Son `close`, bir önceki `exclude_last` mum içindeki high/low kırılımı.
fn normalized_setup_side(raw: &str) -> &'static str {
    let s = raw.trim().to_uppercase().replace('İ', "I");
    match s.as_str() {
        "LONG" => "LONG",
        "SHORT" => "SHORT",
        _ => "NOTR",
    }
}

/// LONG: `sl < entry < tp`; SHORT: `tp < entry < sl`. Aksi halde seviyeler yanıltıcıdır (süpürme + kapanış uyumsuzluğu).
fn trade_levels_consistent(side: &str, entry: f64, sl: f64, tp: f64) -> bool {
    if !entry.is_finite() || !sl.is_finite() || !tp.is_finite() {
        return false;
    }
    match side {
        "LONG" => sl < entry && entry < tp,
        "SHORT" => tp < entry && entry < sl,
        _ => false,
    }
}

fn structure_shift(bars: &[OhlcBar], exclude_last: usize) -> bool {
    let n = bars.len();
    if n < exclude_last + 3 {
        return false;
    }
    let last = bars[n - 1].close;
    let from = n.saturating_sub(exclude_last + 1);
    let to = n - 2;
    if from >= to {
        return false;
    }
    let mut hi = f64::NEG_INFINITY;
    let mut lo = f64::INFINITY;
    for b in &bars[from..=to] {
        hi = hi.max(b.high);
        lo = lo.min(b.low);
    }
    last > hi || last < lo
}

/// `tr` ile aynı mum dilimi; `bars` kronolojik artan. Politika: çift yönlü (varsayılan).
#[must_use]
pub fn compute_signal_dashboard_v1(bars: &[OhlcBar], tr: &TradingRangeResult) -> SignalDashboardV1 {
    compute_signal_dashboard_v1_with_policy(bars, tr, SignalDirectionPolicy::Both)
}

/// [`SignalDirectionPolicy`] ile spot tek yön / vadeli çift yön ayrımı.
#[must_use]
pub fn compute_signal_dashboard_v1_with_policy(
    bars: &[OhlcBar],
    tr: &TradingRangeResult,
    direction_policy: SignalDirectionPolicy,
) -> SignalDashboardV1 {
    let c = closes(bars);
    let n = c.len();
    let last_c = if n > 0 { c[n - 1] } else { f64::NAN };

    let sma20 = sma_last(&c, 20);
    let sma50 = sma_last(&c, 50);

    let yerel_trend = match sma20 {
        Some(s) if last_c.is_finite() && s.is_finite() => {
            let p = (last_c - s) / s * 100.0;
            if p > 0.08 {
                "YUKARI"
            } else if p < -0.08 {
                "ASAGI"
            } else {
                "YATAY"
            }
        }
        _ => "YATAY",
    }
    .to_string();

    let global_trend = if n < 80 {
        "KAPALI".to_string()
    } else {
        match sma50 {
            Some(s) if last_c.is_finite() && s.is_finite() => {
                let p = (last_c - s) / s * 100.0;
                if p > 0.12 {
                    "YUKARI".to_string()
                } else if p < -0.12 {
                    "ASAGI".to_string()
                } else {
                    "YATAY".to_string()
                }
            }
            _ => "KAPALI".to_string(),
        }
    };

    let rsi_s = wilder_rsi(&c, 14);
    let rsi_last = rsi_s.last().copied().unwrap_or(f64::NAN);
    let momentum_1 = if rsi_last.is_finite() {
        rsi_label(rsi_last).to_string()
    } else {
        "NOTR".to_string()
    };
    let roc10 = roc_pct(&c, 10);
    let momentum_2 = roc_label(roc10).to_string();

    let oynaklik_pct = if last_c.is_finite() && last_c.abs() > 1e-12 {
        tr.atr
            .filter(|a| a.is_finite())
            .map(|a| a / last_c * 100.0)
            .unwrap_or(0.0)
    } else {
        0.0
    };

    let piyasa_modu = if tr.long_sweep_signal || tr.short_sweep_signal {
        "KOPUS"
    } else if tr.is_range_regime && tr.valid {
        "RANGE"
    } else if yerel_trend != "YATAY" {
        "TREND"
    } else {
        "BELIRSIZ"
    }
    .to_string();

    let giris_modu = if tr.long_sweep_signal || tr.short_sweep_signal {
        "DONUS"
    } else {
        "TAKIP"
    }
    .to_string();

    let (rh, rl) = (tr.range_high, tr.range_low);
    let buffer = rh
        .zip(rl)
        .map(|(h, l)| (h - l).abs() * 0.008)
        .unwrap_or(0.0);

    let entry_opt = last_c.is_finite().then_some(last_c);
    let long_levels = if tr.long_sweep_signal {
        let sl = rl.map(|x| x - buffer);
        let tp = rh;
        (entry_opt, sl, tp)
    } else {
        (None, None, None)
    };
    let short_levels = if tr.short_sweep_signal {
        let sl = rh.map(|x| x + buffer);
        let tp = rl;
        (entry_opt, sl, tp)
    } else {
        (None, None, None)
    };

    // Skor tabanlı setup motoru: TradingRangeResult içindeki karar (hard+soft+eşik).
    // Trend fallback kaldırıldı: setup yoksa NOTR.
    let durum_model_raw = tr.setup_side.clone();
    let side_norm = normalized_setup_side(&durum_model_raw);

    let (mut giris_gercek, mut stop_ilk, mut kar_al_ilk) = match side_norm {
        "LONG" => long_levels,
        "SHORT" => short_levels,
        _ => (None, None, None),
    };

    if let (Some(e), Some(sl), Some(tp)) = (giris_gercek, stop_ilk, kar_al_ilk) {
        if !trade_levels_consistent(side_norm, e, sl, tp) {
            giris_gercek = None;
            stop_ilk = None;
            kar_al_ilk = None;
        }
    }

    // İlk TP (`kar_al_ilk`) ile dinamik TP aynı başlar; yürütülebilir kurulum yoksa trail/dinamik gösterilmez (orta bant yetim kalmaz).
    let levels_geom_ok = giris_gercek.is_some() && stop_ilk.is_some() && kar_al_ilk.is_some();
    let mut kar_al_dinamik = if levels_geom_ok { kar_al_ilk } else { None };
    let mut stop_trail_aktif = if levels_geom_ok { stop_ilk } else { None };

    let mut durum = match direction_policy {
        SignalDirectionPolicy::Both => durum_model_raw.clone(),
        SignalDirectionPolicy::LongOnly if durum_model_raw == "SHORT" => "NOTR".to_string(),
        SignalDirectionPolicy::ShortOnly if durum_model_raw == "LONG" => "NOTR".to_string(),
        _ => durum_model_raw.clone(),
    };

    if direction_policy == SignalDirectionPolicy::LongOnly
        && durum_model_raw == "SHORT"
        && tr.short_sweep_signal
    {
        giris_gercek = None;
        stop_ilk = None;
        kar_al_ilk = None;
        stop_trail_aktif = None;
        kar_al_dinamik = None;
    }
    if direction_policy == SignalDirectionPolicy::ShortOnly
        && durum_model_raw == "LONG"
        && tr.long_sweep_signal
    {
        giris_gercek = None;
        stop_ilk = None;
        kar_al_ilk = None;
        stop_trail_aktif = None;
        kar_al_dinamik = None;
    }

    // İlk etap: yürütülebilir üçlü (entry + SL + TP) yoksa etkin sinyal LONG/SHORT olamaz;
    // skor ham olarak LONG/SHORT dese bile NOTR. Sonraki fazda TP revizyonu / iz süren SL ayrı güncellenir.
    let levels_ready = giris_gercek.is_some() && stop_ilk.is_some() && kar_al_ilk.is_some();
    if !levels_ready && (durum == "LONG" || durum == "SHORT") {
        durum = "NOTR".to_string();
    }

    let trend_tukenmesi = rsi_last.is_finite() && (rsi_last > 72.0 || rsi_last < 28.0);
    let yapi_kaymasi = structure_shift(bars, 18);

    let sweep_long_for_score = tr.long_sweep_signal && direction_policy.allows_long();
    let sweep_short_for_score = tr.short_sweep_signal && direction_policy.allows_short();

    let mut score: i32 = 4;
    if sweep_long_for_score || sweep_short_for_score {
        score += 2;
    }
    if (sweep_long_for_score && momentum_1 == "POZITIF")
        || (sweep_short_for_score && momentum_1 == "NEGATIF")
    {
        score += 1;
    }
    if piyasa_modu != "BELIRSIZ" {
        score += 1;
    }
    if yapi_kaymasi {
        score += 1;
    }
    if tr.valid && tr.is_range_regime {
        score += 1;
    }
    let pozisyon_gucu_10 = (score.clamp(0, 10)) as u8;

    let sinyal_kaynagi = format!(
        "RANGE_SETUP(score_best={}, long={}, short={}, zone={})",
        tr.setup_score_best,
        tr.setup_score_long,
        tr.setup_score_short,
        tr.range_zone
    );

    let rsi_14_last = rsi_last
        .is_finite()
        .then(|| (rsi_last * 100.0).round() / 100.0);

    SignalDashboardV1 {
        schema_version: 2,
        durum,
        durum_model_raw,
        yerel_trend,
        global_trend,
        piyasa_modu,
        giris_modu,
        oynaklik_pct,
        momentum_1,
        momentum_2,
        giris_gercek,
        stop_ilk,
        kar_al_ilk,
        stop_trail_aktif,
        kar_al_dinamik,
        sinyal_kaynagi,
        trend_tukenmesi,
        yapi_kaymasi,
        pozisyon_gucu_10,
        sistem_aktif: true,
        rsi_14_last,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading_range::{analyze_trading_range, TradingRangeParams, TradingRangeResult};

    fn bar(i: i64, c: f64) -> OhlcBar {
        OhlcBar {
            open: c,
            high: c + 0.5,
            low: c - 0.5,
            close: c,
            bar_index: i,
            volume: None,
        }
    }

    #[test]
    fn dashboard_runs() {
        let mut v: Vec<OhlcBar> = (0..120_i64)
            .map(|i| bar(i, 100.0 + (i as f64) * 0.01))
            .collect();
        let p = TradingRangeParams {
            lookback: 40,
            atr_period: 14,
            atr_sma_period: 20,
            require_range_regime: false,
            ..TradingRangeParams::default()
        };
        let tr = analyze_trading_range(&v, &p);
        let d = compute_signal_dashboard_v1(&v, &tr);
        assert_eq!(d.schema_version, 2);
        assert!(d.sistem_aktif);
        v[119] = bar(119, 102.0);
        let tr2 = analyze_trading_range(&v, &p);
        let _d2 = compute_signal_dashboard_v1(&v, &tr2);
    }

    #[test]
    fn long_only_neutralizes_short_side_but_keeps_raw() {
        let v: Vec<OhlcBar> = (0..120_i64)
            .map(|i| bar(i, 200.0 - (i as f64) * 0.8))
            .collect();
        let p = TradingRangeParams {
            lookback: 40,
            atr_period: 14,
            atr_sma_period: 20,
            require_range_regime: false,
            ..TradingRangeParams::default()
        };
        let tr = analyze_trading_range(&v, &p);
        let both = compute_signal_dashboard_v1_with_policy(&v, &tr, SignalDirectionPolicy::Both);
        let long_only =
            compute_signal_dashboard_v1_with_policy(&v, &tr, SignalDirectionPolicy::LongOnly);
        assert_eq!(both.durum_model_raw, long_only.durum_model_raw);
        if both.durum_model_raw == "SHORT" {
            assert_eq!(long_only.durum, "NOTR");
            if tr.short_sweep_signal {
                assert!(long_only.giris_gercek.is_none());
            }
        }
    }

    fn synthetic_tr_both_sweeps_short_wins() -> TradingRangeResult {
        TradingRangeResult {
            valid: true,
            reason: None,
            bar_count: 120,
            range_high: Some(2200.0),
            range_low: Some(2000.0),
            mid: Some(2100.0),
            is_range_regime: true,
            atr: Some(10.0),
            atr_sma: Some(10.0),
            window_bar_first: Some(0),
            window_bar_last: Some(119),
            last_bar_index: Some(119),
            long_sweep_signal: true,
            short_sweep_signal: true,
            long_sweep_latent: true,
            short_sweep_latent: true,
            support_touches: 2,
            resistance_touches: 2,
            close_breakout: false,
            range_width: Some(200.0),
            range_width_atr: Some(20.0),
            range_too_narrow: false,
            range_too_wide: false,
            wick_rejection_long: false,
            wick_rejection_short: false,
            fake_breakout_long: false,
            fake_breakout_short: false,
            setup_score_long: 50,
            setup_score_short: 60,
            setup_score_best: 60,
            guardrails_pass: true,
            setup_side: "SHORT".to_string(),
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
            range_zone: "mid".to_string(),
        }
    }

    /// Skor LONG dese de long süpürme yoksa üçlü üretilemez; etkin `durum` NOTR kalmalı (`durum_model_raw` LONG kalır).
    #[test]
    fn effective_durum_notr_when_long_model_without_executable_levels() {
        let mut tr = synthetic_tr_both_sweeps_short_wins();
        tr.setup_side = "LONG".to_string();
        tr.long_sweep_signal = false;
        tr.long_sweep_latent = false;
        let bars: Vec<OhlcBar> = (0..120_i64).map(|i| bar(i, 2100.0)).collect();
        let d = compute_signal_dashboard_v1_with_policy(&bars, &tr, SignalDirectionPolicy::Both);
        assert_eq!(d.durum_model_raw, "LONG");
        assert!(d.giris_gercek.is_none());
        assert_eq!(d.durum, "NOTR");
    }

    /// İki süpürme aynı anda ateşlendiğinde eski kod long dalını seçerdi; seviyeler `setup_side` ile hizalanmalı.
    #[test]
    fn setup_levels_follow_short_when_both_sweeps_and_short_wins() {
        let tr = synthetic_tr_both_sweeps_short_wins();
        let bars: Vec<OhlcBar> = (0..120_i64).map(|i| bar(i, 2100.0)).collect();
        let d = compute_signal_dashboard_v1_with_policy(&bars, &tr, SignalDirectionPolicy::Both);
        assert_eq!(d.durum, "SHORT");
        let e = d.giris_gercek.expect("entry");
        let sl = d.stop_ilk.expect("stop");
        let tp = d.kar_al_ilk.expect("tp");
        assert!(
            tp < e && e < sl,
            "SHORT geometry: tp={tp} entry={e} sl={sl}"
        );
        assert_eq!(
            d.kar_al_dinamik,
            d.kar_al_ilk,
            "dynamic TP should start from initial TP, not range mid"
        );
    }
}

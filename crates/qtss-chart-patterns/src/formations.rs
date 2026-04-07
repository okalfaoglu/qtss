//! Faz 2 — Klasik grafik formasyonları: Double Top/Bottom, Head & Shoulders,
//! Triple Top/Bottom, Flag/Pennant.
//!
//! Mevcut 6-pivot kanal/üçgen sistemi (ID 1–13) iki trend çizgisine dayalıdır.
//! Bu modül zigzag pivotlarından farklı geometrik kalıpları tespit eder.
//!
//! **Literatür filtreleri** ([`FormationParams`]): dönüş kalıplarında isteğe bağlı önceki trend,
//! boyun kırılımı ve kırılım hacmi; `bars` dilimi boşsa bu kontroller atlanır (pivot-only API uyumu).
//! [`FormationParams::default`] önceki trend / boyun / tekilleştirme / bayrak eğimi kapalıdır (gevşek tarama).
//! Worker ve analiz API [`FormationParams::literature_standard`] kullanır (klasik M/W, önceki trend,
//! çakışan formasyonlarda kaliteye göre tekilleştirme, bayrak karşı-trend pivot eğimi).
//!
//! Pattern IDs:
//!   14 = Double Top
//!   15 = Double Bottom
//!   16 = Head and Shoulders
//!   17 = Inverse Head and Shoulders
//!   18 = Triple Top
//!   19 = Triple Bottom
//!   20 = Bullish Flag
//!   21 = Bearish Flag

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::find::PivotTriple;
use crate::ohlc::OhlcBar;

/// Faz 2 formasyon tespit sonucu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormationMatch {
    /// Formasyon ID'si (14–21).
    pub pattern_type_id: i32,
    /// İnsan okunur formasyon adı.
    pub pattern_name: &'static str,
    /// Formasyonu oluşturan pivotlar (kronolojik).
    pub pivots: Vec<PivotTriple>,
    /// Boyun çizgisi (neckline) fiyatı — H&S ve Double/Triple için.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub neckline: Option<f64>,
    /// Formasyon yüksekliği (tepe-boyun arası mutlak fark).
    pub height: f64,
    /// Hedef fiyat (boyun çizgisinden yükseklik kadar projeksiyon).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_price: Option<f64>,
    /// Kalite skoru (0.0–1.0). Pivot simetri, fiyat yakınlığı, hacim gibi faktörlere göre.
    pub quality: f64,
    /// Faz 3: Hacim analizi (volume divergence, breakout teyidi, pivot volumes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_analysis: Option<crate::volume_analysis::FormationVolumeAnalysis>,
}

/// Formasyon tespiti için yapılandırma parametreleri.
#[derive(Debug, Clone)]
pub struct FormationParams {
    /// İki tepe/dip fiyatının birbirine eşit sayılması için maksimum yüzde farkı.
    /// Örnek: 0.012 ≈ %1.2.
    pub price_tolerance: f64,
    /// Double bottom: ikinci dip fiyatının birinciye göre alt sınırı (oran × ilk dip).
    /// - `None`: ek kısıt yok (yalnızca [`Self::price_tolerance`] ile iki dip yakınlığı).
    /// - `Some(1.0)`: klasik W — ikinci dip **birinci dipten düşük olamaz** (hafif yukarı / eşit kabul).
    /// - `Some(0.98)`: en fazla %2 “undercut” (stop avı) toleri.
    pub double_bottom_second_low_min_fraction_of_first: Option<f64>,
    /// Double top: ikinci tepe fiyatının birinciye göre üst sınırı (oran × ilk tepe).
    /// - `None`: ek kısıt yok.
    /// - `Some(1.0)`: ikinci tepe **birinci tepeden yüksek olamaz** (klasik M — “equal highs” / başarısız kırılım yok).
    /// - `Some(1.02)`: en fazla %2 ikinci tepe yüksekliği toleri.
    pub double_top_second_peak_max_fraction_of_first: Option<f64>,
    /// Flag formasyonlarında impulsive bacağın minimum bar sayısı.
    pub flag_min_pole_bars: usize,
    /// Flag formasyonlarında konsolidasyon alanının minimum bar sayısı.
    pub flag_min_flag_bars: usize,
    /// Flag bayrak gövdesi en fazla impulsive hareketin bu oranı kadar geri çekilebilir.
    pub flag_max_retrace: f64,
    // --- Literature-style filters (need `bars` in `detect_*` / `scan_formations`) ---
    /// Dönüş formasyonlarında, yapıdan önce destekleyici trend: tepe kalıplarında yükseliş, dip kalıplarında düşüş.
    /// `bars` boşsa veya referans bar indeksi için yeterli geçmiş yoksa kontrol atlanır (geriye dönük uyum).
    pub require_prior_trend: bool,
    /// Önceki trend için kapanışların karşılaştırılacağı mum sayısı (referans bara göre geriye).
    pub prior_trend_lookback_bars: usize,
    /// Referans kapanışın, lookback başındaki kapanışa göre minimum göreli farkı (örn. `0.002` ≈ %0.2).
    pub prior_trend_min_rel_move: f64,
    /// Boyun kırılımı: son mum kapanışı boyun çizgisinin “doğru” tarafında olmalı (teyit edilmiş dönüş).
    /// `bars` boşsa atlanır.
    pub require_neckline_break: bool,
    /// Boyun kırılımında fiyat toleransı (`neckline` ve kapanış büyüklüğüne göre göreli).
    pub neckline_break_epsilon_rel: f64,
    /// Boyun kırılımıyla birlikte son mum hacminin son `breakout_volume_lookback` mum ortalamasına göre yükselmesi.
    /// Hacim verisi yetersizse kontrol atlanır.
    pub require_breakout_volume_spike: bool,
    pub breakout_volume_lookback: usize,
    pub breakout_volume_ratio: f64,
    /// Ortak pivot barı olan eşleşmelerde kaliteye göre açgözlü tekilleştirme (üst üste üçlü+çift tepe vb.).
    pub dedupe_overlapping: bool,
    /// Bayrak konsolidasyonunda karşı-trend eğim: bull flag’te ardışık **tepe** pivotları zayıflamalı;
    /// bear flag’te ardışık **dip** pivotları yükselmeli (literatürdeki dar karşı kanal).
    pub flag_require_countertrend_pivot_slope: bool,
}

impl Default for FormationParams {
    fn default() -> Self {
        Self {
            price_tolerance: 0.012,
            double_bottom_second_low_min_fraction_of_first: None,
            double_top_second_peak_max_fraction_of_first: None,
            flag_min_pole_bars: 3,
            flag_min_flag_bars: 3,
            flag_max_retrace: 0.618,
            require_prior_trend: false,
            prior_trend_lookback_bars: 20,
            prior_trend_min_rel_move: 0.002,
            require_neckline_break: false,
            neckline_break_epsilon_rel: 0.0004,
            require_breakout_volume_spike: false,
            breakout_volume_lookback: 20,
            breakout_volume_ratio: 1.15,
            dedupe_overlapping: false,
            flag_require_countertrend_pivot_slope: false,
        }
    }
}

impl FormationParams {
    /// Motor / API için: klasik M/W (ikinci tepe birinciyi aşmaz, ikinci dip birincinin altına inmez) + önceki trend filtresi açık.
    #[must_use]
    pub fn literature_standard() -> Self {
        Self {
            double_top_second_peak_max_fraction_of_first: Some(1.0),
            double_bottom_second_low_min_fraction_of_first: Some(1.0),
            require_prior_trend: true,
            require_neckline_break: false,
            dedupe_overlapping: true,
            flag_require_countertrend_pivot_slope: true,
            ..Self::default()
        }
    }

    /// Boyun kırılımı teyidi (Bulkowski / klasik “completed pattern”).
    #[must_use]
    pub fn with_literature_neckline_confirmation(mut self) -> Self {
        self.require_neckline_break = true;
        self
    }

    /// Boyun kırılımında hacim artışı (son mum hacmi > ortalama × oran).
    #[must_use]
    pub fn with_literature_breakout_volume(mut self) -> Self {
        self.require_breakout_volume_spike = true;
        self
    }

    /// Classical double-bottom lows: second trough must not print below the first (`b2 >= b1`).
    #[must_use]
    pub fn with_strict_double_bottom_lows(mut self) -> Self {
        self.double_bottom_second_low_min_fraction_of_first = Some(1.0);
        self
    }

    /// Classical double-top highs: second peak must not exceed the first (`t2 <= t1`).
    #[must_use]
    pub fn with_strict_double_top_peaks(mut self) -> Self {
        self.double_top_second_peak_max_fraction_of_first = Some(1.0);
        self
    }
}

// ─── Yardımcı fonksiyonlar ─────────────────────────────────────────

/// İki fiyat arasındaki yüzde farkı (mutlak).
#[inline]
fn pct_diff(a: f64, b: f64) -> f64 {
    let avg = (a + b).abs() / 2.0;
    if avg < 1e-15 {
        return 0.0;
    }
    (a - b).abs() / avg
}

/// Boyun / dip–tepe eşlemesinde mutlak `1e-9` büyük fiyatlarda yetersiz; göreli + mutlak karışık tolerans.
#[inline]
fn approx_same_price(a: f64, b: f64) -> bool {
    let m = a.abs().max(b.abs()).max(1.0);
    (a - b).abs() <= m * 1e-10_f64 || (a - b).abs() <= 1e-6_f64
}

#[inline]
fn top_pattern_extremes_valid(peak_avg: f64, neckline: f64) -> bool {
    let m = peak_avg.abs().max(neckline.abs()).max(1.0);
    peak_avg > neckline + m * 1e-12_f64
}

#[inline]
fn bottom_pattern_extremes_valid(trough_avg: f64, neckline: f64) -> bool {
    let m = trough_avg.abs().max(neckline.abs()).max(1.0);
    trough_avg < neckline - m * 1e-12_f64
}

/// Pivotlardan sadece tepe (dir > 0) olanları filtreler.
fn highs(pivots: &[PivotTriple]) -> Vec<PivotTriple> {
    pivots.iter().copied().filter(|(_, _, d)| *d > 0).collect()
}

/// Pivotlardan sadece dip (dir < 0) olanları filtreler.
fn lows(pivots: &[PivotTriple]) -> Vec<PivotTriple> {
    pivots.iter().copied().filter(|(_, _, d)| *d < 0).collect()
}

#[derive(Clone, Copy)]
enum ReversalFamily {
    /// Çift/üçlü tepe, omuz-baş-omuz: önce yükseliş; teyitte kapanış boyunun altı.
    Top { first_high_bar: i64 },
    /// Çift/üçlü dip, ters omuz-baş-omuz: önce düşüş; teyitte kapanış boyunun üstü.
    Bottom { first_low_bar: i64 },
}

fn bar_index_to_pos(bars: &[OhlcBar], bar_idx: i64) -> Option<usize> {
    bars.iter().position(|b| b.bar_index == bar_idx)
}

/// Tepe formasyonundan önce: referans tepe barındaki kapanış, `lookback` önceki kapanıştan en az `min_rel_move` kadar yüksek.
fn prior_uptrend_ok(bars: &[OhlcBar], end_bar_idx: i64, lookback: usize, min_rel_move: f64) -> bool {
    let Some(end_pos) = bar_index_to_pos(bars, end_bar_idx) else {
        return true;
    };
    if end_pos < lookback {
        return true;
    }
    let start_pos = end_pos - lookback;
    let c_end = bars[end_pos].close;
    let c_start = bars[start_pos].close;
    if !c_end.is_finite() || !c_start.is_finite() || c_start.abs() < 1e-12 {
        return true;
    }
    c_end >= c_start * (1.0 + min_rel_move.max(0.0))
}

/// Dip formasyonundan önce: referans dip barındaki kapanış, lookback öncesine göre en az `min_rel_move` kadar düşük.
fn prior_downtrend_ok(bars: &[OhlcBar], end_bar_idx: i64, lookback: usize, min_rel_move: f64) -> bool {
    let Some(end_pos) = bar_index_to_pos(bars, end_bar_idx) else {
        return true;
    };
    if end_pos < lookback {
        return true;
    }
    let start_pos = end_pos - lookback;
    let c_end = bars[end_pos].close;
    let c_start = bars[start_pos].close;
    if !c_end.is_finite() || !c_start.is_finite() || c_start.abs() < 1e-12 {
        return true;
    }
    c_end <= c_start * (1.0 - min_rel_move.max(0.0))
}

fn neckline_break_bearish(bars: &[OhlcBar], neckline: f64, epsilon_rel: f64) -> bool {
    let Some(last) = bars.last() else {
        return false;
    };
    let c = last.close;
    if !c.is_finite() || !neckline.is_finite() {
        return false;
    }
    let m = neckline.abs().max(c.abs()).max(1.0);
    let tol = m * epsilon_rel.max(1e-12);
    c < neckline - tol
}

fn neckline_break_bullish(bars: &[OhlcBar], neckline: f64, epsilon_rel: f64) -> bool {
    let Some(last) = bars.last() else {
        return false;
    };
    let c = last.close;
    if !c.is_finite() || !neckline.is_finite() {
        return false;
    }
    let m = neckline.abs().max(c.abs()).max(1.0);
    let tol = m * epsilon_rel.max(1e-12);
    c > neckline + tol
}

fn breakout_volume_spike_ok(bars: &[OhlcBar], lookback: usize, ratio: f64) -> bool {
    let n = bars.len();
    let lb = lookback.max(3);
    if n < lb + 1 {
        return true;
    }
    let last = &bars[n - 1];
    let Some(lv) = last.volume.filter(|v| v.is_finite() && *v >= 0.0) else {
        return true;
    };
    let start = n.saturating_sub(lb + 1);
    let mut sum = 0.0_f64;
    let mut cnt = 0usize;
    for b in &bars[start..n - 1] {
        if let Some(v) = b.volume {
            if v.is_finite() && v >= 0.0 {
                sum += v;
                cnt += 1;
            }
        }
    }
    let min_need = (lb * 7 / 10).max(3).min(lb);
    if cnt < min_need {
        return true;
    }
    let avg = sum / cnt as f64;
    if avg <= 1e-18 {
        return true;
    }
    lv >= avg * ratio.max(1.0)
}

fn literature_reversal_filters_pass(
    bars: &[OhlcBar],
    params: &FormationParams,
    neckline: f64,
    family: ReversalFamily,
) -> bool {
    if bars.is_empty() {
        return true;
    }
    let look = params.prior_trend_lookback_bars.max(1);
    let mv = params.prior_trend_min_rel_move;

    if params.require_prior_trend {
        let ok = match family {
            ReversalFamily::Top { first_high_bar } => {
                prior_uptrend_ok(bars, first_high_bar, look, mv)
            }
            ReversalFamily::Bottom { first_low_bar } => {
                prior_downtrend_ok(bars, first_low_bar, look, mv)
            }
        };
        if !ok {
            return false;
        }
    }

    if params.require_neckline_break {
        let eps = params.neckline_break_epsilon_rel;
        let ok = match family {
            ReversalFamily::Top { .. } => neckline_break_bearish(bars, neckline, eps),
            ReversalFamily::Bottom { .. } => neckline_break_bullish(bars, neckline, eps),
        };
        if !ok {
            return false;
        }
    }

    if params.require_breakout_volume_spike {
        if !breakout_volume_spike_ok(
            bars,
            params.breakout_volume_lookback.max(3),
            params.breakout_volume_ratio,
        ) {
            return false;
        }
    }

    true
}

/// Bull flag: konsolidasyondaki tepe pivotları zayıflamalı (ardışık yükselen tepe yok).
fn flag_bull_highs_non_rising(flag_pivots: &[PivotTriple], ref_price: f64, price_tol: f64) -> bool {
    let peaks: Vec<f64> = flag_pivots
        .iter()
        .filter(|(_, _, d)| *d > 0)
        .map(|(_, p, _)| *p)
        .collect();
    if peaks.len() < 2 {
        return true;
    }
    let tol = ref_price.abs().max(1.0) * price_tol;
    for w in peaks.windows(2) {
        if w[1] > w[0] + tol {
            return false;
        }
    }
    true
}

/// Bear flag: konsolidasyondaki dip pivotları genelde yükselir (ardışık daha düşük dip yok).
fn flag_bear_lows_non_falling(flag_pivots: &[PivotTriple], ref_price: f64, price_tol: f64) -> bool {
    let troughs: Vec<f64> = flag_pivots
        .iter()
        .filter(|(_, _, d)| *d < 0)
        .map(|(_, p, _)| *p)
        .collect();
    if troughs.len() < 2 {
        return true;
    }
    let tol = ref_price.abs().max(1.0) * price_tol;
    for w in troughs.windows(2) {
        if w[1] < w[0] - tol {
            return false;
        }
    }
    true
}

fn formation_shares_pivot_bar(a: &FormationMatch, b: &FormationMatch) -> bool {
    let bars_a: HashSet<i64> = a.pivots.iter().map(|(ix, _, _)| *ix).collect();
    b.pivots.iter().any(|(ix, _, _)| bars_a.contains(ix))
}

fn dedupe_formations_greedy_by_pivot_overlap(mut v: Vec<FormationMatch>) -> Vec<FormationMatch> {
    v.sort_by(|a, b| {
        b.quality
            .partial_cmp(&a.quality)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut out: Vec<FormationMatch> = Vec::new();
    for m in v {
        if out.iter().any(|k| formation_shares_pivot_bar(k, &m)) {
            continue;
        }
        out.push(m);
    }
    out
}

/// Simetri skoru: iki pivot arasındaki bar mesafesinin birbirine yakınlığı (0–1).
fn symmetry_score(bar_diffs: &[i64]) -> f64 {
    if bar_diffs.len() < 2 {
        return 1.0;
    }
    let avg = bar_diffs.iter().sum::<i64>() as f64 / bar_diffs.len() as f64;
    if avg < 1.0 {
        return 1.0;
    }
    let max_dev = bar_diffs
        .iter()
        .map(|d| (*d as f64 - avg).abs())
        .fold(0.0_f64, f64::max);
    (1.0 - max_dev / avg).clamp(0.0, 1.0)
}

// ─── Double Top (ID 14) ────────────────────────────────────────────

/// Double Top tespiti: Son pivotlarda iki benzer yükseklikte tepe + aralarında bir dip.
///
/// Gereksinimler:
/// - En az 3 pivot: H-L-H (tepe-dip-tepe)
/// - İki tepe fiyatı `price_tolerance` dahilinde yakın
/// - Ortalama tepe fiyatı boyun çizgisinden anlamlı şekilde yüksek (ters geometri elenir)
/// - Neckline = aradaki dip(ler)in en düşüğü
/// - Hedef = neckline - (tepe_ortalaması − neckline)
/// - İsteğe bağlı: [`FormationParams::double_top_second_peak_max_fraction_of_first`] ile ikinci tepenin birinciyi aşmaması (klasik M)
#[must_use]
pub fn detect_double_top(
    pivots: &[PivotTriple],
    bars: &[OhlcBar],
    params: &FormationParams,
) -> Option<FormationMatch> {
    if pivots.len() < 3 {
        return None;
    }
    // Son 5 pivottan arayalım (geniş pencere)
    let window = if pivots.len() > 5 {
        &pivots[pivots.len() - 5..]
    } else {
        pivots
    };

    let h = highs(window);
    let l = lows(window);
    if h.len() < 2 || l.is_empty() {
        return None;
    }

    // Son iki tepe
    let t1 = h[h.len() - 2];
    let t2 = h[h.len() - 1];

    // Arada en az bir dip olmalı
    let between_lows: Vec<_> = l
        .iter()
        .copied()
        .filter(|(b, _, _)| *b > t1.0 && *b < t2.0)
        .collect();
    if between_lows.is_empty() {
        return None;
    }

    let diff = pct_diff(t1.1, t2.1);
    if diff > params.price_tolerance {
        return None;
    }

    if let Some(frac) = params.double_top_second_peak_max_fraction_of_first {
        if t1.1 <= 1e-15 || !frac.is_finite() || frac <= 0.0 {
            return None;
        }
        let max_second = t1.1 * frac;
        let eps = t1.1.abs().max(1.0) * 1e-12_f64;
        if t2.1 > max_second + eps {
            return None;
        }
    }

    let neckline = between_lows.iter().map(|(_, p, _)| *p).fold(f64::MAX, f64::min);
    let peak_avg = (t1.1 + t2.1) / 2.0;
    if !top_pattern_extremes_valid(peak_avg, neckline) {
        return None;
    }
    let height = (peak_avg - neckline).abs();
    let target = neckline - height;

    // Kalite: fiyat yakınlığı + simetri (tepe1→boyun ve boyun→tepe2 bar mesafeleri; tek skaler
    // `symmetry_score` ile anlamsız kalıyordu — en düşük dip barını referans al).
    let price_q = 1.0 - (diff / params.price_tolerance).min(1.0);
    let trough_bar = between_lows
        .iter()
        .filter(|(_, p, _)| approx_same_price(*p, neckline))
        .map(|(b, _, _)| *b)
        .min()
        .unwrap_or_else(|| {
            between_lows
                .iter()
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(b, _, _)| *b)
                .unwrap_or(t1.0)
        });
    let sym = symmetry_score(&[trough_bar - t1.0, t2.0 - trough_bar]);
    let quality = (price_q * 0.7 + sym * 0.3).clamp(0.0, 1.0);

    if !literature_reversal_filters_pass(
        bars,
        params,
        neckline,
        ReversalFamily::Top {
            first_high_bar: t1.0,
        },
    ) {
        return None;
    }

    let mut fpivots = vec![t1];
    fpivots.extend_from_slice(&between_lows);
    fpivots.push(t2);

    Some(FormationMatch {
        pattern_type_id: 14,
        pattern_name: "Double Top",
        pivots: fpivots,
        neckline: Some(neckline),
        height,
        target_price: Some(target),
        quality,
        volume_analysis: None,
    })
}

// ─── Double Bottom (ID 15) ─────────────────────────────────────────

/// Double Bottom: İki benzer dip + aralarında bir tepe.
///
/// Klasik literatürde ikinci dip genelde birinciyle aynı bölgede veya **biraz yukarıda** (higher low);
/// ikinci dipten birincinin anlamlı şekilde aşağı kırılması çoğu tanımda saf çift dip sayılmaz.
/// Bunu zorunlu kılmak için [`FormationParams::double_bottom_second_low_min_fraction_of_first`]
/// veya [`FormationParams::with_strict_double_bottom_lows`] kullanın.
#[must_use]
pub fn detect_double_bottom(
    pivots: &[PivotTriple],
    bars: &[OhlcBar],
    params: &FormationParams,
) -> Option<FormationMatch> {
    if pivots.len() < 3 {
        return None;
    }
    let window = if pivots.len() > 5 {
        &pivots[pivots.len() - 5..]
    } else {
        pivots
    };

    let h = highs(window);
    let l = lows(window);
    if l.len() < 2 || h.is_empty() {
        return None;
    }

    let b1 = l[l.len() - 2];
    let b2 = l[l.len() - 1];

    let between_highs: Vec<_> = h
        .iter()
        .copied()
        .filter(|(b, _, _)| *b > b1.0 && *b < b2.0)
        .collect();
    if between_highs.is_empty() {
        return None;
    }

    let diff = pct_diff(b1.1, b2.1);
    if diff > params.price_tolerance {
        return None;
    }

    if let Some(frac) = params.double_bottom_second_low_min_fraction_of_first {
        if b1.1 <= 1e-15 || !frac.is_finite() || frac <= 0.0 {
            return None;
        }
        let min_second = b1.1 * frac;
        if b2.1 + 1e-12 < min_second {
            return None;
        }
    }

    let neckline = between_highs
        .iter()
        .map(|(_, p, _)| *p)
        .fold(f64::MIN, f64::max);
    let trough_avg = (b1.1 + b2.1) / 2.0;
    if !bottom_pattern_extremes_valid(trough_avg, neckline) {
        return None;
    }
    let height = (neckline - trough_avg).abs();
    let target = neckline + height;

    let price_q = 1.0 - (diff / params.price_tolerance).min(1.0);
    let peak_bar = between_highs
        .iter()
        .filter(|(_, p, _)| approx_same_price(*p, neckline))
        .map(|(b, _, _)| *b)
        .max()
        .unwrap_or_else(|| {
            between_highs
                .iter()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(b, _, _)| *b)
                .unwrap_or(b1.0)
        });
    let sym = symmetry_score(&[peak_bar - b1.0, b2.0 - peak_bar]);
    let quality = (price_q * 0.7 + sym * 0.3).clamp(0.0, 1.0);

    if !literature_reversal_filters_pass(
        bars,
        params,
        neckline,
        ReversalFamily::Bottom {
            first_low_bar: b1.0,
        },
    ) {
        return None;
    }

    let mut fpivots = vec![b1];
    fpivots.extend_from_slice(&between_highs);
    fpivots.push(b2);

    Some(FormationMatch {
        pattern_type_id: 15,
        pattern_name: "Double Bottom",
        pivots: fpivots,
        neckline: Some(neckline),
        height,
        target_price: Some(target),
        quality,
        volume_analysis: None,
    })
}

// ─── Head and Shoulders (ID 16) ────────────────────────────────────

/// Head and Shoulders: Sol omuz (H) – Baş (daha yüksek H) – Sağ omuz (H).
/// Aralarında iki dip → neckline.
///
/// 5 pivot gerekir: H-L-H-L-H (omuz-dip-baş-dip-omuz).
/// Baş > her iki omuz, iki dip benzer → neckline.
#[must_use]
pub fn detect_head_and_shoulders(
    pivots: &[PivotTriple],
    bars: &[OhlcBar],
    params: &FormationParams,
) -> Option<FormationMatch> {
    if pivots.len() < 5 {
        return None;
    }
    let window = if pivots.len() > 7 {
        &pivots[pivots.len() - 7..]
    } else {
        pivots
    };

    let h = highs(window);
    let l = lows(window);
    if h.len() < 3 || l.len() < 2 {
        return None;
    }

    // Son 3 tepe ve son 2 dip
    let ls = h[h.len() - 3]; // left shoulder
    let head = h[h.len() - 2]; // head
    let rs = h[h.len() - 1]; // right shoulder

    // Baş her iki omuzdan yüksek olmalı
    if head.1 <= ls.1 || head.1 <= rs.1 {
        return None;
    }

    // İki omuzun simetrik olması tercih edilir (ama zorunlu değil)
    let shoulder_diff = pct_diff(ls.1, rs.1);
    if shoulder_diff > params.price_tolerance * 2.0 {
        return None;
    }

    // Neckline: baş ile omuzlar arasındaki iki dip
    let left_troughs: Vec<_> = l
        .iter()
        .copied()
        .filter(|(b, _, _)| *b > ls.0 && *b < head.0)
        .collect();
    let right_troughs: Vec<_> = l
        .iter()
        .copied()
        .filter(|(b, _, _)| *b > head.0 && *b < rs.0)
        .collect();

    if left_troughs.is_empty() || right_troughs.is_empty() {
        return None;
    }

    let lt_price = left_troughs.iter().map(|(_, p, _)| *p).fold(f64::MAX, f64::min);
    let rt_price = right_troughs.iter().map(|(_, p, _)| *p).fold(f64::MAX, f64::min);
    let neckline = (lt_price + rt_price) / 2.0;

    let height = (head.1 - neckline).abs();
    let target = neckline - height;

    // Kalite: baş belirginliği + omuz simetrisi + dip simetrisi
    let head_prominence = ((head.1 - ls.1.max(rs.1)) / head.1).clamp(0.0, 0.5) * 2.0;
    let shoulder_q = 1.0 - (shoulder_diff / (params.price_tolerance * 2.0)).min(1.0);
    let trough_q = 1.0 - pct_diff(lt_price, rt_price).min(0.1) / 0.1;
    let bar_diffs = [head.0 - ls.0, rs.0 - head.0];
    let sym = symmetry_score(&bar_diffs);

    let quality = (head_prominence * 0.3 + shoulder_q * 0.25 + trough_q * 0.2 + sym * 0.25)
        .clamp(0.0, 1.0);

    if !literature_reversal_filters_pass(
        bars,
        params,
        neckline,
        ReversalFamily::Top {
            first_high_bar: ls.0,
        },
    ) {
        return None;
    }

    let mut fpivots = vec![ls];
    fpivots.extend(left_troughs.iter().copied());
    fpivots.push(head);
    fpivots.extend(right_troughs.iter().copied());
    fpivots.push(rs);

    Some(FormationMatch {
        pattern_type_id: 16,
        pattern_name: "Head and Shoulders",
        pivots: fpivots,
        neckline: Some(neckline),
        height,
        target_price: Some(target),
        quality,
        volume_analysis: None,
    })
}

// ─── Inverse Head and Shoulders (ID 17) ────────────────────────────

/// Inverse H&S: Sol omuz (L) – Baş (daha düşük L) – Sağ omuz (L).
/// Aralarında iki tepe → neckline.
#[must_use]
pub fn detect_inverse_head_and_shoulders(
    pivots: &[PivotTriple],
    bars: &[OhlcBar],
    params: &FormationParams,
) -> Option<FormationMatch> {
    if pivots.len() < 5 {
        return None;
    }
    let window = if pivots.len() > 7 {
        &pivots[pivots.len() - 7..]
    } else {
        pivots
    };

    let h = highs(window);
    let l = lows(window);
    if l.len() < 3 || h.len() < 2 {
        return None;
    }

    let ls = l[l.len() - 3]; // left shoulder
    let head = l[l.len() - 2]; // head (lowest)
    let rs = l[l.len() - 1]; // right shoulder

    // Baş her iki omuzdan düşük olmalı
    if head.1 >= ls.1 || head.1 >= rs.1 {
        return None;
    }

    let shoulder_diff = pct_diff(ls.1, rs.1);
    if shoulder_diff > params.price_tolerance * 2.0 {
        return None;
    }

    // Neckline: aralarındaki iki tepe
    let left_peaks: Vec<_> = h
        .iter()
        .copied()
        .filter(|(b, _, _)| *b > ls.0 && *b < head.0)
        .collect();
    let right_peaks: Vec<_> = h
        .iter()
        .copied()
        .filter(|(b, _, _)| *b > head.0 && *b < rs.0)
        .collect();

    if left_peaks.is_empty() || right_peaks.is_empty() {
        return None;
    }

    let lp_price = left_peaks.iter().map(|(_, p, _)| *p).fold(f64::MIN, f64::max);
    let rp_price = right_peaks.iter().map(|(_, p, _)| *p).fold(f64::MIN, f64::max);
    let neckline = (lp_price + rp_price) / 2.0;

    let height = (neckline - head.1).abs();
    let target = neckline + height;

    let head_prominence = ((ls.1.min(rs.1) - head.1) / ls.1.min(rs.1).abs().max(1e-15))
        .clamp(0.0, 0.5)
        * 2.0;
    let shoulder_q = 1.0 - (shoulder_diff / (params.price_tolerance * 2.0)).min(1.0);
    let peak_q = 1.0 - pct_diff(lp_price, rp_price).min(0.1) / 0.1;
    let bar_diffs = [head.0 - ls.0, rs.0 - head.0];
    let sym = symmetry_score(&bar_diffs);

    let quality =
        (head_prominence * 0.3 + shoulder_q * 0.25 + peak_q * 0.2 + sym * 0.25).clamp(0.0, 1.0);

    if !literature_reversal_filters_pass(
        bars,
        params,
        neckline,
        ReversalFamily::Bottom {
            first_low_bar: ls.0,
        },
    ) {
        return None;
    }

    let mut fpivots = vec![ls];
    fpivots.extend(left_peaks.iter().copied());
    fpivots.push(head);
    fpivots.extend(right_peaks.iter().copied());
    fpivots.push(rs);

    Some(FormationMatch {
        pattern_type_id: 17,
        pattern_name: "Inverse Head and Shoulders",
        pivots: fpivots,
        neckline: Some(neckline),
        height,
        target_price: Some(target),
        quality,
        volume_analysis: None,
    })
}

// ─── Triple Top (ID 18) ───────────────────────────────────────────

/// Triple Top: Üç benzer yükseklikte tepe + aralarında iki dip bölgesi.
/// Boyun: iki vadinin yerel minimumlarının daha düşüğü; tepe ortalaması boyundan yüksek olmalıdır.
#[must_use]
pub fn detect_triple_top(
    pivots: &[PivotTriple],
    bars: &[OhlcBar],
    params: &FormationParams,
) -> Option<FormationMatch> {
    if pivots.len() < 5 {
        return None;
    }
    let window = if pivots.len() > 7 {
        &pivots[pivots.len() - 7..]
    } else {
        pivots
    };

    let h = highs(window);
    let l = lows(window);
    if h.len() < 3 || l.len() < 2 {
        return None;
    }

    let t1 = h[h.len() - 3];
    let t2 = h[h.len() - 2];
    let t3 = h[h.len() - 1];

    // Üç tepe birbirine yakın olmalı
    let d12 = pct_diff(t1.1, t2.1);
    let d23 = pct_diff(t2.1, t3.1);
    let d13 = pct_diff(t1.1, t3.1);
    if d12 > params.price_tolerance || d23 > params.price_tolerance || d13 > params.price_tolerance
    {
        return None;
    }

    // İki dip: t1-t2 arası ve t2-t3 arası
    let l1: Vec<_> = l
        .iter()
        .copied()
        .filter(|(b, _, _)| *b > t1.0 && *b < t2.0)
        .collect();
    let l2: Vec<_> = l
        .iter()
        .copied()
        .filter(|(b, _, _)| *b > t2.0 && *b < t3.0)
        .collect();
    if l1.is_empty() || l2.is_empty() {
        return None;
    }

    let l1_min = l1.iter().map(|(_, p, _)| *p).fold(f64::MAX, f64::min);
    let l2_min = l2.iter().map(|(_, p, _)| *p).fold(f64::MAX, f64::min);
    let neckline = l1_min.min(l2_min);
    let peak_avg = (t1.1 + t2.1 + t3.1) / 3.0;
    if !top_pattern_extremes_valid(peak_avg, neckline) {
        return None;
    }
    let height = (peak_avg - neckline).abs();
    let target = neckline - height;

    let price_q = 1.0 - ((d12 + d23 + d13) / 3.0 / params.price_tolerance).min(1.0);
    let bar_diffs = [t2.0 - t1.0, t3.0 - t2.0];
    let sym = symmetry_score(&bar_diffs);
    let quality = (price_q * 0.6 + sym * 0.4).clamp(0.0, 1.0);

    if !literature_reversal_filters_pass(
        bars,
        params,
        neckline,
        ReversalFamily::Top {
            first_high_bar: t1.0,
        },
    ) {
        return None;
    }

    let mut fpivots = vec![t1];
    fpivots.extend(l1.iter().copied());
    fpivots.push(t2);
    fpivots.extend(l2.iter().copied());
    fpivots.push(t3);

    Some(FormationMatch {
        pattern_type_id: 18,
        pattern_name: "Triple Top",
        pivots: fpivots,
        neckline: Some(neckline),
        height,
        target_price: Some(target),
        quality,
        volume_analysis: None,
    })
}

// ─── Triple Bottom (ID 19) ─────────────────────────────────────────

/// Triple Bottom: Üç benzer dip + aralarında iki tepe bölgesi.
/// Boyun: iki tepenin yerel maksimumlarının daha yükseği; dip ortalaması boyundan düşük olmalıdır.
#[must_use]
pub fn detect_triple_bottom(
    pivots: &[PivotTriple],
    bars: &[OhlcBar],
    params: &FormationParams,
) -> Option<FormationMatch> {
    if pivots.len() < 5 {
        return None;
    }
    let window = if pivots.len() > 7 {
        &pivots[pivots.len() - 7..]
    } else {
        pivots
    };

    let h = highs(window);
    let l = lows(window);
    if l.len() < 3 || h.len() < 2 {
        return None;
    }

    let b1 = l[l.len() - 3];
    let b2 = l[l.len() - 2];
    let b3 = l[l.len() - 1];

    let d12 = pct_diff(b1.1, b2.1);
    let d23 = pct_diff(b2.1, b3.1);
    let d13 = pct_diff(b1.1, b3.1);
    if d12 > params.price_tolerance || d23 > params.price_tolerance || d13 > params.price_tolerance
    {
        return None;
    }

    let h1: Vec<_> = h
        .iter()
        .copied()
        .filter(|(b, _, _)| *b > b1.0 && *b < b2.0)
        .collect();
    let h2: Vec<_> = h
        .iter()
        .copied()
        .filter(|(b, _, _)| *b > b2.0 && *b < b3.0)
        .collect();
    if h1.is_empty() || h2.is_empty() {
        return None;
    }

    let h1_max = h1.iter().map(|(_, p, _)| *p).fold(f64::MIN, f64::max);
    let h2_max = h2.iter().map(|(_, p, _)| *p).fold(f64::MIN, f64::max);
    let neckline = h1_max.max(h2_max);
    let trough_avg = (b1.1 + b2.1 + b3.1) / 3.0;
    if !bottom_pattern_extremes_valid(trough_avg, neckline) {
        return None;
    }
    let height = (neckline - trough_avg).abs();
    let target = neckline + height;

    let price_q = 1.0 - ((d12 + d23 + d13) / 3.0 / params.price_tolerance).min(1.0);
    let bar_diffs = [b2.0 - b1.0, b3.0 - b2.0];
    let sym = symmetry_score(&bar_diffs);
    let quality = (price_q * 0.6 + sym * 0.4).clamp(0.0, 1.0);

    if !literature_reversal_filters_pass(
        bars,
        params,
        neckline,
        ReversalFamily::Bottom {
            first_low_bar: b1.0,
        },
    ) {
        return None;
    }

    let mut fpivots = vec![b1];
    fpivots.extend(h1.iter().copied());
    fpivots.push(b2);
    fpivots.extend(h2.iter().copied());
    fpivots.push(b3);

    Some(FormationMatch {
        pattern_type_id: 19,
        pattern_name: "Triple Bottom",
        pivots: fpivots,
        neckline: Some(neckline),
        height,
        target_price: Some(target),
        quality,
        volume_analysis: None,
    })
}

// ─── Bullish Flag (ID 20) ──────────────────────────────────────────

/// Bullish Flag: Güçlü yukarı hareket (pole) + aşağı eğimli dar konsolidasyon (flag).
///
/// Tespit: pivotlardan son impulsive up move + ardından gelen küçük geri çekilme.
#[must_use]
pub fn detect_bullish_flag(
    pivots: &[PivotTriple],
    bars: &[OhlcBar],
    params: &FormationParams,
) -> Option<FormationMatch> {
    if pivots.len() < 4 || bars.len() < 10 {
        return None;
    }
    let window = if pivots.len() > 6 {
        &pivots[pivots.len() - 6..]
    } else {
        pivots
    };

    let h = highs(window);
    let l = lows(window);
    if h.is_empty() || l.len() < 1 {
        return None;
    }

    // Pole: en yüksek tepeyi bul, ondan önceki son dibi pole_bottom olarak al
    let pole_top = *h.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())?;
    let pole_bottom = l
        .iter()
        .copied()
        .filter(|(b, _, _)| *b < pole_top.0)
        .last()?;

    let pole_height = pole_top.1 - pole_bottom.1;
    if pole_height <= 0.0 {
        return None;
    }
    let pole_bars = pole_top.0 - pole_bottom.0;
    if pole_bars < params.flag_min_pole_bars as i64 {
        return None;
    }

    // Flag: pole_top'tan sonraki pivotlar — hafif aşağı geri çekilme
    let flag_pivots: Vec<_> = window
        .iter()
        .copied()
        .filter(|(b, _, _)| *b > pole_top.0)
        .collect();
    if flag_pivots.len() < 2 {
        return None;
    }
    let flag_bars = flag_pivots.last().unwrap().0 - pole_top.0;
    if flag_bars < params.flag_min_flag_bars as i64 {
        return None;
    }

    // Flag içindeki minimum fiyat — geri çekilme kontrolü
    let flag_low = flag_pivots
        .iter()
        .map(|(_, p, _)| *p)
        .fold(f64::MAX, f64::min);
    let retrace = (pole_top.1 - flag_low) / pole_height;
    if retrace > params.flag_max_retrace {
        return None;
    }

    if params.flag_require_countertrend_pivot_slope
        && !flag_bull_highs_non_rising(&flag_pivots, pole_top.1, params.price_tolerance)
    {
        return None;
    }

    let target = pole_top.1 + pole_height; // measured move

    // Kalite: küçük retrace = daha iyi
    let retrace_q = (1.0 - retrace / params.flag_max_retrace).clamp(0.0, 1.0);
    let quality = retrace_q;

    let mut fpivots = vec![pole_bottom, pole_top];
    fpivots.extend(flag_pivots.iter().copied().skip(1)); // pole_top zaten var

    Some(FormationMatch {
        pattern_type_id: 20,
        pattern_name: "Bullish Flag",
        pivots: fpivots,
        neckline: None,
        height: pole_height,
        target_price: Some(target),
        quality,
        volume_analysis: None,
    })
}

// ─── Bearish Flag (ID 21) ──────────────────────────────────────────

/// Bearish Flag: Güçlü aşağı hareket (pole) + yukarı eğimli dar konsolidasyon (flag).
#[must_use]
pub fn detect_bearish_flag(
    pivots: &[PivotTriple],
    bars: &[OhlcBar],
    params: &FormationParams,
) -> Option<FormationMatch> {
    if pivots.len() < 4 || bars.len() < 10 {
        return None;
    }
    let window = if pivots.len() > 6 {
        &pivots[pivots.len() - 6..]
    } else {
        pivots
    };

    let h = highs(window);
    let l = lows(window);
    if l.is_empty() || h.is_empty() {
        return None;
    }

    // Pole: en düşük dibi bul, ondan önceki son tepeyi pole_top olarak al
    let pole_bottom = *l.iter().min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())?;
    let pole_top = h
        .iter()
        .copied()
        .filter(|(b, _, _)| *b < pole_bottom.0)
        .last()?;

    let pole_height = pole_top.1 - pole_bottom.1;
    if pole_height <= 0.0 {
        return None;
    }
    let pole_bars = pole_bottom.0 - pole_top.0;
    if pole_bars < params.flag_min_pole_bars as i64 {
        return None;
    }

    // Flag: pole_bottom'dan sonraki pivotlar — hafif yukarı geri çekilme
    let flag_pivots: Vec<_> = window
        .iter()
        .copied()
        .filter(|(b, _, _)| *b > pole_bottom.0)
        .collect();
    if flag_pivots.len() < 2 {
        return None;
    }
    let flag_bars = flag_pivots.last().unwrap().0 - pole_bottom.0;
    if flag_bars < params.flag_min_flag_bars as i64 {
        return None;
    }

    let flag_high = flag_pivots
        .iter()
        .map(|(_, p, _)| *p)
        .fold(f64::MIN, f64::max);
    let retrace = (flag_high - pole_bottom.1) / pole_height;
    if retrace > params.flag_max_retrace {
        return None;
    }

    if params.flag_require_countertrend_pivot_slope
        && !flag_bear_lows_non_falling(&flag_pivots, pole_bottom.1, params.price_tolerance)
    {
        return None;
    }

    let target = pole_bottom.1 - pole_height; // measured move down

    let retrace_q = (1.0 - retrace / params.flag_max_retrace).clamp(0.0, 1.0);
    let quality = retrace_q;

    let mut fpivots = vec![pole_top, pole_bottom];
    fpivots.extend(flag_pivots.iter().copied().skip(1));

    Some(FormationMatch {
        pattern_type_id: 21,
        pattern_name: "Bearish Flag",
        pivots: fpivots,
        neckline: None,
        height: pole_height,
        target_price: Some(target),
        quality,
        volume_analysis: None,
    })
}

// ─── Tüm formasyonları tarama ──────────────────────────────────────

/// Verilen pivotları tüm Faz 2 formasyonlarına karşı tarar.
/// Eşleşen tüm formasyonları döndürür (birden fazla olabilir).
/// Faz 3: Hacim verisi varsa her eşleşmeye volume analizi eklenir.
#[must_use]
pub fn scan_formations(
    pivots: &[PivotTriple],
    bars: &[OhlcBar],
    params: &FormationParams,
) -> Vec<FormationMatch> {
    let mut results = Vec::new();

    if let Some(m) = detect_double_top(pivots, bars, params) {
        results.push(m);
    }
    if let Some(m) = detect_double_bottom(pivots, bars, params) {
        results.push(m);
    }
    if let Some(m) = detect_head_and_shoulders(pivots, bars, params) {
        results.push(m);
    }
    if let Some(m) = detect_inverse_head_and_shoulders(pivots, bars, params) {
        results.push(m);
    }
    if let Some(m) = detect_triple_top(pivots, bars, params) {
        results.push(m);
    }
    if let Some(m) = detect_triple_bottom(pivots, bars, params) {
        results.push(m);
    }
    if let Some(m) = detect_bullish_flag(pivots, bars, params) {
        results.push(m);
    }
    if let Some(m) = detect_bearish_flag(pivots, bars, params) {
        results.push(m);
    }

    // Faz 3: Hacim analizi ekleme
    if !results.is_empty() {
        let bar_map: std::collections::BTreeMap<i64, OhlcBar> =
            bars.iter().map(|b| (b.bar_index, *b)).collect();
        let has_volume = bars.iter().any(|b| b.volume.is_some());
        if has_volume {
            let last_bar = bar_map.keys().next_back().copied().unwrap_or(0);
            for m in &mut results {
                m.volume_analysis = Some(crate::volume_analysis::analyze_formation_volume(
                    &m.pivots, &bar_map, last_bar, 20, 1.5,
                ));
            }
        }
    }

    if params.dedupe_overlapping && results.len() > 1 {
        results = dedupe_formations_greedy_by_pivot_overlap(results);
    }

    results
}

// ─── Testler ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn default_params() -> FormationParams {
        FormationParams::default()
    }

    #[test]
    fn double_top_detected() {
        // H-L-H pattern: two peaks at ~100, trough at 90.
        // Second peak 101 vs 100 keeps `pct_diff` within `price_tolerance` but drives `price_q` down
        // (quality = 0.7 * price_q + 0.3 * symmetry); use 100.5 so quality stays above 0.5.
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),
            (5, 90.0, -1),
            (10, 100.5, 1),
        ];
        let result = detect_double_top(&pivots, &[], &default_params());
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.pattern_type_id, 14);
        assert!((m.neckline.unwrap() - 90.0).abs() < 0.01);
        assert!(m.target_price.unwrap() < 90.0); // target below neckline
        assert!(m.quality > 0.5);
    }

    #[test]
    fn double_top_rejected_when_peaks_too_different() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),
            (5, 90.0, -1),
            (10, 110.0, 1), // 10% difference > default tolerance
        ];
        assert!(detect_double_top(&pivots, &[], &default_params()).is_none());
    }

    #[test]
    fn double_top_rejected_when_second_peak_higher_under_strict_rule() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),
            (5, 92.0, -1),
            (10, 100.5, 1), // within default tolerance vs first peak
        ];
        assert!(detect_double_top(&pivots, &[], &default_params()).is_some());
        let strict = FormationParams::default().with_strict_double_top_peaks();
        assert!(detect_double_top(&pivots, &[], &strict).is_none());
    }

    #[test]
    fn double_bottom_detected() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 50.0, -1),
            (5, 60.0, 1),
            (10, 50.5, -1),
        ];
        let result = detect_double_bottom(&pivots, &[], &default_params());
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.pattern_type_id, 15);
        assert!(m.target_price.unwrap() > 60.0);
    }

    #[test]
    fn double_bottom_rejected_when_second_trough_deeper_under_strict_rule() {
        // Within default price tolerance, but second low is clearly below first — many textbooks exclude this.
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, -1),
            (5, 110.0, 1),
            (10, 99.0, -1), // ~1% below first vs avg 99.5 → pct_diff under tightened tolerance
        ];
        assert!(detect_double_bottom(&pivots, &[], &default_params()).is_some());
        let strict = FormationParams::default().with_strict_double_bottom_lows();
        assert!(
            detect_double_bottom(&pivots, &[], &strict).is_none(),
            "second low below first should not pass classical W rule"
        );
    }

    #[test]
    fn head_and_shoulders_detected() {
        // LS=100, LT=90, H=110, RT=91, RS=101
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),   // left shoulder
            (5, 90.0, -1),   // left trough
            (10, 110.0, 1),  // head
            (15, 91.0, -1),  // right trough
            (20, 101.0, 1),  // right shoulder
        ];
        let result = detect_head_and_shoulders(&pivots, &[], &default_params());
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.pattern_type_id, 16);
        assert!(m.neckline.unwrap() < 95.0);
        assert!(m.target_price.unwrap() < m.neckline.unwrap());
    }

    #[test]
    fn head_and_shoulders_rejected_when_head_not_highest() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 110.0, 1),  // "left shoulder" higher than head
            (5, 90.0, -1),
            (10, 105.0, 1), // "head" lower
            (15, 91.0, -1),
            (20, 100.0, 1),
        ];
        assert!(detect_head_and_shoulders(&pivots, &[], &default_params()).is_none());
    }

    #[test]
    fn inverse_head_and_shoulders_detected() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 50.0, -1),  // left shoulder
            (5, 60.0, 1),   // left peak
            (10, 40.0, -1), // head (lowest)
            (15, 59.0, 1),  // right peak
            (20, 49.0, -1), // right shoulder
        ];
        let result = detect_inverse_head_and_shoulders(&pivots, &[], &default_params());
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.pattern_type_id, 17);
        assert!(m.target_price.unwrap() > m.neckline.unwrap());
    }

    #[test]
    fn triple_top_detected() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),
            (5, 90.0, -1),
            (10, 101.0, 1),
            (15, 89.0, -1),
            (20, 100.5, 1),
        ];
        let result = detect_triple_top(&pivots, &[], &default_params());
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.pattern_type_id, 18);
    }

    #[test]
    fn triple_bottom_detected() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 50.0, -1),
            (5, 60.0, 1),
            (10, 50.15, -1),
            (15, 61.0, 1),
            (20, 49.95, -1),
        ];
        let result = detect_triple_bottom(&pivots, &[], &default_params());
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.pattern_type_id, 19);
    }

    #[test]
    fn bullish_flag_detected() {
        // Pole: 50→100, then flag consolidation dipping slightly
        let pivots: Vec<PivotTriple> = vec![
            (0, 50.0, -1),  // pole bottom
            (5, 100.0, 1),  // pole top
            (8, 92.0, -1),  // flag low
            (11, 96.0, 1),  // flag high
            (14, 93.0, -1), // flag low
        ];
        let bars: Vec<OhlcBar> = (0..15)
            .map(|i| OhlcBar {
                open: 70.0,
                high: 71.0,
                low: 69.0,
                close: 70.0,
                bar_index: i,
                volume: None,
            })
            .collect();
        let result = detect_bullish_flag(&pivots, &bars, &default_params());
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.pattern_type_id, 20);
        assert!((m.target_price.unwrap() - 150.0).abs() < 1.0);
    }

    #[test]
    fn bearish_flag_detected() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),  // pole top
            (5, 50.0, -1),  // pole bottom
            (8, 58.0, 1),   // flag high
            (11, 53.0, -1), // flag low
            (14, 57.0, 1),  // flag high
        ];
        let bars: Vec<OhlcBar> = (0..15)
            .map(|i| OhlcBar {
                open: 70.0,
                high: 71.0,
                low: 69.0,
                close: 70.0,
                bar_index: i,
                volume: None,
            })
            .collect();
        let result = detect_bearish_flag(&pivots, &bars, &default_params());
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.pattern_type_id, 21);
        assert!(m.target_price.unwrap() < 50.0);
    }

    #[test]
    fn scan_formations_finds_multiple() {
        // Double top + triple top share pivots in extended series
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),
            (5, 90.0, -1),
            (10, 101.0, 1),
            (15, 89.0, -1),
            (20, 100.5, 1),
        ];
        let bars: Vec<OhlcBar> = (0..25)
            .map(|i| OhlcBar {
                open: 95.0,
                high: 101.0,
                low: 89.0,
                close: 95.0,
                bar_index: i,
                volume: None,
            })
            .collect();
        let matches = scan_formations(&pivots, &bars, &default_params());
        // Should find triple top (all 3 peaks similar) and possibly double top (last 2 peaks)
        assert!(!matches.is_empty());
        let ids: Vec<i32> = matches.iter().map(|m| m.pattern_type_id).collect();
        assert!(ids.contains(&18)); // triple top
    }

    #[test]
    fn literature_prior_trend_rejects_flat_market_double_top() {
        let pivots = vec![
            (20_i64, 100.0, 1),
            (22, 90.0, -1),
            (30, 100.0, 1),
        ];
        let bars: Vec<OhlcBar> = (0..35)
            .map(|i| OhlcBar {
                open: 100.0,
                high: 101.0,
                low: 99.0,
                close: 100.0,
                bar_index: i,
                volume: None,
            })
            .collect();
        assert!(
            detect_double_top(&pivots, &bars, &FormationParams::literature_standard()).is_none(),
            "flat closes should fail prior uptrend into first peak"
        );
    }

    #[test]
    fn literature_prior_trend_accepts_rising_closes_double_top() {
        let pivots = vec![
            (20_i64, 100.0, 1),
            (22, 90.0, -1),
            (30, 100.0, 1),
        ];
        let bars: Vec<OhlcBar> = (0..35)
            .map(|i| OhlcBar {
                open: 60.0 + i as f64,
                high: 61.0 + i as f64,
                low: 59.0 + i as f64,
                close: 60.0 + i as f64,
                bar_index: i,
                volume: None,
            })
            .collect();
        assert!(detect_double_top(&pivots, &bars, &FormationParams::literature_standard()).is_some());
    }

    #[test]
    fn literature_neckline_break_required_double_bottom() {
        let pivots = vec![
            (0_i64, 50.0, -1),
            (5, 60.0, 1),
            (10, 50.5, -1),
        ];
        let mut params = FormationParams::default().with_literature_neckline_confirmation();
        params.require_prior_trend = false;
        let bars_fail: Vec<OhlcBar> = (0..12)
            .map(|i| OhlcBar {
                open: 55.0,
                high: 58.0,
                low: 52.0,
                close: 55.0,
                bar_index: i,
                volume: None,
            })
            .collect();
        assert!(detect_double_bottom(&pivots, &bars_fail, &params).is_none());
        let mut bars_ok = bars_fail;
        if let Some(b) = bars_ok.last_mut() {
            b.close = 65.0;
        }
        assert!(detect_double_bottom(&pivots, &bars_ok, &params).is_some());
    }

    #[test]
    fn bullish_flag_rejected_when_countertrend_highs_rise() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 50.0, -1),
            (5, 100.0, 1),
            (8, 92.0, -1),
            (11, 98.0, 1),
            (14, 94.0, -1),
            (17, 99.0, 1),
        ];
        let bars: Vec<OhlcBar> = (0..20)
            .map(|i| OhlcBar {
                open: 70.0,
                high: 71.0,
                low: 69.0,
                close: 70.0,
                bar_index: i,
                volume: None,
            })
            .collect();
        let mut p = default_params();
        p.flag_require_countertrend_pivot_slope = true;
        assert!(detect_bullish_flag(&pivots, &bars, &p).is_none());
    }

    #[test]
    fn scan_dedupe_removes_pivot_overlaps() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),
            (5, 90.0, -1),
            (10, 101.0, 1),
            (15, 89.0, -1),
            (20, 100.5, 1),
        ];
        let bars: Vec<OhlcBar> = (0..25)
            .map(|i| OhlcBar {
                open: 95.0,
                high: 101.0,
                low: 89.0,
                close: 95.0,
                bar_index: i,
                volume: None,
            })
            .collect();
        let full = scan_formations(&pivots, &bars, &default_params());
        let mut p = default_params();
        p.dedupe_overlapping = true;
        let deduped = scan_formations(&pivots, &bars, &p);
        assert!(deduped.len() <= full.len());
        for i in 0..deduped.len() {
            for j in (i + 1)..deduped.len() {
                let sa: HashSet<i64> = deduped[i].pivots.iter().map(|(x, _, _)| *x).collect();
                let overlap = deduped[j].pivots.iter().any(|(x, _, _)| sa.contains(x));
                assert!(!overlap, "deduped matches must not share pivot bars");
            }
        }
    }

    #[test]
    fn insufficient_pivots_returns_none() {
        let pivots: Vec<PivotTriple> = vec![(0, 100.0, 1), (5, 90.0, -1)];
        let params = default_params();
        assert!(detect_double_top(&pivots, &[], &params).is_none());
        assert!(detect_head_and_shoulders(&pivots, &[], &params).is_none());
        assert!(detect_triple_top(&pivots, &[], &params).is_none());
    }
}

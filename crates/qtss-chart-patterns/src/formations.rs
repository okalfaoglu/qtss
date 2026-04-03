//! Faz 2 — Klasik grafik formasyonları: Double Top/Bottom, Head & Shoulders,
//! Triple Top/Bottom, Flag/Pennant.
//!
//! Mevcut 6-pivot kanal/üçgen sistemi (ID 1–13) iki trend çizgisine dayalıdır.
//! Bu modül zigzag pivotlarından farklı geometrik kalıpları tespit eder.
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
}

/// Formasyon tespiti için yapılandırma parametreleri.
#[derive(Debug, Clone)]
pub struct FormationParams {
    /// İki tepe/dip fiyatının birbirine eşit sayılması için maksimum yüzde farkı.
    /// Örnek: 0.02 = %2.
    pub price_tolerance: f64,
    /// Flag formasyonlarında impulsive bacağın minimum bar sayısı.
    pub flag_min_pole_bars: usize,
    /// Flag formasyonlarında konsolidasyon alanının minimum bar sayısı.
    pub flag_min_flag_bars: usize,
    /// Flag bayrak gövdesi en fazla impulsive hareketin bu oranı kadar geri çekilebilir.
    pub flag_max_retrace: f64,
}

impl Default for FormationParams {
    fn default() -> Self {
        Self {
            price_tolerance: 0.02,
            flag_min_pole_bars: 3,
            flag_min_flag_bars: 3,
            flag_max_retrace: 0.618,
        }
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

/// Pivotlardan sadece tepe (dir > 0) olanları filtreler.
fn highs(pivots: &[PivotTriple]) -> Vec<PivotTriple> {
    pivots.iter().copied().filter(|(_, _, d)| *d > 0).collect()
}

/// Pivotlardan sadece dip (dir < 0) olanları filtreler.
fn lows(pivots: &[PivotTriple]) -> Vec<PivotTriple> {
    pivots.iter().copied().filter(|(_, _, d)| *d < 0).collect()
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
/// - Neckline = aradaki dip fiyatı
/// - Hedef = neckline - (tepe - neckline)
#[must_use]
pub fn detect_double_top(pivots: &[PivotTriple], params: &FormationParams) -> Option<FormationMatch> {
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

    let neckline = between_lows.iter().map(|(_, p, _)| *p).fold(f64::MAX, f64::min);
    let peak_avg = (t1.1 + t2.1) / 2.0;
    let height = (peak_avg - neckline).abs();
    let target = neckline - height;

    // Kalite: fiyat yakınlığı + simetri
    let price_q = 1.0 - (diff / params.price_tolerance).min(1.0);
    let sym = symmetry_score(&[t2.0 - t1.0]);
    let quality = (price_q * 0.7 + sym * 0.3).clamp(0.0, 1.0);

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
    })
}

// ─── Double Bottom (ID 15) ─────────────────────────────────────────

/// Double Bottom: İki benzer dip + aralarında bir tepe.
#[must_use]
pub fn detect_double_bottom(
    pivots: &[PivotTriple],
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

    let neckline = between_highs
        .iter()
        .map(|(_, p, _)| *p)
        .fold(f64::MIN, f64::max);
    let trough_avg = (b1.1 + b2.1) / 2.0;
    let height = (neckline - trough_avg).abs();
    let target = neckline + height;

    let price_q = 1.0 - (diff / params.price_tolerance).min(1.0);
    let quality = price_q;

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
    })
}

// ─── Inverse Head and Shoulders (ID 17) ────────────────────────────

/// Inverse H&S: Sol omuz (L) – Baş (daha düşük L) – Sağ omuz (L).
/// Aralarında iki tepe → neckline.
#[must_use]
pub fn detect_inverse_head_and_shoulders(
    pivots: &[PivotTriple],
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
    })
}

// ─── Triple Top (ID 18) ───────────────────────────────────────────

/// Triple Top: Üç benzer yükseklikte tepe + aralarında iki dip.
#[must_use]
pub fn detect_triple_top(
    pivots: &[PivotTriple],
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
    let height = (peak_avg - neckline).abs();
    let target = neckline - height;

    let price_q = 1.0 - ((d12 + d23 + d13) / 3.0 / params.price_tolerance).min(1.0);
    let bar_diffs = [t2.0 - t1.0, t3.0 - t2.0];
    let sym = symmetry_score(&bar_diffs);
    let quality = (price_q * 0.6 + sym * 0.4).clamp(0.0, 1.0);

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
    })
}

// ─── Triple Bottom (ID 19) ─────────────────────────────────────────

/// Triple Bottom: Üç benzer dip + aralarında iki tepe.
#[must_use]
pub fn detect_triple_bottom(
    pivots: &[PivotTriple],
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
    let height = (neckline - trough_avg).abs();
    let target = neckline + height;

    let price_q = 1.0 - ((d12 + d23 + d13) / 3.0 / params.price_tolerance).min(1.0);
    let bar_diffs = [b2.0 - b1.0, b3.0 - b2.0];
    let sym = symmetry_score(&bar_diffs);
    let quality = (price_q * 0.6 + sym * 0.4).clamp(0.0, 1.0);

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
    })
}

// ─── Tüm formasyonları tarama ──────────────────────────────────────

/// Verilen pivotları tüm Faz 2 formasyonlarına karşı tarar.
/// Eşleşen tüm formasyonları döndürür (birden fazla olabilir).
#[must_use]
pub fn scan_formations(
    pivots: &[PivotTriple],
    bars: &[OhlcBar],
    params: &FormationParams,
) -> Vec<FormationMatch> {
    let mut results = Vec::new();

    if let Some(m) = detect_double_top(pivots, params) {
        results.push(m);
    }
    if let Some(m) = detect_double_bottom(pivots, params) {
        results.push(m);
    }
    if let Some(m) = detect_head_and_shoulders(pivots, params) {
        results.push(m);
    }
    if let Some(m) = detect_inverse_head_and_shoulders(pivots, params) {
        results.push(m);
    }
    if let Some(m) = detect_triple_top(pivots, params) {
        results.push(m);
    }
    if let Some(m) = detect_triple_bottom(pivots, params) {
        results.push(m);
    }
    if let Some(m) = detect_bullish_flag(pivots, bars, params) {
        results.push(m);
    }
    if let Some(m) = detect_bearish_flag(pivots, bars, params) {
        results.push(m);
    }

    results
}

// ─── Testler ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params() -> FormationParams {
        FormationParams::default()
    }

    #[test]
    fn double_top_detected() {
        // H-L-H pattern: two peaks at ~100, trough at 90
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),
            (5, 90.0, -1),
            (10, 101.0, 1),
        ];
        let result = detect_double_top(&pivots, &default_params());
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
            (10, 110.0, 1), // 10% difference > 2% tolerance
        ];
        assert!(detect_double_top(&pivots, &default_params()).is_none());
    }

    #[test]
    fn double_bottom_detected() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 50.0, -1),
            (5, 60.0, 1),
            (10, 50.5, -1),
        ];
        let result = detect_double_bottom(&pivots, &default_params());
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.pattern_type_id, 15);
        assert!(m.target_price.unwrap() > 60.0);
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
        let result = detect_head_and_shoulders(&pivots, &default_params());
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
        assert!(detect_head_and_shoulders(&pivots, &default_params()).is_none());
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
        let result = detect_inverse_head_and_shoulders(&pivots, &default_params());
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
        let result = detect_triple_top(&pivots, &default_params());
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.pattern_type_id, 18);
    }

    #[test]
    fn triple_bottom_detected() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 50.0, -1),
            (5, 60.0, 1),
            (10, 50.5, -1),
            (15, 61.0, 1),
            (20, 49.5, -1),
        ];
        let result = detect_triple_bottom(&pivots, &default_params());
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
    fn insufficient_pivots_returns_none() {
        let pivots: Vec<PivotTriple> = vec![(0, 100.0, 1), (5, 90.0, -1)];
        let params = default_params();
        assert!(detect_double_top(&pivots, &params).is_none());
        assert!(detect_head_and_shoulders(&pivots, &params).is_none());
        assert!(detect_triple_top(&pivots, &params).is_none());
    }
}

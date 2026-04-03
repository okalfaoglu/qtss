//! Faz 3 — Hacim analizi: volume divergence, pivot volume comparison, breakout teyidi.
//!
//! Formasyon pivotlarındaki hacim davranışını analiz eder:
//! - **Volume Divergence**: Fiyat yeni tepe/dip yaparken hacim düşüyor → zayıf hareket.
//! - **Pivot Volume Profile**: Her pivottaki hacim ortalaması — hangi pivot daha güçlü?
//! - **Formation Breakout Volume**: Formasyonun son bar'ında hacim spike kontrolü.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::failure_swing::BreakoutVolumeResult;
use crate::find::PivotTriple;
use crate::ohlc::OhlcBar;

/// Formasyon hacim analizi sonucu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormationVolumeAnalysis {
    /// Hacim verisi mevcut mu (bar'larda volume alanı var mı)?
    pub has_volume_data: bool,

    /// Volume divergence tespit edildi mi?
    /// `true` → fiyat yeni tepe/dip yaparken hacim düşüyor (zayıflama sinyali).
    pub volume_divergence: bool,

    /// Divergence yönü: `"bearish"` (fiyat↑ hacim↓), `"bullish"` (fiyat↓ hacim↓), veya `"none"`.
    pub divergence_type: &'static str,

    /// Son iki aynı yönlü pivot arasındaki hacim değişim oranı.
    /// < 1.0 → hacim düşüyor (divergence), > 1.0 → hacim artıyor (teyit).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_change_ratio: Option<f64>,

    /// Breakout teyidi: son bar'daki hacim önceki ortalamanın kaç katı?
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breakout_volume: Option<BreakoutVolumeResult>,

    /// Her pivot bar'ındaki hacim (pivot bar_index sırasında).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pivot_volumes: Vec<PivotVolumeEntry>,

    /// Formasyondaki ortalama hacim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_formation_volume: Option<f64>,
}

/// Tek bir pivot noktasındaki hacim bilgisi.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PivotVolumeEntry {
    pub bar_index: i64,
    pub price: f64,
    pub dir: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
}

// ─── Pivot Volume Çıkarma ──────────────────────────────────────────

/// Her pivot bar'ındaki hacmi çıkarır.
fn extract_pivot_volumes(
    pivots: &[PivotTriple],
    bars: &BTreeMap<i64, OhlcBar>,
) -> Vec<PivotVolumeEntry> {
    pivots
        .iter()
        .map(|(b, p, d)| {
            let vol = bars.get(b).and_then(|bar| bar.volume);
            PivotVolumeEntry {
                bar_index: *b,
                price: *p,
                dir: *d,
                volume: vol,
            }
        })
        .collect()
}

// ─── Volume Divergence ─────────────────────────────────────────────

/// Aynı yönlü (dir) son iki pivot arasında hacim karşılaştırması.
///
/// Bearish divergence: fiyat yeni yüksek tepe yaparken hacim düşüyor.
/// Bullish divergence: fiyat yeni düşük dip yaparken hacim düşüyor.
///
/// Döndürür: `(divergence_detected, divergence_type, volume_change_ratio)`.
fn detect_volume_divergence(
    pivot_volumes: &[PivotVolumeEntry],
) -> (bool, &'static str, Option<f64>) {
    // Son iki tepe pivotunu kontrol et
    let tops: Vec<&PivotVolumeEntry> = pivot_volumes.iter().filter(|pv| pv.dir > 0).collect();
    if let Some(div) = check_pair_divergence(&tops, true) {
        return div;
    }

    // Son iki dip pivotunu kontrol et
    let bottoms: Vec<&PivotVolumeEntry> = pivot_volumes.iter().filter(|pv| pv.dir < 0).collect();
    if let Some(div) = check_pair_divergence(&bottoms, false) {
        return div;
    }

    (false, "none", None)
}

/// İki aynı yönlü pivoyu karşılaştırır. `is_top = true` → tepe pivotları.
fn check_pair_divergence(
    same_dir: &[&PivotVolumeEntry],
    is_top: bool,
) -> Option<(bool, &'static str, Option<f64>)> {
    if same_dir.len() < 2 {
        return None;
    }
    let prev = &same_dir[same_dir.len() - 2];
    let last = &same_dir[same_dir.len() - 1];

    let (prev_vol, last_vol) = match (prev.volume, last.volume) {
        (Some(pv), Some(lv)) if pv > 0.0 && lv > 0.0 => (pv, lv),
        _ => return None,
    };

    let ratio = last_vol / prev_vol;

    if is_top {
        // Bearish divergence: fiyat yükseliyor ama hacim düşüyor
        if last.price >= prev.price && ratio < 0.8 {
            return Some((true, "bearish", Some(ratio)));
        }
    } else {
        // Bullish divergence: fiyat düşüyor ama hacim düşüyor
        if last.price <= prev.price && ratio < 0.8 {
            return Some((true, "bullish", Some(ratio)));
        }
    }

    Some((false, "none", Some(ratio)))
}

// ─── Formasyon İçi Ortalama Hacim ──────────────────────────────────

/// Formasyondaki tüm bar'ların ortalama hacmini hesaplar.
fn avg_volume_in_range(bars: &BTreeMap<i64, OhlcBar>, first: i64, last: i64) -> Option<f64> {
    let vols: Vec<f64> = bars
        .range(first..=last)
        .filter_map(|(_, b)| b.volume)
        .filter(|v| *v > 0.0)
        .collect();
    if vols.is_empty() {
        return None;
    }
    Some(vols.iter().sum::<f64>() / vols.len() as f64)
}

// ─── Ana Analiz Fonksiyonu ─────────────────────────────────────────

/// Bir formasyon için tam hacim analizi yapar.
///
/// - `pivots`: formasyonu oluşturan pivotlar
/// - `bars`: BTreeMap olarak tüm OHLC bar'ları (volume dahil)
/// - `breakout_bar`: kırılım/son bar indeksi
/// - `volume_lookback`: breakout ortalama hesabı için kaç bar geriye bakılacak (önerilen: 20)
/// - `volume_multiplier`: breakout hacminin ortalamayı kaç katı geçmesi gerektiği (önerilen: 1.5)
#[must_use]
pub fn analyze_formation_volume(
    pivots: &[PivotTriple],
    bars: &BTreeMap<i64, OhlcBar>,
    breakout_bar: i64,
    volume_lookback: usize,
    volume_multiplier: f64,
) -> FormationVolumeAnalysis {
    // Hacim verisi var mı kontrol et
    let any_volume = bars.values().any(|b| b.volume.is_some());
    if !any_volume {
        return FormationVolumeAnalysis {
            has_volume_data: false,
            volume_divergence: false,
            divergence_type: "none",
            volume_change_ratio: None,
            breakout_volume: None,
            pivot_volumes: Vec::new(),
            avg_formation_volume: None,
        };
    }

    let pivot_volumes = extract_pivot_volumes(pivots, bars);
    let (divergence, div_type, vol_ratio) = detect_volume_divergence(&pivot_volumes);

    // Breakout volume teyidi
    let breakout_volume =
        crate::failure_swing::check_breakout_volume(bars, breakout_bar, volume_lookback, volume_multiplier);

    // Formasyon aralığındaki ortalama hacim
    let (first_bar, last_bar) = pivots
        .iter()
        .fold((i64::MAX, i64::MIN), |(mn, mx), (b, _, _)| {
            (mn.min(*b), mx.max(*b))
        });
    let avg_vol = avg_volume_in_range(bars, first_bar, last_bar);

    FormationVolumeAnalysis {
        has_volume_data: true,
        volume_divergence: divergence,
        divergence_type: div_type,
        volume_change_ratio: vol_ratio,
        breakout_volume,
        pivot_volumes,
        avg_formation_volume: avg_vol,
    }
}

// ─── Testler ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bars(data: &[(i64, f64, Option<f64>)]) -> BTreeMap<i64, OhlcBar> {
        data.iter()
            .map(|(i, c, v)| {
                (
                    *i,
                    OhlcBar {
                        open: *c,
                        high: c + 1.0,
                        low: c - 1.0,
                        close: *c,
                        bar_index: *i,
                        volume: *v,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn bearish_divergence_detected() {
        // İki tepe: fiyat yükseliyor (100→105) ama hacim düşüyor (1000→500)
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),  // first top
            (5, 90.0, -1),  // trough
            (10, 105.0, 1), // second top (higher price, lower volume)
        ];
        let bars = make_bars(&[
            (0, 100.0, Some(1000.0)),
            (5, 90.0, Some(800.0)),
            (10, 105.0, Some(500.0)),
        ]);
        let result = analyze_formation_volume(&pivots, &bars, 10, 5, 1.5);
        assert!(result.has_volume_data);
        assert!(result.volume_divergence);
        assert_eq!(result.divergence_type, "bearish");
        assert!(result.volume_change_ratio.unwrap() < 0.8);
    }

    #[test]
    fn bullish_divergence_detected() {
        // İki dip: fiyat düşüyor (50→45) ama hacim düşüyor (1000→400)
        let pivots: Vec<PivotTriple> = vec![
            (0, 50.0, -1),
            (5, 60.0, 1),
            (10, 45.0, -1),
        ];
        let bars = make_bars(&[
            (0, 50.0, Some(1000.0)),
            (5, 60.0, Some(800.0)),
            (10, 45.0, Some(400.0)),
        ]);
        let result = analyze_formation_volume(&pivots, &bars, 10, 5, 1.5);
        assert!(result.volume_divergence);
        assert_eq!(result.divergence_type, "bullish");
    }

    #[test]
    fn no_divergence_when_volume_increases() {
        // İki tepe: fiyat ve hacim birlikte yükseliyor
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),
            (5, 90.0, -1),
            (10, 105.0, 1),
        ];
        let bars = make_bars(&[
            (0, 100.0, Some(1000.0)),
            (5, 90.0, Some(800.0)),
            (10, 105.0, Some(1200.0)),
        ]);
        let result = analyze_formation_volume(&pivots, &bars, 10, 5, 1.5);
        assert!(!result.volume_divergence);
        assert_eq!(result.divergence_type, "none");
    }

    #[test]
    fn no_volume_data_returns_empty() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),
            (5, 90.0, -1),
            (10, 101.0, 1),
        ];
        let bars = make_bars(&[
            (0, 100.0, None),
            (5, 90.0, None),
            (10, 101.0, None),
        ]);
        let result = analyze_formation_volume(&pivots, &bars, 10, 5, 1.5);
        assert!(!result.has_volume_data);
        assert!(!result.volume_divergence);
        assert!(result.pivot_volumes.is_empty());
    }

    #[test]
    fn breakout_volume_integrated() {
        // 20 bar öncesi ortalama hacim = 100, breakout bar'ında hacim = 250
        let mut data: Vec<(i64, f64, Option<f64>)> = (0..20)
            .map(|i| (i, 100.0, Some(100.0)))
            .collect();
        data.push((20, 105.0, Some(250.0)));

        let bars = make_bars(&data);
        let pivots: Vec<PivotTriple> = vec![
            (5, 100.0, 1),
            (10, 90.0, -1),
            (15, 101.0, 1),
        ];
        let result = analyze_formation_volume(&pivots, &bars, 20, 20, 1.5);
        assert!(result.breakout_volume.is_some());
        let bv = result.breakout_volume.unwrap();
        assert!(bv.confirmed);
        assert!(bv.volume_ratio > 2.0);
    }

    #[test]
    fn pivot_volumes_populated() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),
            (5, 90.0, -1),
            (10, 101.0, 1),
        ];
        let bars = make_bars(&[
            (0, 100.0, Some(500.0)),
            (5, 90.0, Some(300.0)),
            (10, 101.0, Some(450.0)),
        ]);
        let result = analyze_formation_volume(&pivots, &bars, 10, 5, 1.5);
        assert_eq!(result.pivot_volumes.len(), 3);
        assert!((result.pivot_volumes[0].volume.unwrap() - 500.0).abs() < 0.01);
    }

    #[test]
    fn avg_formation_volume_computed() {
        let pivots: Vec<PivotTriple> = vec![
            (0, 100.0, 1),
            (5, 90.0, -1),
            (10, 101.0, 1),
        ];
        let mut data: Vec<(i64, f64, Option<f64>)> = Vec::new();
        for i in 0..=10 {
            data.push((i, 95.0, Some(100.0)));
        }
        let bars = make_bars(&data);
        let result = analyze_formation_volume(&pivots, &bars, 10, 5, 1.5);
        assert!((result.avg_formation_volume.unwrap() - 100.0).abs() < 0.01);
    }
}

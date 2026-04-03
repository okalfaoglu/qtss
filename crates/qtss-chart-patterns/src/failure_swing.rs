//! Failure Swing (Başarısız Salınım) tespiti.
//!
//! Bir kanal/üçgen formasyonunda fiyatın üst veya alt banda **dokunamadan**
//! geri dönmesi — formasyonun kırılacağının (breakout) öncü sinyalidir.
//!
//! Örnek: Yükselen kanalda fiyat üst banda ulaşamadan geri döner →
//! aşağı yönlü breakout olasılığı artar.

use std::collections::BTreeMap;

use crate::find::{ChannelSixDrawingHints, ChannelSixScanOutcome};
use crate::line_price_at_bar_index;
use crate::ohlc::OhlcBar;

/// Failure swing analiz sonucu.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FailureSwingResult {
    /// Son pivot'un bant sınırına göre ne kadar yaklaştığı (0.0–1.0).
    /// 1.0 = banda tam dokundu, 0.0 = tam orta noktada.
    pub reach_ratio: f64,
    /// Failure swing tespit edildi mi?
    /// `true` → son pivot banda ulaşamadı (reach_ratio < threshold).
    pub is_failure: bool,
    /// Failure yönü: `"upper"` = üst banda ulaşamadı, `"lower"` = alt banda ulaşamadı.
    pub failure_side: String,
    /// Son pivot'un fiyatı.
    pub last_pivot_price: f64,
    /// Hedef bant fiyatı (son pivot bar'ındaki).
    pub target_band_price: f64,
    /// Bant genişliği (son pivot bar'ındaki).
    pub band_width: f64,
}

/// Son pivot'un bant sınırına ulaşma oranını hesaplar.
///
/// `threshold`: 0.0–1.0 arası. Pivot'un banda ulaşma oranı bu değerin altındaysa
/// failure swing kabul edilir. Önerilen: 0.85 (son pivot bandın %85'ine bile ulaşamadı).
#[must_use]
pub fn detect_failure_swing(
    outcome: &ChannelSixScanOutcome,
    _bars: &BTreeMap<i64, OhlcBar>,
    threshold: f64,
) -> Option<FailureSwingResult> {
    let pivots = &outcome.pivots;
    if pivots.len() < 3 {
        return None;
    }

    let hints = crate::find::channel_six_drawing_hints(outcome);
    let last = pivots.last()?;
    let (last_bar, last_price, last_dir) = *last;

    // Son pivot bar'ındaki bant fiyatları
    let upper_at_bar = band_price_at(&hints, last_bar, true)?;
    let lower_at_bar = band_price_at(&hints, last_bar, false)?;
    let width = (upper_at_bar - lower_at_bar).abs();

    if width < 1e-15 {
        return None;
    }

    // dir > 0 → tepe pivotu → üst banda ne kadar yaklaştı
    // dir < 0 → dip pivotu → alt banda ne kadar yaklaştı
    let (target_band, failure_side) = if last_dir > 0 {
        (upper_at_bar, "upper")
    } else {
        (lower_at_bar, "lower")
    };

    // Reach ratio: pivot'un banda ulaşma oranı
    let mid = (upper_at_bar + lower_at_bar) / 2.0;
    let reach_ratio = if last_dir > 0 {
        // Tepe pivotu: mid → upper arası ne kadar
        let range = (upper_at_bar - mid).abs();
        if range < 1e-15 { 1.0 } else {
            ((last_price - mid) / range).clamp(0.0, 1.0)
        }
    } else {
        // Dip pivotu: mid → lower arası ne kadar
        let range = (mid - lower_at_bar).abs();
        if range < 1e-15 { 1.0 } else {
            ((mid - last_price) / range).clamp(0.0, 1.0)
        }
    };

    Some(FailureSwingResult {
        reach_ratio,
        is_failure: reach_ratio < threshold,
        failure_side: failure_side.to_string(),
        last_pivot_price: last_price,
        target_band_price: target_band,
        band_width: width,
    })
}

fn band_price_at(hints: &ChannelSixDrawingHints, bar: i64, upper: bool) -> Option<f64> {
    let endpoints = if upper { &hints.upper } else { &hints.lower };
    line_price_at_bar_index(
        endpoints[0].bar_index, endpoints[0].price,
        endpoints[1].bar_index, endpoints[1].price,
        bar,
    )
}

/// Hacimle desteklenen breakout teyidi.
///
/// `bars`: son N mum, `breakout_bar`: kırılımın gerçekleştiği bar indeksi.
/// Kırılım bar'ındaki hacim, önceki `lookback` bar'ın ortalama hacminin
/// `volume_multiplier` katından büyükse teyit edilir.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BreakoutVolumeResult {
    /// Kırılım bar'ındaki hacim.
    pub breakout_volume: f64,
    /// Önceki `lookback` bar'ın ortalama hacmi.
    pub avg_volume: f64,
    /// Hacim oranı (breakout / ortalama).
    pub volume_ratio: f64,
    /// Hacimle teyit edildi mi?
    pub confirmed: bool,
}

/// Breakout bar'ındaki hacmi önceki ortalamaya göre değerlendirir.
///
/// `volume_multiplier`: önerilen 1.5 (ortalama hacmin %150'si).
/// `lookback`: önceki kaç bar'ın ortalaması alınsın (önerilen 20).
#[must_use]
pub fn check_breakout_volume(
    bars: &BTreeMap<i64, OhlcBar>,
    breakout_bar: i64,
    lookback: usize,
    volume_multiplier: f64,
) -> Option<BreakoutVolumeResult> {
    let breakout_candle = bars.get(&breakout_bar)?;
    let breakout_vol = breakout_candle.volume?;
    if breakout_vol <= 0.0 {
        return None;
    }

    // Önceki bar'ların hacim ortalaması
    let prior_bars: Vec<f64> = bars
        .range(..breakout_bar)
        .rev()
        .take(lookback)
        .filter_map(|(_, b)| b.volume)
        .filter(|v| *v > 0.0)
        .collect();

    if prior_bars.is_empty() {
        return None;
    }

    let avg = prior_bars.iter().sum::<f64>() / prior_bars.len() as f64;
    let ratio = breakout_vol / avg;

    Some(BreakoutVolumeResult {
        breakout_volume: breakout_vol,
        avg_volume: avg,
        volume_ratio: ratio,
        confirmed: ratio >= volume_multiplier,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakout_volume_confirmed() {
        let mut bars = BTreeMap::new();
        // 20 bar ortalama hacim = 100
        for i in 0..20 {
            bars.insert(i, OhlcBar {
                open: 100.0, high: 101.0, low: 99.0, close: 100.0,
                bar_index: i, volume: Some(100.0),
            });
        }
        // Breakout bar: hacim 200 (2x ortalama)
        bars.insert(20, OhlcBar {
            open: 101.0, high: 105.0, low: 100.0, close: 104.0,
            bar_index: 20, volume: Some(200.0),
        });

        let result = check_breakout_volume(&bars, 20, 20, 1.5).unwrap();
        assert!(result.confirmed);
        assert!((result.volume_ratio - 2.0).abs() < 0.01);
    }

    #[test]
    fn breakout_volume_not_confirmed() {
        let mut bars = BTreeMap::new();
        for i in 0..20 {
            bars.insert(i, OhlcBar {
                open: 100.0, high: 101.0, low: 99.0, close: 100.0,
                bar_index: i, volume: Some(100.0),
            });
        }
        // Düşük hacimli breakout
        bars.insert(20, OhlcBar {
            open: 101.0, high: 105.0, low: 100.0, close: 104.0,
            bar_index: 20, volume: Some(120.0),
        });

        let result = check_breakout_volume(&bars, 20, 20, 1.5).unwrap();
        assert!(!result.confirmed);
    }

    #[test]
    fn no_volume_data_returns_none() {
        let mut bars = BTreeMap::new();
        for i in 0..=20 {
            bars.insert(i, OhlcBar {
                open: 100.0, high: 101.0, low: 99.0, close: 100.0,
                bar_index: i, volume: None,
            });
        }
        let result = check_breakout_volume(&bars, 20, 20, 1.5);
        assert!(result.is_none());
    }
}

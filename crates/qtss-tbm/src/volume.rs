//! Volume Pillar — MFI, OBV trend, CVD divergence, volume spike.

use crate::pillar::{PillarKind, PillarScore};

/// Volume pillar skoru hesaplar.
///
/// - `mfi`: Money Flow Index (0–100)
/// - `obv_slope`: OBV'nin son N bardaki eğimi (pozitif = alım baskısı)
/// - `cvd_slope`: CVD eğimi
/// - `volume_last`: Son bar hacmi
/// - `volume_avg`: Ortalama hacim (SMA20)
/// - `is_bottom_search`: true = dip, false = tepe
#[must_use]
pub fn score_volume(
    mfi: f64,
    obv_slope: f64,
    cvd_slope: f64,
    volume_last: f64,
    volume_avg: f64,
    is_bottom_search: bool,
) -> PillarScore {
    let mut score = 0.0_f64;
    let mut details = Vec::new();

    // 1) MFI oversold/overbought (max 30 puan)
    if is_bottom_search {
        if mfi < 20.0 {
            let pts = 30.0 * (20.0 - mfi) / 20.0;
            score += pts;
            details.push(format!("MFI oversold {mfi:.1} (+{pts:.1})"));
        } else if mfi < 35.0 {
            score += 10.0;
            details.push(format!("MFI low zone {mfi:.1} (+10)"));
        }
    } else {
        if mfi > 80.0 {
            let pts = 30.0 * (mfi - 80.0) / 20.0;
            score += pts;
            details.push(format!("MFI overbought {mfi:.1} (+{pts:.1})"));
        } else if mfi > 65.0 {
            score += 10.0;
            details.push(format!("MFI high zone {mfi:.1} (+10)"));
        }
    }

    // 2) OBV trend (max 25 puan)
    if is_bottom_search && obv_slope > 0.0 {
        score += 25.0;
        details.push("OBV rising (accumulation)".into());
    } else if !is_bottom_search && obv_slope < 0.0 {
        score += 25.0;
        details.push("OBV falling (distribution)".into());
    }

    // 3) CVD divergence (max 25 puan)
    // Dip arama: fiyat düşerken CVD yükseliyorsa = gizli alım
    if is_bottom_search && cvd_slope > 0.0 {
        score += 25.0;
        details.push("CVD rising while searching bottom (hidden buying)".into());
    } else if !is_bottom_search && cvd_slope < 0.0 {
        score += 25.0;
        details.push("CVD falling while searching top (hidden selling)".into());
    }

    // 4) Volume spike — climactic volume (max 20 puan)
    if volume_avg > 0.0 {
        let ratio = volume_last / volume_avg;
        if ratio > 2.0 {
            let pts = (20.0 * (ratio - 1.0) / 2.0).min(20.0);
            score += pts;
            details.push(format!("Volume spike {ratio:.1}x avg (+{pts:.1})"));
        }
    }

    PillarScore {
        kind: PillarKind::Volume,
        score: score.min(100.0),
        weight: 0.25,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_accumulation() {
        let s = score_volume(15.0, 100.0, 50.0, 3000.0, 1000.0, true);
        assert!(s.score > 60.0);
        assert!(!s.details.is_empty());
    }

    #[test]
    fn top_distribution() {
        let s = score_volume(85.0, -100.0, -50.0, 500.0, 1000.0, false);
        assert!(s.score > 50.0);
    }
}

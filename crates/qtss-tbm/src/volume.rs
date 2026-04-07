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

    // 1) MFI (max 30 puan) — tek doğrusal bant; klasik yorumla uyumlu (düşük = dip sinyali, yüksek = tepe).
    //
    // Eski formül hatası: `30*(20-mfi)/20` ile mfi≈19 neredeyse 0 puana düşüyor, oysa 20–35 bandında
    // düz +10 vardı → **daha derin oversold (ör. 15)**, daha zayıf bölgeden (ör. 32) **daha az** puan alıyordu.
    // Tepe tarafında aynı ayna sorunu (ör. mfi=85 < mfi=70’nin +10’u).
    if is_bottom_search {
        if mfi < 35.0 {
            let pts = (30.0 * (35.0 - mfi) / 35.0).clamp(0.0, 30.0);
            score += pts;
            details.push(format!("MFI oversold / dip zone {mfi:.1} (+{pts:.1})"));
        }
    } else if mfi > 65.0 {
        let pts = (30.0 * (mfi - 65.0) / 35.0).clamp(0.0, 30.0);
        score += pts;
        details.push(format!("MFI overbought / top zone {mfi:.1} (+{pts:.1})"));
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
    // Smooth ramp: `2×` kesitinde 0→~10 sıçraması yerine 1.5× → 0 puan, 3× → 20 puan (doğrusal, sürekli).
    const VOL_SPIKE_LO: f64 = 1.5;
    const VOL_SPIKE_HI: f64 = 3.0;
    if volume_avg > 0.0 {
        let ratio = volume_last / volume_avg;
        if ratio > VOL_SPIKE_LO {
            let span = VOL_SPIKE_HI - VOL_SPIKE_LO;
            let pts = (20.0 * (ratio - VOL_SPIKE_LO) / span).clamp(0.0, 20.0);
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

    #[test]
    fn mfi_lower_reading_scores_higher_on_bottom_search() {
        let deep = score_volume(12.0, 0.0, 0.0, 0.0, 1.0, true);
        let mild = score_volume(30.0, 0.0, 0.0, 0.0, 1.0, true);
        assert!(
            deep.score > mild.score,
            "deeper oversold MFI should contribute more to bottom pillar than mid band"
        );
    }

    #[test]
    fn mfi_higher_reading_scores_higher_on_top_search() {
        let strong = score_volume(92.0, 0.0, 0.0, 0.0, 1.0, false);
        let weak = score_volume(68.0, 0.0, 0.0, 0.0, 1.0, false);
        assert!(
            strong.score > weak.score,
            "stronger overbought MFI should contribute more to top pillar"
        );
    }

    #[test]
    fn volume_spike_no_cliff_near_2x() {
        // Yalnızca hacim bileşeni: MFI 50 (dip bandı dışı), OBV/CVD kapalı.
        let just_below = score_volume(50.0, 0.0, 0.0, 1990.0, 1000.0, true);
        let just_above = score_volume(50.0, 0.0, 0.0, 2010.0, 1000.0, true);
        assert!(
            just_below.score > 0.0 && just_above.score > just_below.score,
            "1.99x and 2.01x should both score with a small delta"
        );
        let step = just_above.score - just_below.score;
        assert!(step < 1.5, "old design jumped ~10 pts across 2×; smooth ramp step={step}");
    }

    #[test]
    fn volume_spike_1_5_zero_3_0_full() {
        let at_lo = score_volume(50.0, 0.0, 0.0, 1500.0, 1000.0, true);
        assert!(
            !at_lo.details.iter().any(|d| d.starts_with("Volume spike")),
            "1.5x should not add spike (threshold exclusive)"
        );
        assert!((at_lo.score - 0.0).abs() < 1e-9);
        let at_hi = score_volume(50.0, 0.0, 0.0, 3000.0, 1000.0, true);
        assert!((at_hi.score - 20.0).abs() < 1e-6);
    }
}

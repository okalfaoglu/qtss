//! TBM Setup detection — skor eşiğini geçtiğinde setup oluşturur.

use serde::{Deserialize, Serialize};
use crate::scorer::{TbmScore, TbmSignal};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetupDirection {
    /// Dip setup — long fırsatı
    Bottom,
    /// Tepe setup — short/çıkış fırsatı
    Top,
}

/// Tespit edilen TBM setup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TbmSetup {
    pub direction: SetupDirection,
    pub score: f64,
    pub signal: TbmSignal,
    /// İnsan-okunur özet
    pub summary: String,
    /// Tüm pillar detayları
    pub pillar_details: Vec<String>,
}

/// Setup eşik ayarları.
#[derive(Debug, Clone)]
pub struct SetupThresholds {
    /// Minimum skor (varsayılan 30)
    pub min_score: f64,
    /// Minimum aktif pillar sayısı (score > 20 olan pillar)
    pub min_active_pillars: usize,
}

impl Default for SetupThresholds {
    fn default() -> Self {
        Self {
            min_score: 30.0,
            min_active_pillars: 2,
        }
    }
}

/// Bottom ve top skorlarından setup'ları tespit eder.
/// Her iki yönü de kontrol eder, eşiği geçenleri döndürür.
#[must_use]
pub fn detect_setups(
    bottom_score: &TbmScore,
    top_score: &TbmScore,
    thresholds: &SetupThresholds,
) -> Vec<TbmSetup> {
    let mut setups = Vec::new();

    for (score, dir) in [
        (bottom_score, SetupDirection::Bottom),
        (top_score, SetupDirection::Top),
    ] {
        let active_pillars = score.pillars.iter().filter(|p| p.score > 20.0).count();
        if score.total >= thresholds.min_score && active_pillars >= thresholds.min_active_pillars {
            let dir_str = match dir {
                SetupDirection::Bottom => "BOTTOM",
                SetupDirection::Top => "TOP",
            };
            let summary = format!(
                "TBM {dir_str} setup: score={:.1}, signal={:?}, {active_pillars} active pillars",
                score.total, score.signal,
            );
            let pillar_details: Vec<String> = score
                .pillars
                .iter()
                .flat_map(|p| p.details.iter().cloned())
                .collect();

            setups.push(TbmSetup {
                direction: dir,
                score: score.total,
                signal: score.signal,
                summary,
                pillar_details,
            });
        }
    }

    setups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pillar::{PillarKind, PillarScore};
    use crate::scorer::score_tbm;

    #[test]
    fn detect_bottom_setup() {
        let bottom = score_tbm(vec![
            PillarScore { kind: PillarKind::Momentum, score: 70.0, weight: 0.30, details: vec!["oversold".into()] },
            PillarScore { kind: PillarKind::Volume, score: 60.0, weight: 0.25, details: vec!["accumulation".into()] },
            PillarScore { kind: PillarKind::Structure, score: 80.0, weight: 0.30, details: vec!["fib 61.8%".into()] },
            PillarScore { kind: PillarKind::Onchain, score: 0.0, weight: 0.0, details: vec![] },
        ]);
        let top = score_tbm(vec![
            PillarScore { kind: PillarKind::Momentum, score: 10.0, weight: 0.30, details: vec![] },
            PillarScore { kind: PillarKind::Volume, score: 5.0, weight: 0.25, details: vec![] },
            PillarScore { kind: PillarKind::Structure, score: 10.0, weight: 0.30, details: vec![] },
            PillarScore { kind: PillarKind::Onchain, score: 0.0, weight: 0.0, details: vec![] },
        ]);

        let setups = detect_setups(&bottom, &top, &SetupThresholds::default());
        assert_eq!(setups.len(), 1);
        assert_eq!(setups[0].direction, SetupDirection::Bottom);
        assert!(setups[0].score > 50.0);
    }

    #[test]
    fn no_setup_below_threshold() {
        let low = score_tbm(vec![
            PillarScore { kind: PillarKind::Momentum, score: 20.0, weight: 0.30, details: vec![] },
            PillarScore { kind: PillarKind::Volume, score: 15.0, weight: 0.25, details: vec![] },
            PillarScore { kind: PillarKind::Structure, score: 10.0, weight: 0.30, details: vec![] },
            PillarScore { kind: PillarKind::Onchain, score: 0.0, weight: 0.0, details: vec![] },
        ]);
        let setups = detect_setups(&low, &low, &SetupThresholds::default());
        assert!(setups.is_empty());
    }
}

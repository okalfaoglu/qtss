//! Pillar skorlarını birleştirerek final TBM skoru üretir.

use serde::{Deserialize, Serialize};
use crate::pillar::PillarScore;

/// Birleşik TBM skoru.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TbmScore {
    /// Ağırlıklı toplam skor (0–100)
    pub total: f64,
    /// Sinyal seviyesi
    pub signal: TbmSignal,
    /// Her pillar'ın skoru
    pub pillars: Vec<PillarScore>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TbmSignal {
    /// < 30: Sinyal yok
    None,
    /// 30–50: Zayıf sinyal, izlemeye al
    Weak,
    /// 50–70: Orta sinyal, hazırlık
    Moderate,
    /// 70–85: Güçlü sinyal, setup oluşuyor
    Strong,
    /// > 85: Çok güçlü, aksiyon al
    VeryStrong,
}

/// Pillar skorlarından final TBM skoru hesaplar.
/// Negatif ağırlıklar yok sayılır (yanlış konfigürasyonda pay/payda tutarlı kalsın).
#[must_use]
pub fn score_tbm(pillars: Vec<PillarScore>) -> TbmScore {
    let mut total_weight = 0.0;
    let mut weighted_sum = 0.0;
    for p in &pillars {
        let w = p.weight.max(0.0);
        total_weight += w;
        weighted_sum += p.score * w;
    }

    let total = if total_weight > 0.0 {
        (weighted_sum / total_weight).min(100.0)
    } else {
        0.0
    };

    let signal = match total {
        t if t >= 85.0 => TbmSignal::VeryStrong,
        t if t >= 70.0 => TbmSignal::Strong,
        t if t >= 50.0 => TbmSignal::Moderate,
        t if t >= 30.0 => TbmSignal::Weak,
        _ => TbmSignal::None,
    };

    TbmScore { total, signal, pillars }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pillar::PillarKind;

    #[test]
    fn scoring_basic() {
        let pillars = vec![
            PillarScore { kind: PillarKind::Momentum, score: 80.0, weight: 0.30, details: vec![] },
            PillarScore { kind: PillarKind::Volume, score: 60.0, weight: 0.25, details: vec![] },
            PillarScore { kind: PillarKind::Structure, score: 90.0, weight: 0.30, details: vec![] },
            PillarScore { kind: PillarKind::Onchain, score: 50.0, weight: 0.15, details: vec![] },
        ];
        let r = score_tbm(pillars);
        // (80*0.3 + 60*0.25 + 90*0.3 + 50*0.15) / 1.0 = 24+15+27+7.5 = 73.5
        assert!((r.total - 73.5).abs() < 0.1);
        assert_eq!(r.signal, TbmSignal::Strong);
    }

    #[test]
    fn no_onchain_data() {
        let pillars = vec![
            PillarScore { kind: PillarKind::Momentum, score: 60.0, weight: 0.30, details: vec![] },
            PillarScore { kind: PillarKind::Volume, score: 40.0, weight: 0.25, details: vec![] },
            PillarScore { kind: PillarKind::Structure, score: 50.0, weight: 0.30, details: vec![] },
            PillarScore { kind: PillarKind::Onchain, score: 0.0, weight: 0.0, details: vec![] },
        ];
        let r = score_tbm(pillars);
        // (60*0.3 + 40*0.25 + 50*0.3) / 0.85 = (18+10+15)/0.85 ≈ 50.6
        assert!(r.total > 50.0 && r.total < 52.0);
        assert_eq!(r.signal, TbmSignal::Moderate);
    }

    #[test]
    fn negative_weight_ignored() {
        let pillars = vec![
            PillarScore { kind: PillarKind::Momentum, score: 80.0, weight: 0.30, details: vec![] },
            PillarScore { kind: PillarKind::Volume, score: 60.0, weight: 0.25, details: vec![] },
            PillarScore { kind: PillarKind::Structure, score: 90.0, weight: 0.30, details: vec![] },
            PillarScore { kind: PillarKind::Onchain, score: 50.0, weight: -0.15, details: vec![] },
        ];
        let r = score_tbm(pillars);
        // Onchain clamped to 0: (80*0.3 + 60*0.25 + 90*0.3) / 0.85
        let expected = (24.0 + 15.0 + 27.0) / 0.85;
        assert!((r.total - expected).abs() < 0.01);
    }
}

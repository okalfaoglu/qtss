//! Multi-Timeframe (MTF) konfirmasyon — farklı zaman dilimlerindeki TBM skorlarını birleştirir.
//!
//! Yüksek TF sinyali düşük TF'yi güçlendirir (alignment).
//! Çelişki varsa skor düşürülür (conflict).

use serde::{Deserialize, Serialize};
use crate::scorer::TbmSignal;

/// Desteklenen zaman dilimleri, küçükten büyüğe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Timeframe {
    M15,
    H1,
    H4,
    D1,
    W1,
}

impl Timeframe {
    /// Timeframe ağırlığı — büyük TF daha güçlü.
    #[must_use]
    pub fn weight(self) -> f64 {
        match self {
            Self::M15 => 0.10,
            Self::H1 => 0.20,
            Self::H4 => 0.30,
            Self::D1 => 0.30,
            Self::W1 => 0.10,
        }
    }

    /// Interval string'inden parse.
    #[must_use]
    pub fn from_interval(s: &str) -> Option<Self> {
        match s {
            "15m" => Some(Self::M15),
            "1h" | "60m" => Some(Self::H1),
            "4h" | "240m" => Some(Self::H4),
            "1d" | "1D" => Some(Self::D1),
            "1w" | "1W" => Some(Self::W1),
            _ => None,
        }
    }
}

/// Tek bir timeframe'den gelen TBM skoru özeti.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TfScore {
    pub timeframe: Timeframe,
    pub bottom_score: f64,
    pub top_score: f64,
    pub bottom_signal: TbmSignal,
    pub top_signal: TbmSignal,
}

/// MTF birleştirme sonucu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtfResult {
    /// Ağırlıklı MTF bottom skoru (0–100).
    pub bottom_score: f64,
    /// Ağırlıklı MTF top skoru (0–100).
    pub top_score: f64,
    /// Kaç TF bottom'da uyumlu (score > 40).
    pub bottom_alignment: usize,
    /// Kaç TF top'da uyumlu.
    pub top_alignment: usize,
    /// Toplam TF sayısı.
    pub tf_count: usize,
    /// Çelişki var mı (bir TF bottom, diğeri top sinyali).
    pub has_conflict: bool,
    /// MTF sinyal seviyesi (bottom yönü).
    pub bottom_signal: TbmSignal,
    /// MTF sinyal seviyesi (top yönü).
    pub top_signal: TbmSignal,
    /// Alignment bonus/penalty açıklaması.
    pub details: Vec<String>,
    /// Her TF'nin bireysel skoru.
    pub tf_scores: Vec<TfScore>,
}

/// Birden fazla timeframe'in TBM skorlarını birleştirir.
///
/// Mantık:
/// 1. Ağırlıklı ortalama (TF weight'lerine göre)
/// 2. Alignment bonus: tüm TF'ler aynı yönde → +15%
/// 3. Conflict penalty: zıt yönler → −20%
/// 4. Higher TF override: D1/W1 çok güçlüyse (>80) düşük TF'yi destekler
#[must_use]
pub fn mtf_confirm(tf_scores: &[TfScore]) -> MtfResult {
    if tf_scores.is_empty() {
        return MtfResult {
            bottom_score: 0.0,
            top_score: 0.0,
            bottom_alignment: 0,
            top_alignment: 0,
            tf_count: 0,
            has_conflict: false,
            bottom_signal: TbmSignal::None,
            top_signal: TbmSignal::None,
            details: vec!["No timeframe data".into()],
            tf_scores: vec![],
        };
    }

    let mut details = Vec::new();

    // 1) Ağırlıklı ortalama
    let total_weight: f64 = tf_scores.iter().map(|t| t.timeframe.weight()).sum();
    let mut bottom_weighted: f64 = tf_scores
        .iter()
        .map(|t| t.bottom_score * t.timeframe.weight())
        .sum::<f64>()
        / total_weight;
    let mut top_weighted: f64 = tf_scores
        .iter()
        .map(|t| t.top_score * t.timeframe.weight())
        .sum::<f64>()
        / total_weight;

    // 2) Alignment sayısı
    let bottom_aligned = tf_scores.iter().filter(|t| t.bottom_score > 40.0).count();
    let top_aligned = tf_scores.iter().filter(|t| t.top_score > 40.0).count();
    let tf_count = tf_scores.len();

    // 3) Conflict: aynı anda bottom ve top güçlü → çelişki
    let has_conflict = bottom_aligned > 0
        && top_aligned > 0
        && tf_scores.iter().any(|t| t.bottom_score > 50.0)
        && tf_scores.iter().any(|t| t.top_score > 50.0);

    // 4) Alignment bonus
    if bottom_aligned == tf_count && tf_count >= 2 {
        let bonus = bottom_weighted * 0.15;
        bottom_weighted += bonus;
        details.push(format!(
            "Full bottom alignment ({tf_count}/{tf_count} TFs) → +{bonus:.1}"
        ));
    } else if bottom_aligned >= 2 {
        let bonus = bottom_weighted * 0.08;
        bottom_weighted += bonus;
        details.push(format!(
            "Partial bottom alignment ({bottom_aligned}/{tf_count} TFs) → +{bonus:.1}"
        ));
    }

    if top_aligned == tf_count && tf_count >= 2 {
        let bonus = top_weighted * 0.15;
        top_weighted += bonus;
        details.push(format!(
            "Full top alignment ({tf_count}/{tf_count} TFs) → +{bonus:.1}"
        ));
    } else if top_aligned >= 2 {
        let bonus = top_weighted * 0.08;
        top_weighted += bonus;
        details.push(format!(
            "Partial top alignment ({top_aligned}/{tf_count} TFs) → +{bonus:.1}"
        ));
    }

    // 5) Conflict penalty
    if has_conflict {
        bottom_weighted *= 0.80;
        top_weighted *= 0.80;
        details.push("MTF conflict detected → −20% penalty".into());
    }

    // 6) Higher TF boost: D1 veya W1'de çok güçlü sinyal varsa
    for t in tf_scores {
        if matches!(t.timeframe, Timeframe::D1 | Timeframe::W1) {
            if t.bottom_score > 80.0 {
                let boost = 5.0;
                bottom_weighted += boost;
                details.push(format!(
                    "Strong {:?} bottom ({:.0}) → +{boost:.0} boost",
                    t.timeframe, t.bottom_score
                ));
            }
            if t.top_score > 80.0 {
                let boost = 5.0;
                top_weighted += boost;
                details.push(format!(
                    "Strong {:?} top ({:.0}) → +{boost:.0} boost",
                    t.timeframe, t.top_score
                ));
            }
        }
    }

    bottom_weighted = bottom_weighted.clamp(0.0, 100.0);
    top_weighted = top_weighted.clamp(0.0, 100.0);

    let to_signal = |s: f64| match s {
        x if x >= 85.0 => TbmSignal::VeryStrong,
        x if x >= 70.0 => TbmSignal::Strong,
        x if x >= 50.0 => TbmSignal::Moderate,
        x if x >= 30.0 => TbmSignal::Weak,
        _ => TbmSignal::None,
    };

    MtfResult {
        bottom_score: bottom_weighted,
        top_score: top_weighted,
        bottom_alignment: bottom_aligned,
        top_alignment: top_aligned,
        tf_count,
        has_conflict,
        bottom_signal: to_signal(bottom_weighted),
        top_signal: to_signal(top_weighted),
        details,
        tf_scores: tf_scores.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tf(tf: Timeframe, bottom: f64, top: f64) -> TfScore {
        let to_sig = |s: f64| match s {
            x if x >= 85.0 => TbmSignal::VeryStrong,
            x if x >= 70.0 => TbmSignal::Strong,
            x if x >= 50.0 => TbmSignal::Moderate,
            x if x >= 30.0 => TbmSignal::Weak,
            _ => TbmSignal::None,
        };
        TfScore {
            timeframe: tf,
            bottom_score: bottom,
            top_score: top,
            bottom_signal: to_sig(bottom),
            top_signal: to_sig(top),
        }
    }

    #[test]
    fn full_alignment_bottom() {
        let scores = vec![
            make_tf(Timeframe::H1, 70.0, 10.0),
            make_tf(Timeframe::H4, 75.0, 15.0),
            make_tf(Timeframe::D1, 80.0, 5.0),
        ];
        let r = mtf_confirm(&scores);
        assert_eq!(r.bottom_alignment, 3);
        assert_eq!(r.top_alignment, 0);
        assert!(!r.has_conflict);
        // Weighted avg = (70*0.2 + 75*0.3 + 80*0.3) / 0.8 = (14+22.5+24)/0.8 = 75.625
        // +15% alignment bonus → ~86.97, clamped to 100
        assert!(r.bottom_score > 80.0);
        assert!(matches!(r.bottom_signal, TbmSignal::VeryStrong | TbmSignal::Strong));
    }

    #[test]
    fn conflict_penalty() {
        let scores = vec![
            make_tf(Timeframe::H1, 60.0, 55.0),
            make_tf(Timeframe::H4, 65.0, 60.0),
        ];
        let r = mtf_confirm(&scores);
        assert!(r.has_conflict);
        // Both should be penalized by 20%
        assert!(r.bottom_score < 65.0);
        assert!(r.top_score < 60.0);
    }

    #[test]
    fn single_timeframe() {
        let scores = vec![make_tf(Timeframe::H4, 50.0, 20.0)];
        let r = mtf_confirm(&scores);
        assert_eq!(r.tf_count, 1);
        assert_eq!(r.bottom_alignment, 1);
        // No alignment bonus for single TF
        assert!((r.bottom_score - 50.0).abs() < 1.0);
    }

    #[test]
    fn higher_tf_boost() {
        let scores = vec![
            make_tf(Timeframe::H1, 40.0, 10.0),
            make_tf(Timeframe::D1, 85.0, 5.0),
        ];
        let r = mtf_confirm(&scores);
        // D1 bottom > 80 → +5 boost
        assert!(r.details.iter().any(|d| d.contains("boost")));
        assert!(r.bottom_score > 60.0);
    }

    #[test]
    fn empty_input() {
        let r = mtf_confirm(&[]);
        assert_eq!(r.tf_count, 0);
        assert_eq!(r.bottom_score, 0.0);
        assert_eq!(r.bottom_signal, TbmSignal::None);
    }
}

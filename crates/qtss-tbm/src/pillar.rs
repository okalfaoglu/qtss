//! Pillar trait ve ortak tipler.

use serde::{Deserialize, Serialize};

/// Her pillar 0.0–100.0 arası skor üretir.
/// 0 = nötr/sinyal yok, 100 = çok güçlü reversal sinyali.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PillarScore {
    pub kind: PillarKind,
    /// 0.0 – 100.0
    pub score: f64,
    /// Pillar ağırlığı (toplam skora katkı oranı)
    pub weight: f64,
    /// İnsan-okunur açıklama
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PillarKind {
    Momentum,
    Volume,
    Structure,
    Onchain,
}

impl PillarScore {
    pub fn weighted(&self) -> f64 {
        self.score * self.weight
    }
}

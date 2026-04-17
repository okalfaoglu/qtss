//! AI score [0..1] → 0-10 tier + Turkish label + progress bar.
//!
//! CLAUDE.md #1: thresholds come from config (`public_card.tier.*`);
//! no hardcoded numbers, no if/else chains — a static dispatch table
//! ordered by descending minimum score.

use serde::{Deserialize, Serialize};

/// Public-facing tier shown in user cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreTier {
    Zayif,     // 0-3
    Orta,      // 4-5
    Guclu,     // 6-7
    CokGuclu,  // 8-9
    Mukemmel,  // 10
}

impl ScoreTier {
    /// Compact code used in logs / analytics.
    pub fn code(self) -> &'static str {
        match self {
            Self::Zayif => "zayif",
            Self::Orta => "orta",
            Self::Guclu => "guclu",
            Self::CokGuclu => "cok_guclu",
            Self::Mukemmel => "mukemmel",
        }
    }

    /// User-visible Turkish label.
    pub fn label_tr(self) -> &'static str {
        match self {
            Self::Zayif => "ZAYIF",
            Self::Orta => "ORTA",
            Self::Guclu => "GÜÇLÜ",
            Self::CokGuclu => "ÇOK GÜÇLÜ",
            Self::Mukemmel => "MÜKEMMEL",
        }
    }

    /// Numeric tier used in the "N/10 LABEL" stamp.
    /// We keep it inside one representative value per tier so the UI
    /// doesn't render e.g. "4/10 GÜÇLÜ" — the mapping is tier → anchor.
    pub fn anchor_out_of_ten(self) -> u8 {
        match self {
            Self::Zayif => 3,
            Self::Orta => 5,
            Self::Guclu => 7,
            Self::CokGuclu => 9,
            Self::Mukemmel => 10,
        }
    }
}

/// Config-driven thresholds for tier mapping. All four values live
/// under `notify.public_card.tier.*` in `system_config`.
///
/// The mapping is: score < orta_min → Zayif; < guclu_min → Orta;
/// < cok_guclu_min → Guclu; < mukemmel_min → CokGuclu; else Mukemmel.
#[derive(Debug, Clone, Copy)]
pub struct TierThresholds {
    pub orta_min: f64,
    pub guclu_min: f64,
    pub cok_guclu_min: f64,
    pub mukemmel_min: f64,
}

impl TierThresholds {
    /// Safe defaults in case config fetch fails. Match migration 0136.
    pub const FALLBACK: Self = Self {
        orta_min: 0.40,
        guclu_min: 0.55,
        cok_guclu_min: 0.70,
        mukemmel_min: 0.85,
    };

    /// Map a score in [0,1] to a tier using the dispatch table below.
    pub fn classify(&self, score: f64) -> ScoreTier {
        // Ordered high→low so the first match wins.
        let rules: [(f64, ScoreTier); 4] = [
            (self.mukemmel_min, ScoreTier::Mukemmel),
            (self.cok_guclu_min, ScoreTier::CokGuclu),
            (self.guclu_min, ScoreTier::Guclu),
            (self.orta_min, ScoreTier::Orta),
        ];
        rules
            .iter()
            .find(|(min, _)| score >= *min)
            .map(|(_, t)| *t)
            .unwrap_or(ScoreTier::Zayif)
    }
}

/// Compact scoreboard payload — what a renderer actually needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierBadge {
    pub tier: ScoreTier,
    pub out_of_ten: u8,
    pub label_tr: String,
    /// ASCII/Unicode progress bar, length = 10.
    pub bar: String,
    /// Raw AI score for debugging / power users.
    pub raw_score: f64,
}

impl TierBadge {
    pub fn build(raw_score: f64, thresholds: &TierThresholds) -> Self {
        let clamped = raw_score.clamp(0.0, 1.0);
        let tier = thresholds.classify(clamped);
        let out_of_ten = tier.anchor_out_of_ten();
        Self {
            tier,
            out_of_ten,
            label_tr: tier.label_tr().to_string(),
            bar: render_bar(out_of_ten, 10),
            raw_score: clamped,
        }
    }
}

/// Render a progress bar like `■■■■■■■□□□` of fixed width.
pub fn render_bar(filled: u8, width: u8) -> String {
    let filled = filled.min(width) as usize;
    let empty = width as usize - filled;
    "■".repeat(filled) + &"□".repeat(empty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_with_fallback_thresholds() {
        let t = TierThresholds::FALLBACK;
        assert_eq!(t.classify(0.10), ScoreTier::Zayif);
        assert_eq!(t.classify(0.39), ScoreTier::Zayif);
        assert_eq!(t.classify(0.40), ScoreTier::Orta);
        assert_eq!(t.classify(0.54), ScoreTier::Orta);
        assert_eq!(t.classify(0.55), ScoreTier::Guclu);
        assert_eq!(t.classify(0.69), ScoreTier::Guclu);
        assert_eq!(t.classify(0.70), ScoreTier::CokGuclu);
        assert_eq!(t.classify(0.84), ScoreTier::CokGuclu);
        assert_eq!(t.classify(0.85), ScoreTier::Mukemmel);
        assert_eq!(t.classify(1.00), ScoreTier::Mukemmel);
    }

    #[test]
    fn clamped_scores_are_safe() {
        let t = TierThresholds::FALLBACK;
        let b = TierBadge::build(-0.5, &t);
        assert_eq!(b.tier, ScoreTier::Zayif);
        assert_eq!(b.raw_score, 0.0);
        let b = TierBadge::build(9.9, &t);
        assert_eq!(b.tier, ScoreTier::Mukemmel);
        assert_eq!(b.raw_score, 1.0);
    }

    #[test]
    fn bar_has_correct_width() {
        let b = TierBadge::build(0.78, &TierThresholds::FALLBACK);
        assert_eq!(b.bar.chars().count(), 10);
        assert_eq!(b.tier, ScoreTier::CokGuclu);
        assert_eq!(b.out_of_ten, 9);
    }

    #[test]
    fn labels_are_turkish() {
        assert_eq!(ScoreTier::Mukemmel.label_tr(), "MÜKEMMEL");
        assert_eq!(ScoreTier::CokGuclu.label_tr(), "ÇOK GÜÇLÜ");
    }
}

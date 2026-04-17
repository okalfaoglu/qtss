//! Position Health Score — 0..100 composite score computed every tick
//! by the setup watcher, persisted only when the band index changes
//! (healthy ↔ warn ↔ danger ↔ critical).
//!
//! CLAUDE.md #1 — band classification uses an ordered dispatch table;
//! no if/else chains. #2 — all weights and band thresholds come from
//! `system_config` via [`load_health_weights`] / [`load_health_bands`].
//!
//! Component values arrive as `0..100` floats; this module is pure
//! arithmetic + classification. The feature sources (momentum index,
//! pattern integrity, orderbook imbalance, etc.) are populated
//! progressively in later Faz 9.7.x patches.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

const MODULE: &str = "notify";

// ---------------------------------------------------------------------------
// Bands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthBand {
    Healthy,
    Warn,
    Danger,
    Critical,
}

impl HealthBand {
    pub fn code(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Warn => "warn",
            Self::Danger => "danger",
            Self::Critical => "critical",
        }
    }

    /// Monotone index so `prev_idx - curr_idx` tells a watcher how many
    /// bands were crossed and in which direction (positive = worsening).
    pub fn index(self) -> u8 {
        match self {
            Self::Healthy => 0,
            Self::Warn => 1,
            Self::Danger => 2,
            Self::Critical => 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Config structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct HealthWeights {
    pub momentum: f64,
    pub structural: f64,
    pub orderbook: f64,
    pub regime: f64,
    pub correlation: f64,
    pub ai_rescore: f64,
}

impl HealthWeights {
    pub const FALLBACK: Self = Self {
        momentum: 0.25,
        structural: 0.25,
        orderbook: 0.15,
        regime: 0.15,
        correlation: 0.10,
        ai_rescore: 0.10,
    };

    /// Sum of all weights — should be ~1.0 but not enforced; compute
    /// normalises by this so misconfiguration doesn't bias the score.
    pub fn sum(&self) -> f64 {
        self.momentum
            + self.structural
            + self.orderbook
            + self.regime
            + self.correlation
            + self.ai_rescore
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HealthBands {
    pub healthy_min: f64,
    pub warn_min: f64,
    pub danger_min: f64,
}

impl HealthBands {
    pub const FALLBACK: Self = Self {
        healthy_min: 70.0,
        warn_min: 50.0,
        danger_min: 30.0,
    };

    /// Classify via a descending dispatch table (CLAUDE.md #1).
    pub fn classify(&self, score: f64) -> HealthBand {
        let rules: [(f64, HealthBand); 3] = [
            (self.healthy_min, HealthBand::Healthy),
            (self.warn_min, HealthBand::Warn),
            (self.danger_min, HealthBand::Danger),
        ];
        rules
            .iter()
            .find(|(min, _)| score >= *min)
            .map(|(_, b)| *b)
            .unwrap_or(HealthBand::Critical)
    }
}

// ---------------------------------------------------------------------------
// Score computation
// ---------------------------------------------------------------------------

/// Component values — each in `[0, 100]`. Missing components are
/// `None` and drop out of the weighted sum (their weight gets
/// redistributed proportionally across the present components).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct HealthComponents {
    pub momentum: Option<f64>,
    pub structural: Option<f64>,
    pub orderbook: Option<f64>,
    pub regime: Option<f64>,
    pub correlation: Option<f64>,
    pub ai_rescore: Option<f64>,
}

/// Result of a health computation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HealthScore {
    pub total: f64,
    pub band: HealthBand,
    pub components: HealthComponents,
}

/// Compute the weighted score from available components. Weights are
/// renormalised across present components so a missing source doesn't
/// silently drop the score. Returns `0.0` / Critical if *no* component
/// is provided — the caller decides whether to persist.
pub fn compute(
    weights: &HealthWeights,
    components: &HealthComponents,
    bands: &HealthBands,
) -> HealthScore {
    // Pair component values with their weights, skipping missing ones.
    let pairs: [(Option<f64>, f64); 6] = [
        (components.momentum, weights.momentum),
        (components.structural, weights.structural),
        (components.orderbook, weights.orderbook),
        (components.regime, weights.regime),
        (components.correlation, weights.correlation),
        (components.ai_rescore, weights.ai_rescore),
    ];
    let (weighted, weight_sum) =
        pairs
            .iter()
            .filter_map(|(v, w)| v.map(|x| (x.clamp(0.0, 100.0) * w, *w)))
            .fold((0.0_f64, 0.0_f64), |(ws, wt), (x, w)| (ws + x, wt + w));
    let total = if weight_sum > 0.0 {
        weighted / weight_sum
    } else {
        0.0
    };
    let band = bands.classify(total);
    HealthScore { total, band, components: *components }
}

// ---------------------------------------------------------------------------
// Config loaders
// ---------------------------------------------------------------------------

pub async fn load_health_weights(pool: &PgPool) -> HealthWeights {
    let f = HealthWeights::FALLBACK;
    HealthWeights {
        momentum: qtss_storage::resolve_system_f64(
            pool, MODULE, "health.weight.momentum",
            "QTSS_HEALTH_W_MOMENTUM", f.momentum,
        ).await,
        structural: qtss_storage::resolve_system_f64(
            pool, MODULE, "health.weight.structural",
            "QTSS_HEALTH_W_STRUCTURAL", f.structural,
        ).await,
        orderbook: qtss_storage::resolve_system_f64(
            pool, MODULE, "health.weight.orderbook",
            "QTSS_HEALTH_W_ORDERBOOK", f.orderbook,
        ).await,
        regime: qtss_storage::resolve_system_f64(
            pool, MODULE, "health.weight.regime",
            "QTSS_HEALTH_W_REGIME", f.regime,
        ).await,
        correlation: qtss_storage::resolve_system_f64(
            pool, MODULE, "health.weight.correlation",
            "QTSS_HEALTH_W_CORRELATION", f.correlation,
        ).await,
        ai_rescore: qtss_storage::resolve_system_f64(
            pool, MODULE, "health.weight.ai_rescore",
            "QTSS_HEALTH_W_AI_RESCORE", f.ai_rescore,
        ).await,
    }
}

pub async fn load_health_bands(pool: &PgPool) -> HealthBands {
    let f = HealthBands::FALLBACK;
    HealthBands {
        healthy_min: qtss_storage::resolve_system_f64(
            pool, MODULE, "health.band.healthy_min",
            "QTSS_HEALTH_BAND_HEALTHY_MIN", f.healthy_min,
        ).await,
        warn_min: qtss_storage::resolve_system_f64(
            pool, MODULE, "health.band.warn_min",
            "QTSS_HEALTH_BAND_WARN_MIN", f.warn_min,
        ).await,
        danger_min: qtss_storage::resolve_system_f64(
            pool, MODULE, "health.band.danger_min",
            "QTSS_HEALTH_BAND_DANGER_MIN", f.danger_min,
        ).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_classify_via_dispatch_table() {
        let b = HealthBands::FALLBACK;
        assert_eq!(b.classify(95.0), HealthBand::Healthy);
        assert_eq!(b.classify(70.0), HealthBand::Healthy);
        assert_eq!(b.classify(69.9), HealthBand::Warn);
        assert_eq!(b.classify(50.0), HealthBand::Warn);
        assert_eq!(b.classify(49.9), HealthBand::Danger);
        assert_eq!(b.classify(30.0), HealthBand::Danger);
        assert_eq!(b.classify(29.9), HealthBand::Critical);
        assert_eq!(b.classify(0.0), HealthBand::Critical);
    }

    #[test]
    fn compute_weighted_with_all_components() {
        let w = HealthWeights::FALLBACK;
        let b = HealthBands::FALLBACK;
        let c = HealthComponents {
            momentum: Some(80.0),
            structural: Some(70.0),
            orderbook: Some(60.0),
            regime: Some(75.0),
            correlation: Some(50.0),
            ai_rescore: Some(65.0),
        };
        let s = compute(&w, &c, &b);
        // weighted = 80*.25 + 70*.25 + 60*.15 + 75*.15 + 50*.10 + 65*.10
        //         = 20 + 17.5 + 9 + 11.25 + 5 + 6.5 = 69.25
        assert!((s.total - 69.25).abs() < 0.01);
        assert_eq!(s.band, HealthBand::Warn);
    }

    #[test]
    fn missing_components_rescale() {
        let w = HealthWeights::FALLBACK;
        let b = HealthBands::FALLBACK;
        // Only momentum + structural present → should be weighted mean of those two.
        let c = HealthComponents {
            momentum: Some(100.0),
            structural: Some(0.0),
            ..Default::default()
        };
        let s = compute(&w, &c, &b);
        // (100*.25 + 0*.25) / (.25 + .25) = 50
        assert!((s.total - 50.0).abs() < 0.01);
        assert_eq!(s.band, HealthBand::Warn);
    }

    #[test]
    fn empty_components_yield_critical() {
        let s = compute(
            &HealthWeights::FALLBACK,
            &HealthComponents::default(),
            &HealthBands::FALLBACK,
        );
        assert_eq!(s.total, 0.0);
        assert_eq!(s.band, HealthBand::Critical);
    }

    #[test]
    fn band_index_is_monotone() {
        assert!(HealthBand::Healthy.index() < HealthBand::Warn.index());
        assert!(HealthBand::Warn.index() < HealthBand::Danger.index());
        assert!(HealthBand::Danger.index() < HealthBand::Critical.index());
    }
}

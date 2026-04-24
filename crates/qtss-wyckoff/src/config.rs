//! Wyckoff detector thresholds — every knob seeded from
//! `system_config.wyckoff.*` (CLAUDE.md #2).

use qtss_domain::v2::pivot::PivotLevel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WyckoffConfig {
    pub pivot_level: PivotLevel,
    pub min_structural_score: f32,

    /// Volume spike multiplier over the rolling SMA for climax/spring
    /// events (SC, BC, Spring). 3.0 × SMA is the Weis / Pruden
    /// convention for genuine climactic action.
    pub climax_volume_mult: f64,
    /// SMA length (bars) for the volume baseline.
    pub volume_sma_bars: u64,
    /// Bar-range expansion (current.range / ATR) required for climax
    /// events. Wide-range bar confirmation.
    pub climax_range_atr_mult: f64,

    /// Spring tolerance — how far below range-low a bar can wick
    /// before we stop calling it a Spring (fraction of range height).
    pub spring_wick_max_pct: f64,
    /// Spring reclaim window (bars) — bars inside which price must
    /// close back above range-low.
    pub spring_reclaim_bars: u64,

    /// Absorbed range width (max fraction of price) — beyond this the
    /// "range" isn't really a range, it's a trend.
    pub range_max_width_pct: f64,
    /// Minimum range duration (bars) before the tracker trusts it as
    /// a Wyckoff schematic candidate.
    pub range_min_bars: u64,

    /// SOS / SOW volume + range amplifier — a Sign of Strength needs
    /// volume × this and range × this over baseline.
    pub sos_amplifier: f64,

    pub scan_lookback: usize,
}

impl Default for WyckoffConfig {
    fn default() -> Self {
        Self {
            pivot_level: PivotLevel::L2,
            min_structural_score: 0.55,
            climax_volume_mult: 3.0,
            volume_sma_bars: 20,
            climax_range_atr_mult: 2.0,
            spring_wick_max_pct: 0.15,
            spring_reclaim_bars: 3,
            range_max_width_pct: 0.12,
            range_min_bars: 20,
            sos_amplifier: 1.5,
            scan_lookback: 200,
        }
    }
}

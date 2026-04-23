//! Resolver-wide configuration. Every threshold is seeded from
//! `system_config.targets.*` in the worker / API layer (CLAUDE.md #2).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetConfig {
    // ── Harmonic PRZ ──
    /// Fibonacci retracement levels for TP1/2/3 from D toward C along
    /// the CD leg. Classical Carney targets.
    pub harmonic_tp_fibs: Vec<f64>,
    /// Buffer beyond the PRZ high/low as SL (fraction of XA leg).
    pub harmonic_sl_buffer_pct: f64,

    // ── VProfile magnet ──
    /// Max distance (fraction of entry) before we stop listing a
    /// profile level as a target candidate.
    pub vprofile_max_distance_pct: f64,

    // ── Fibonacci extension ──
    /// Extension ratios (1.272 / 1.618 / 2.618) applied to the
    /// swing leg.
    pub fib_extensions: Vec<f64>,

    // ── Structural invalidation ──
    /// Number of same-direction pivots to use as TP ladder.
    pub structural_tp_count: usize,
    /// Buffer beyond the structural pivot as SL (fraction of price).
    pub structural_sl_buffer_pct: f64,

    // ── ATR band fallback ──
    /// Multipliers for TP1/2/3 in ATR units.
    pub atr_tp_multipliers: Vec<f64>,
    /// Multiplier for SL in ATR units.
    pub atr_sl_multiplier: f64,
    /// Minimum viable ATR — below this the resolver bails (flat
    /// markets produce garbage targets otherwise).
    pub atr_min_abs: f64,
}

impl Default for TargetConfig {
    fn default() -> Self {
        Self {
            harmonic_tp_fibs: vec![0.382, 0.618, 1.0],
            harmonic_sl_buffer_pct: 0.02,
            vprofile_max_distance_pct: 0.05,
            fib_extensions: vec![1.272, 1.618, 2.618],
            structural_tp_count: 3,
            structural_sl_buffer_pct: 0.005,
            atr_tp_multipliers: vec![1.5, 3.0, 5.0],
            atr_sl_multiplier: 1.0,
            atr_min_abs: 1e-9,
        }
    }
}

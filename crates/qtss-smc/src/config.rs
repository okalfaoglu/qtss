//! SMC detector configuration. Every threshold lives here (no
//! hardcoded constants in the event evaluators — CLAUDE.md #2). Defaults
//! seeded by migration 0227.

use qtss_domain::v2::pivot::PivotLevel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmcConfig {
    /// Pivot level the detector consumes. SMC events are usually read
    /// on the higher levels (L1/L2) — too much noise on L0.
    pub pivot_level: PivotLevel,
    /// Minimum structural score required before an event surfaces.
    /// 0..1. Matches the convention across the rest of the detectors.
    pub min_structural_score: f32,

    // ── BOS / CHoCH / MSS ───────────────────────────────────────────
    /// How many bars of price action the break-confirming close must
    /// be measured against. Pine uses 1 (single close > prior high);
    /// 2 reduces false breaks on doji / indecision bars.
    pub break_confirm_bars: usize,
    /// MSS adds a "break the close not just the extreme" rule. This
    /// is the extra cushion expressed as a fraction of the broken
    /// swing's range. 0.0 = just close > close; 0.002 = 0.2% beyond.
    pub mss_close_cushion_pct: f64,

    // ── Liquidity sweep ─────────────────────────────────────────────
    /// Sweep wick penetration (how far above/below the prior swing a
    /// valid sweep must reach), as fraction of price. Default 0.001 =
    /// 10 bps beyond the swing.
    pub sweep_wick_penetration_pct: f64,
    /// Sweep rejection (how far back below/above the sweep the close
    /// must recover), as fraction of the sweep excursion. Default 0.5
    /// — close must recover at least half the wick.
    pub sweep_reject_frac: f64,
    /// Bars in which the rejection must complete. Default 2 — tight
    /// to avoid misclassifying slow breakouts as sweeps.
    pub sweep_reject_bars: usize,

    // ── Fair Value Imbalance (FVI) ─────────────────────────────────
    /// Minimum gap size relative to ATR. Stricter than qtss-range's
    /// FVG default (0.5) — 0.8 filters all but the clearest
    /// imbalances.
    pub fvi_min_gap_atr_frac: f64,
    /// Middle-candle volume multiplier over SMA(volume). Default 1.5.
    pub fvi_volume_spike_mult: f64,
    /// How many recent bars to scan per tick.
    pub scan_lookback: usize,
}

impl Default for SmcConfig {
    fn default() -> Self {
        Self {
            pivot_level: PivotLevel::L1,
            min_structural_score: 0.55,
            break_confirm_bars: 1,
            mss_close_cushion_pct: 0.002,
            sweep_wick_penetration_pct: 0.001,
            sweep_reject_frac: 0.5,
            sweep_reject_bars: 2,
            fvi_min_gap_atr_frac: 0.8,
            fvi_volume_spike_mult: 1.5,
            scan_lookback: 100,
        }
    }
}

impl SmcConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&(self.min_structural_score as f64)) {
            return Err("min_structural_score must be in 0..=1".into());
        }
        if self.break_confirm_bars == 0 || self.break_confirm_bars > 10 {
            return Err("break_confirm_bars must be in 1..=10".into());
        }
        if !(0.0..=0.05).contains(&self.mss_close_cushion_pct) {
            return Err("mss_close_cushion_pct must be in 0..=0.05".into());
        }
        if !(0.0..=0.01).contains(&self.sweep_wick_penetration_pct) {
            return Err("sweep_wick_penetration_pct must be in 0..=0.01".into());
        }
        if !(0.0..=1.0).contains(&self.sweep_reject_frac) {
            return Err("sweep_reject_frac must be in 0..=1".into());
        }
        if self.sweep_reject_bars == 0 || self.sweep_reject_bars > 10 {
            return Err("sweep_reject_bars must be in 1..=10".into());
        }
        if !(0.0..=5.0).contains(&self.fvi_min_gap_atr_frac) {
            return Err("fvi_min_gap_atr_frac must be in 0..=5".into());
        }
        if self.fvi_volume_spike_mult < 1.0 {
            return Err("fvi_volume_spike_mult must be >= 1.0".into());
        }
        if self.scan_lookback < 10 {
            return Err("scan_lookback must be >= 10".into());
        }
        Ok(())
    }
}

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

    /// FAZ 25.4.A — Phase-progression knobs for the previously-stub
    /// AR / ST / LPS / PS / BU / Test evaluators.
    /// AR = Automatic Rally: window after SC to look for the bounce
    /// high (canonical: 5-15 bars).
    pub ar_window_bars: u64,
    /// ST = Secondary Test: how close to SC low (fraction of price)
    /// the retest bar must come.
    pub st_proximity_pct: f64,
    /// ST = Secondary Test: max volume relative to SMA. Real ST
    /// happens on REDUCED volume — that's the whole signal.
    pub st_volume_max_mult: f64,
    /// LPS = Last Point of Support: forward window from SOS to look
    /// for the higher-low.
    pub lps_lookforward_bars: u64,

    /// FAZ 26 backlog (B-CTX-MM-1) — volume gate for Phase-C events.
    /// Real Wyckoff Spring / UTAD requires high volume on the
    /// wick bar (capitulation selling / blow-off buying). Below
    /// this multiple of the SMA baseline the event is suppressed
    /// — too thin to be a real climactic action.
    pub spring_min_volume_mult: f64,
    /// Soft ceiling — volume multiples >= this get the maximum
    /// score boost. Linear scaling between min and max.
    pub spring_max_volume_mult: f64,
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
            ar_window_bars: 12,
            st_proximity_pct: 0.015,
            st_volume_max_mult: 0.85,
            lps_lookforward_bars: 30,
            // FAZ 26 backlog (B-CTX-MM-1) — Wyckoff doctrine: real
            // Spring fires on HEAVY volume (sellers exhausted on the
            // wick). 1.0 = baseline-or-better gate; below that the
            // event is suppressed entirely. 2.5 = saturate at max
            // score (3×+ baseline = textbook climactic).
            spring_min_volume_mult: 1.0,
            spring_max_volume_mult: 2.5,
        }
    }
}

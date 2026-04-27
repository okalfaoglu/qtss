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
            // 2026-04-27 chart audit — was 2.0; tightened too far,
            // 1d/1w climax bars often fail with that. ATR×1.5 is the
            // Weis convention.
            climax_range_atr_mult: 1.5,
            // 2026-04-27 chart audit — was 0.15 (15%). Real Wyckoff
            // Springs on higher TFs (1d/1w) routinely pierce 30-45%
            // of the trading range as the final shakeout flush. The
            // tight 15% gate suppressed every multi-month BTC Spring
            // visible to the eye on 1w. 0.40 admits genuine
            // climactic flushes while still rejecting all-out
            // breakdowns (>40% pierce = trend continuation, not
            // Spring).
            spring_wick_max_pct: 0.40,
            // 2026-04-27 chart audit — was 3 bars. On 1w that's a
            // 3-week reclaim; on 4h it's only 12 hours. 5 bars is a
            // better universal default (≈ 1 swing across timeframes).
            spring_reclaim_bars: 5,
            range_max_width_pct: 0.12,
            range_min_bars: 20,
            sos_amplifier: 1.5,
            scan_lookback: 200,
            ar_window_bars: 12,
            st_proximity_pct: 0.015,
            st_volume_max_mult: 0.85,
            lps_lookforward_bars: 30,
            // 2026-04-27 chart audit — was 1.0 floor. Genuine
            // Springs at 1w / 1d sometimes have volume slightly
            // below SMA when the broader market is quiet (the
            // SHAKEOUT version of a Spring, less climactic).
            // Lowered to 0.85 so muted-volume shakeouts still fire.
            spring_min_volume_mult: 0.85,
            // 2026-04-27 chart audit — was 2.5. 1w / 1d Springs
            // routinely have 4-6× SMA volume during major
            // capitulations; capping at 2.5 disqualified them.
            // 5.0 saturates the score at 5× and lets the very
            // heavy bars through.
            spring_max_volume_mult: 5.0,
        }
    }
}

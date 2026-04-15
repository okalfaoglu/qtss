//! Detector configuration.

use crate::error::{WyckoffError, WyckoffResult};
use qtss_domain::v2::pivot::PivotLevel;

#[derive(Debug, Clone)]
pub struct WyckoffConfig {
    // --- Range detection ---
    /// Which pivot level to consume.
    pub pivot_level: PivotLevel,
    /// Minimum number of pivots for a valid trading range.
    pub min_range_pivots: usize,
    /// Max edge deviation as fraction of range midpoint (0.04 = 4%).
    pub range_edge_tolerance: f64,
    /// Volume multiplier for "climactic" pivot (SC / BC).
    pub climax_volume_mult: f64,
    /// Min penetration for Spring / Upthrust (fraction of range height).
    pub min_penetration: f64,
    /// Max penetration for an *ordinary* Spring/UTAD before it's
    /// treated as a genuine breakout. Villahermosa: ordinary Springs
    /// pierce <=5-10% of range height; 30% was letting true breakouts
    /// register as Springs. Shakeouts (aggressive variant #1) have
    /// their own deeper cap in `shakeout_max_penetration`.
    pub max_penetration: f64,
    /// Upper bound for Shakeout penetration specifically. Shakeouts
    /// legitimately pierce deeper than ordinary Springs.
    pub shakeout_max_penetration: f64,
    /// Drop candidates below this structural score.
    pub min_structural_score: f32,
    /// P8 — maximum pivots fed to the detector (trailing window). Without
    /// a cap, `tree.at_level()` returns all pivots since asset inception
    /// and `from_pivots` resolves to ATL↔ATH — which made BTC 1d ranges
    /// read as 3621→126208. Canonical Wyckoff ranges span 15–40 pivots
    /// (Villahermosa ch. 4); default 40 keeps room for PS+A+B+C+D while
    /// cutting multi-cycle noise.
    pub pivot_window: usize,

    // --- Phase A: Stopping ---
    /// SC/BC volume must be >= Nx average volume.
    pub sc_volume_multiplier: f64,
    /// SC/BC bar range must be >= Nx ATR.
    pub sc_bar_width_multiplier: f64,
    /// ST volume must be <= Nx SC volume.
    pub st_max_volume_ratio: f64,
    /// AR must retrace >= N% of SC drop/rally.
    pub ar_min_retracement: f64,

    // --- Phase B ---
    /// UA may exceed AR level by at most N%.
    pub ua_max_exceed_pct: f64,
    /// Each ST-B volume must be <= N * previous ST-B volume.
    pub stb_volume_decay_min: f64,
    /// Minimum bars that must elapse between Phase A completion (last SC/AR/ST)
    /// and the earliest eligible Phase-B → Phase-C transition. Villahermosa
    /// ch. 5–6: Phase B is the longest phase and cannot be skipped. Default
    /// 10 bars; callers override per-TF via config table.
    pub phase_b_min_bars: usize,
    /// Phase B requires at least this many internal tests (UA, ST-B, ST)
    /// beyond the Phase-A canonical triple before C may open. Default 1 —
    /// i.e. climax + AR + (ST or UA or STB) gives you A→B; B→C then needs
    /// one *more* inner test on top.
    pub phase_b_min_inner_tests: usize,

    // --- Phase C ---
    /// Shakeout penetration >= N% of range height.
    pub shakeout_min_penetration: f64,
    /// Shakeout must recover within N bars.
    pub shakeout_recovery_bars: usize,
    /// Spring/UTAD: minimum prior edge tests (lows near support, highs
    /// near resistance) that must already exist in the context before a
    /// Phase-C manipulation can fire. Stops every pullback from being
    /// flagged as a Spring in trending markets.
    pub manipulation_min_edge_tests: usize,
    /// Spring/UTAD: minimum bar-index span between the first support /
    /// resistance test and the manipulation candidate. Ensures the
    /// range has existed long enough to be a real Wyckoff range.
    pub manipulation_min_range_age_bars: u64,
    /// P20 — maximum allowed slope of same-kind pivot prices over the
    /// context window, expressed as fraction of mean price per pivot
    /// step. A Spring/UTAD is a FALSE BREAK of a HORIZONTAL edge. If
    /// the low-pivot series (for Spring) or high-pivot series (for
    /// UTAD) is itself trending (slope > this cap), the "edge" is not
    /// horizontal and any pierce is just a trend pullback, not a
    /// Wyckoff manipulation. Default 0.004 = 0.4%/pivot — catches
    /// higher-lows / lower-highs sequences in trending markets.
    pub manipulation_max_edge_slope: f64,
    /// Reject ranges whose height (resistance - support) exceeds this
    /// fraction of the midpoint price. Prevents an H1 detector from
    /// surfacing a multi-month D1-scale range as "valid Wyckoff". The
    /// caller sets this per-TF via config (H1: 0.08, H4: 0.15, D1: 0.30).
    pub max_range_height_pct: f64,
    /// Reject ranges whose first-to-last pivot span exceeds this bar
    /// count. Another TF guard: a "range" older than N bars on a given
    /// TF has almost certainly already resolved — continuing to evaluate
    /// it produces stale setups. Set per-TF by caller.
    pub max_range_age_bars: u64,

    /// Range-quality: max ratio of late-window to early-window pivot
    /// volume. > 1.0 means volume is expanding through the range —
    /// that's a trending market, not Wyckoff accumulation. Canonical
    /// Wyckoff ranges exhibit VOLUME CONTRACTION ("drying up") as
    /// the composite operator finishes absorbing supply. Reject the
    /// range if late/early ratio exceeds this cap.
    pub max_range_volume_expansion: f64,

    // --- Phase C: Spring / UTAD test ---
    /// Spring/UTAD Test: retest bar volume must be <= N * Spring/UTAD
    /// volume. Villahermosa ch. 6 — the test is a LOW-VOLUME return to
    /// the prior pierce; high-volume retest = still-active supply/demand
    /// and the setup is not confirmed. Default 0.6 (60% of Spring vol).
    pub spring_test_max_vol_ratio: f64,
    /// Spring/UTAD Test must fire within this many bars of the parent
    /// Spring/UTAD. Beyond this window the pullback is a separate move.
    pub spring_test_window_bars: u64,
    /// Maximum distance (as fraction of range height) between the test
    /// low and the parent Spring low. Too far away = different swing,
    /// not a test. Default 0.10 (within 10% of range height).
    pub spring_test_max_distance: f64,

    // --- Phase C: Spring variant (Pruden) ---
    /// A "No-Supply" Spring (#3): climax bar volume <= N x avg_vol.
    /// The hallmark of the highest-probability Spring — absence of
    /// supply at the break confirms sellers are exhausted.
    pub spring_no_supply_vol_ratio: f64,
    /// A "Terminal" Spring (#1): climax bar volume >= N x avg_vol.
    /// Ultra-high volume break = composite operator still absorbing;
    /// statistically the weakest of the three variants for entry.
    pub spring_terminal_vol_ratio: f64,
    /// Skip #1 Terminal Springs (aggressive / weakest). Default: true.
    /// Ordinary (#2) and No-Supply (#3) are always kept.
    pub skip_terminal_springs: bool,

    // --- Phase D ---
    /// SOS/SOW volume must be >= Nx average.
    pub sos_min_volume_ratio: f64,
    /// SOS/SOW bar range must be >= Nx ATR proxy. Villahermosa ch. 7:
    /// wide-range bar is the single crisp numeric rule for SOS/SOW. A
    /// narrow-range pivot above creek is NOT a SOS.
    pub sos_min_bar_width_atr_mult: f64,
    /// SOS close must sit in the upper `(1 - N)` of its bar range
    /// (i.e. close_pos >= N). Default 0.66 = upper third. Mirror for SOW
    /// (close_pos <= 1 - N = lower third).
    pub sos_close_third_threshold: f64,
    /// LPS retracement <= N% of SOS move.
    pub lps_max_retracement: f64,
    /// LPS volume must be <= N% of SOS volume.
    pub lps_max_volume_ratio: f64,
    /// Creek = N percentile of range height (from support).
    pub creek_level_percentile: f64,
    /// JAC body ratio: body (|close-open|) must be >= N * bar_range to
    /// count as a true Jump-Across-Creek. Tiny wicks that spike above
    /// creek but close back near the middle are not JACs. Default 0.5.
    pub jac_min_body_ratio: f64,
    /// Phase B mid-range climactic-volume flip: any Phase-B bar with
    /// volume >= N * avg_volume flips schematic to Distribution /
    /// ReDistribution (Villahermosa: unexpected vol peaks inside the
    /// range signal distributive character). Default 3.0. Set 0 to
    /// disable the flip entirely.
    pub phase_b_climactic_vol_flip_mult: f64,

    // --- Sloping structures ---
    /// Range slope > N degrees → sloping structure.
    pub slope_threshold_deg: f64,

    // --- SOT ---
    /// Each SOS/SOW thrust must be <= N * prev thrust for SOT.
    pub sot_thrust_decay_ratio: f64,
}

impl WyckoffConfig {
    pub fn defaults() -> Self {
        Self {
            pivot_level: PivotLevel::L1,
            min_range_pivots: 5,
            range_edge_tolerance: 0.04,
            climax_volume_mult: 1.8,
            min_penetration: 0.02,
            max_penetration: 0.12,          // P2-P1-#5: tightened from 0.30
            shakeout_max_penetration: 0.30,
            min_structural_score: 0.50,
            pivot_window: 40,
            // Phase A
            sc_volume_multiplier: 2.5,
            sc_bar_width_multiplier: 2.0,
            st_max_volume_ratio: 0.7,
            ar_min_retracement: 0.3,
            // Phase B
            ua_max_exceed_pct: 0.03,
            stb_volume_decay_min: 0.85,
            phase_b_min_bars: 10,
            phase_b_min_inner_tests: 1,
            // Phase C
            shakeout_min_penetration: 0.05,
            shakeout_recovery_bars: 3,
            manipulation_min_edge_tests: 3,          // P20 — was 2, too permissive
            manipulation_min_range_age_bars: 20,     // P20 — was 10, too permissive
            manipulation_max_edge_slope: 0.004,      // P20 — 0.4%/pivot max slope
            // TF guards — defaults sized for crypto H4; caller overrides
            // per-TF from config table.
            max_range_height_pct: 0.15,
            max_range_age_bars: 500,
            // Canonical Wyckoff ranges exhibit volume contraction;
            // 1.3 allows up to ~30% expansion (mild noise) before reject.
            max_range_volume_expansion: 1.3,
            // Phase C test
            spring_test_max_vol_ratio: 0.6,
            spring_test_window_bars: 8,
            spring_test_max_distance: 0.10,
            // Spring variant (Pruden): no-supply = low vol, terminal = very high vol
            spring_no_supply_vol_ratio: 0.8,
            spring_terminal_vol_ratio: 3.0,
            skip_terminal_springs: true,
            // Phase D
            sos_min_volume_ratio: 1.5,
            sos_min_bar_width_atr_mult: 1.5,
            sos_close_third_threshold: 0.66,
            lps_max_retracement: 0.5,
            lps_max_volume_ratio: 0.5,
            creek_level_percentile: 0.6,
            jac_min_body_ratio: 0.5,
            phase_b_climactic_vol_flip_mult: 3.0,
            // Sloping
            slope_threshold_deg: 5.0,
            // SOT
            sot_thrust_decay_ratio: 0.7,
        }
    }

    pub fn validate(&self) -> WyckoffResult<()> {
        if self.min_range_pivots < 4 {
            return Err(WyckoffError::InvalidConfig(
                "min_range_pivots must be >= 4".into(),
            ));
        }
        if !(0.0..=0.25).contains(&self.range_edge_tolerance) {
            return Err(WyckoffError::InvalidConfig(
                "range_edge_tolerance must be in 0..=0.25".into(),
            ));
        }
        if self.climax_volume_mult <= 1.0 {
            return Err(WyckoffError::InvalidConfig(
                "climax_volume_mult must be > 1.0".into(),
            ));
        }
        if !(self.min_penetration < self.max_penetration) {
            return Err(WyckoffError::InvalidConfig(
                "min_penetration must be < max_penetration".into(),
            ));
        }
        if !(0.0..=1.0).contains(&(self.min_structural_score as f64)) {
            return Err(WyckoffError::InvalidConfig(
                "min_structural_score must be in 0..=1".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.max_range_height_pct) {
            return Err(WyckoffError::InvalidConfig(
                "max_range_height_pct must be in 0..=1".into(),
            ));
        }
        if self.spring_no_supply_vol_ratio >= self.spring_terminal_vol_ratio {
            return Err(WyckoffError::InvalidConfig(
                "spring_no_supply_vol_ratio must be < spring_terminal_vol_ratio".into(),
            ));
        }
        Ok(())
    }
}

//! Faz C — Early Warning Quorum.
//!
//! Formation-agnostic per-tick layer that watches an armed/active
//! setup for symptoms of reversal **against** its direction. Five
//! independent, pure detectors vote; the aggregate verdict is
//! either `None`, `Warn` (soft, ≥ `warn_quorum`) or `Exit`
//! (hard, ≥ `exit_quorum`).
//!
//! Design notes:
//!  * Detectors are direction-aware but **formation-agnostic** —
//!    they see only price/volume/momentum/regime, so the same layer
//!    protects harmonic, elliott, classical, wyckoff and range
//!    setups identically.
//!  * Runs regardless of `tp1_hit`. When an `Exit` verdict lands on
//!    a post-TP1 setup the caller maps the close to `Scratch`
//!    (Faz A); pre-TP1 it lands as a standard invalidated loss.
//!  * No DB / IO here. All inputs land via `WarningContext`; all
//!    knobs via `WarningConfig`.

use crate::types::Direction;

/// Individual warning fired by a detector. Persisted to
/// `qtss_setup_events.payload` under `"warnings": [..]` so an
/// operator can see exactly which mix triggered an exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WarningSignal {
    /// Regular momentum divergence against trade direction
    /// (price HH + indicator LH for a long).
    Divergence,
    /// Climactic volume bar against trade direction
    /// (volume ≥ avg × mult, close against position).
    VolumeReversal,
    /// Last confirmed pivot on trade-side was breached
    /// (close < pivot_low for a long, etc.).
    MicroStructureBreak,
    /// Reversal candlestick on the latest closed bar
    /// (bearish engulfing for long, bullish engulfing for short).
    CandleReversal,
    /// Latest confluence direction flipped to the opposite side
    /// with guven ≥ threshold.
    RegimeFlip,
}

impl WarningSignal {
    pub fn as_str(self) -> &'static str {
        match self {
            WarningSignal::Divergence => "divergence",
            WarningSignal::VolumeReversal => "volume_reversal",
            WarningSignal::MicroStructureBreak => "micro_structure_break",
            WarningSignal::CandleReversal => "candle_reversal",
            WarningSignal::RegimeFlip => "regime_flip",
        }
    }
}

/// Aggregate quorum result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningAction {
    /// Below warn threshold — do nothing.
    None,
    /// ≥ `warn_quorum` and < `exit_quorum` — log but don't close.
    Warn,
    /// ≥ `exit_quorum` — close the setup early.
    Exit,
}

#[derive(Debug, Clone)]
pub struct WarningVerdict {
    pub triggered: Vec<WarningSignal>,
    pub action: WarningAction,
}

/// Per-tick inputs. Slices are chronological (oldest first).
/// Any missing slice can be empty — the relevant detector simply
/// returns `false`.
pub struct WarningContext<'a> {
    pub direction: Direction,
    pub opens: &'a [f64],
    pub highs: &'a [f64],
    pub lows: &'a [f64],
    pub closes: &'a [f64],
    pub volumes: &'a [f64],
    /// Momentum series for the divergence detector (MACD histogram,
    /// RSI, or any mean-reverting momentum proxy). Same length as
    /// `closes` recommended.
    pub momentum: &'a [f64],
    /// Latest confluence direction + guven for `RegimeFlip`.
    pub regime_direction: Direction,
    pub regime_guven: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct WarningConfig {
    pub warn_quorum: usize,
    pub exit_quorum: usize,
    pub divergence_enabled: bool,
    pub divergence_lookback_bars: usize,
    pub divergence_pivot_left: usize,
    pub divergence_pivot_right: usize,
    pub volume_enabled: bool,
    pub volume_spike_mult: f64,
    pub volume_lookback_bars: usize,
    /// How many of the most recent bars count as "just happened"
    /// when checking for a volume spike (default 3).
    pub volume_recent_bars: usize,
    pub micro_enabled: bool,
    pub micro_pivot_left: usize,
    pub micro_pivot_right: usize,
    pub candle_enabled: bool,
    pub regime_enabled: bool,
    pub regime_guven_threshold: f64,
}

/// Run every enabled detector and build a verdict. Pure.
pub fn evaluate_warnings(ctx: &WarningContext, cfg: &WarningConfig) -> WarningVerdict {
    let mut triggered: Vec<WarningSignal> = Vec::with_capacity(5);
    if cfg.divergence_enabled && detect_divergence(ctx, cfg) {
        triggered.push(WarningSignal::Divergence);
    }
    if cfg.volume_enabled && detect_volume_reversal(ctx, cfg) {
        triggered.push(WarningSignal::VolumeReversal);
    }
    if cfg.micro_enabled && detect_micro_break(ctx, cfg) {
        triggered.push(WarningSignal::MicroStructureBreak);
    }
    if cfg.candle_enabled && detect_candle_reversal(ctx) {
        triggered.push(WarningSignal::CandleReversal);
    }
    if cfg.regime_enabled && detect_regime_flip(ctx, cfg) {
        triggered.push(WarningSignal::RegimeFlip);
    }
    let n = triggered.len();
    let action = if cfg.exit_quorum > 0 && n >= cfg.exit_quorum {
        WarningAction::Exit
    } else if cfg.warn_quorum > 0 && n >= cfg.warn_quorum {
        WarningAction::Warn
    } else {
        WarningAction::None
    };
    WarningVerdict { triggered, action }
}

// ------------------------------------------------------------------
// Detectors — each is direction-aware + pure.
// ------------------------------------------------------------------

/// Bearish regular divergence for long (price HH, momentum LH) or
/// bullish regular for short (price LL, momentum HL). Looks at the
/// two most recent confirmed pivots inside `lookback_bars`.
fn detect_divergence(ctx: &WarningContext, cfg: &WarningConfig) -> bool {
    if ctx.closes.len() != ctx.momentum.len() {
        return false;
    }
    let n = ctx.closes.len();
    if n < cfg.divergence_pivot_left + cfg.divergence_pivot_right + 3 {
        return false;
    }
    let window_start = n.saturating_sub(cfg.divergence_lookback_bars);
    let is_high_side = matches!(ctx.direction, Direction::Long);

    // Extract the two most-recent confirmed pivots of the relevant
    // side inside the lookback window.
    let pivots = extract_pivots(
        if is_high_side { ctx.highs } else { ctx.lows },
        window_start,
        cfg.divergence_pivot_left,
        cfg.divergence_pivot_right,
        is_high_side,
    );
    if pivots.len() < 2 {
        return false;
    }
    // Newest-first → take last two and order oldest → newest.
    let len = pivots.len();
    let (idx_a, price_a) = pivots[len - 2];
    let (idx_b, price_b) = pivots[len - 1];
    let mom_a = ctx.momentum[idx_a];
    let mom_b = ctx.momentum[idx_b];
    if !mom_a.is_finite() || !mom_b.is_finite() {
        return false;
    }
    match ctx.direction {
        // price HH + momentum LH → bearish regular divergence
        Direction::Long => price_b > price_a && mom_b < mom_a,
        // price LL + momentum HL → bullish regular divergence
        Direction::Short => price_b < price_a && mom_b > mom_a,
        Direction::Neutral => false,
    }
}

/// Climactic-volume bar against trade direction in the last
/// `volume_recent_bars`. "Against" = close went the opposite way
/// from a long/short. Average baseline excludes the recent window.
fn detect_volume_reversal(ctx: &WarningContext, cfg: &WarningConfig) -> bool {
    let n = ctx.volumes.len();
    if n < cfg.volume_lookback_bars + cfg.volume_recent_bars + 1 {
        return false;
    }
    if ctx.closes.len() != n {
        return false;
    }
    let recent_start = n - cfg.volume_recent_bars;
    let base_start = recent_start.saturating_sub(cfg.volume_lookback_bars);
    let base_end = recent_start;
    if base_end <= base_start {
        return false;
    }
    let base_sum: f64 = ctx.volumes[base_start..base_end].iter().sum();
    let base_avg = base_sum / (base_end - base_start) as f64;
    if base_avg <= 0.0 {
        return false;
    }
    let threshold = base_avg * cfg.volume_spike_mult;
    for i in recent_start..n {
        if ctx.volumes[i] < threshold {
            continue;
        }
        // bar direction vs trade direction
        let prev_close = if i > 0 { ctx.closes[i - 1] } else { ctx.closes[i] };
        let adverse = match ctx.direction {
            Direction::Long => ctx.closes[i] < prev_close,
            Direction::Short => ctx.closes[i] > prev_close,
            Direction::Neutral => false,
        };
        if adverse {
            return true;
        }
    }
    false
}

/// Micro-structure break — latest close pierced the last confirmed
/// pivot on trade-side (pivot_low for long, pivot_high for short).
fn detect_micro_break(ctx: &WarningContext, cfg: &WarningConfig) -> bool {
    let n = ctx.closes.len();
    if n == 0 || ctx.highs.len() != n || ctx.lows.len() != n {
        return false;
    }
    let latest_close = ctx.closes[n - 1];
    match ctx.direction {
        Direction::Long => {
            if let Some(piv) = most_recent_pivot_low(
                ctx.lows,
                cfg.micro_pivot_left,
                cfg.micro_pivot_right,
            ) {
                latest_close < piv
            } else {
                false
            }
        }
        Direction::Short => {
            if let Some(piv) = most_recent_pivot_high(
                ctx.highs,
                cfg.micro_pivot_left,
                cfg.micro_pivot_right,
            ) {
                latest_close > piv
            } else {
                false
            }
        }
        Direction::Neutral => false,
    }
}

/// Reversal candlestick on the latest closed bar.
/// Long → bearish engulfing; Short → bullish engulfing.
fn detect_candle_reversal(ctx: &WarningContext) -> bool {
    let n = ctx.closes.len();
    if n < 2
        || ctx.opens.len() != n
        || ctx.highs.len() != n
        || ctx.lows.len() != n
    {
        return false;
    }
    let (o_prev, c_prev) = (ctx.opens[n - 2], ctx.closes[n - 2]);
    let (o_cur, c_cur) = (ctx.opens[n - 1], ctx.closes[n - 1]);
    match ctx.direction {
        // Bearish engulfing: prev green, curr red, curr body engulfs prev body.
        Direction::Long => {
            let prev_green = c_prev > o_prev;
            let cur_red = c_cur < o_cur;
            let engulfs = o_cur >= c_prev && c_cur <= o_prev;
            prev_green && cur_red && engulfs
        }
        // Bullish engulfing: prev red, curr green, curr body engulfs prev body.
        Direction::Short => {
            let prev_red = c_prev < o_prev;
            let cur_green = c_cur > o_cur;
            let engulfs = o_cur <= c_prev && c_cur >= o_prev;
            prev_red && cur_green && engulfs
        }
        Direction::Neutral => false,
    }
}

/// Regime flip — latest confluence direction is opposite of the
/// setup direction, AND its guven is strong enough to take seriously.
fn detect_regime_flip(ctx: &WarningContext, cfg: &WarningConfig) -> bool {
    if ctx.regime_guven < cfg.regime_guven_threshold {
        return false;
    }
    matches!(
        (ctx.direction, ctx.regime_direction),
        (Direction::Long, Direction::Short) | (Direction::Short, Direction::Long)
    )
}

// ------------------------------------------------------------------
// Pivot helpers (scoped to this module).
// ------------------------------------------------------------------

/// Extract all confirmed pivots (highs if `is_high`, else lows) from
/// `series[start..]`, ordered oldest → newest. Indices are returned
/// in original series coordinates.
fn extract_pivots(
    series: &[f64],
    start: usize,
    left: usize,
    right: usize,
    is_high: bool,
) -> Vec<(usize, f64)> {
    let n = series.len();
    let mut out = Vec::new();
    if n < left + right + 1 {
        return out;
    }
    let first = start.max(left);
    let last = n.saturating_sub(right + 1);
    for i in first..=last {
        let v = series[i];
        let left_ok = if is_high {
            (i - left..i).all(|j| series[j] <= v)
        } else {
            (i - left..i).all(|j| series[j] >= v)
        };
        if !left_ok {
            continue;
        }
        let right_ok = if is_high {
            (i + 1..=i + right).all(|j| series[j] <= v)
        } else {
            (i + 1..=i + right).all(|j| series[j] >= v)
        };
        if right_ok {
            out.push((i, v));
        }
    }
    out
}

fn most_recent_pivot_low(lows: &[f64], left: usize, right: usize) -> Option<f64> {
    let piv = extract_pivots(lows, 0, left, right, false);
    piv.last().map(|(_, v)| *v)
}

fn most_recent_pivot_high(highs: &[f64], left: usize, right: usize) -> Option<f64> {
    let piv = extract_pivots(highs, 0, left, right, true);
    piv.last().map(|(_, v)| *v)
}

// ------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn base_cfg() -> WarningConfig {
        WarningConfig {
            warn_quorum: 2,
            exit_quorum: 3,
            divergence_enabled: true,
            divergence_lookback_bars: 50,
            divergence_pivot_left: 2,
            divergence_pivot_right: 2,
            volume_enabled: true,
            volume_spike_mult: 1.8,
            volume_lookback_bars: 10,
            volume_recent_bars: 3,
            micro_enabled: true,
            micro_pivot_left: 2,
            micro_pivot_right: 2,
            candle_enabled: true,
            regime_enabled: true,
            regime_guven_threshold: 0.5,
        }
    }

    /// Minimal neutral context — overridden by per-test fields.
    fn empty_ctx<'a>() -> WarningContext<'a> {
        WarningContext {
            direction: Direction::Long,
            opens: &[],
            highs: &[],
            lows: &[],
            closes: &[],
            volumes: &[],
            momentum: &[],
            regime_direction: Direction::Neutral,
            regime_guven: 0.0,
        }
    }

    #[test]
    fn verdict_none_below_warn_quorum() {
        let v = evaluate_warnings(&empty_ctx(), &base_cfg());
        assert_eq!(v.action, WarningAction::None);
        assert!(v.triggered.is_empty());
    }

    #[test]
    fn regime_flip_fires_when_opposite_and_high_guven() {
        let mut ctx = empty_ctx();
        ctx.direction = Direction::Long;
        ctx.regime_direction = Direction::Short;
        ctx.regime_guven = 0.7;
        let cfg = WarningConfig {
            divergence_enabled: false,
            volume_enabled: false,
            micro_enabled: false,
            candle_enabled: false,
            ..base_cfg()
        };
        let v = evaluate_warnings(&ctx, &cfg);
        assert_eq!(v.triggered, vec![WarningSignal::RegimeFlip]);
        assert_eq!(v.action, WarningAction::None); // 1 < warn
    }

    #[test]
    fn regime_flip_gated_by_guven_threshold() {
        let mut ctx = empty_ctx();
        ctx.direction = Direction::Long;
        ctx.regime_direction = Direction::Short;
        ctx.regime_guven = 0.3; // below default 0.5
        let v = evaluate_warnings(&ctx, &base_cfg());
        assert!(v.triggered.is_empty());
    }

    #[test]
    fn candle_bearish_engulfing_on_long() {
        let mut ctx = empty_ctx();
        ctx.direction = Direction::Long;
        // prev green (100→102), curr red that engulfs: opens above prev close,
        // closes below prev open.
        let opens = &[100.0, 102.5];
        let closes = &[102.0, 99.0];
        let highs = &[103.0, 103.0];
        let lows = &[99.0, 98.0];
        ctx.opens = opens;
        ctx.closes = closes;
        ctx.highs = highs;
        ctx.lows = lows;
        assert!(detect_candle_reversal(&ctx));
    }

    #[test]
    fn candle_no_false_positive_on_normal_pullback() {
        let mut ctx = empty_ctx();
        ctx.direction = Direction::Long;
        // prev green, curr red but body not engulfed.
        let opens = &[100.0, 101.5];
        let closes = &[102.0, 101.0];
        let highs = &[103.0, 102.0];
        let lows = &[99.0, 100.5];
        ctx.opens = opens;
        ctx.closes = closes;
        ctx.highs = highs;
        ctx.lows = lows;
        assert!(!detect_candle_reversal(&ctx));
    }

    #[test]
    fn volume_spike_against_long_fires() {
        let mut ctx = empty_ctx();
        ctx.direction = Direction::Long;
        // 15 bars: first 12 baseline, last 3 with one red bar on 3× volume.
        let closes: Vec<f64> = (0..15).map(|i| 100.0 + i as f64 * 0.1).collect();
        let mut closes = closes;
        closes[14] = closes[13] - 1.0;           // red bar
        let mut vols = vec![1.0; 15];
        vols[14] = 5.0;                          // climactic
        ctx.closes = &closes;
        ctx.volumes = &vols;
        let cfg = base_cfg();
        assert!(detect_volume_reversal(&ctx, &cfg));
    }

    #[test]
    fn volume_spike_with_green_bar_no_fire_for_long() {
        let mut ctx = empty_ctx();
        ctx.direction = Direction::Long;
        let closes: Vec<f64> = (0..15).map(|i| 100.0 + i as f64 * 0.1).collect();
        let mut vols = vec![1.0; 15];
        vols[14] = 5.0;
        ctx.closes = &closes;
        ctx.volumes = &vols;
        assert!(!detect_volume_reversal(&ctx, &base_cfg()));
    }

    #[test]
    fn micro_break_long_when_close_below_last_pivot_low() {
        let mut ctx = empty_ctx();
        ctx.direction = Direction::Long;
        // Construct a series with a confirmed pivot low at idx 2 value 95.
        // lows = [97, 96, 95, 96, 97, 96], left=2,right=2 → pivot at idx 2.
        let lows = vec![97.0, 96.0, 95.0, 96.0, 97.0, 96.0, 98.0];
        let highs = vec![99.0; 7];
        // Latest close = 94 → below pivot 95 → break.
        let closes = vec![98.0, 97.0, 96.0, 97.0, 98.0, 97.0, 94.0];
        ctx.lows = &lows;
        ctx.highs = &highs;
        ctx.closes = &closes;
        let cfg = base_cfg();
        assert!(detect_micro_break(&ctx, &cfg));
    }

    #[test]
    fn micro_break_long_no_fire_when_close_holds() {
        let mut ctx = empty_ctx();
        ctx.direction = Direction::Long;
        let lows = vec![97.0, 96.0, 95.0, 96.0, 97.0, 96.0, 98.0];
        let highs = vec![99.0; 7];
        let closes = vec![98.0, 97.0, 96.0, 97.0, 98.0, 97.0, 96.0]; // close 96 > pivot 95
        ctx.lows = &lows;
        ctx.highs = &highs;
        ctx.closes = &closes;
        assert!(!detect_micro_break(&ctx, &base_cfg()));
    }

    #[test]
    fn divergence_long_bearish_regular() {
        let mut ctx = empty_ctx();
        ctx.direction = Direction::Long;
        // Two confirmed pivot highs with HH but momentum LH.
        //   idx 2 → high=105, momentum=80
        //   idx 6 → high=108, momentum=70
        let highs = vec![100.0, 102.0, 105.0, 103.0, 104.0, 106.0, 108.0, 105.0, 103.0];
        let lows = vec![98.0; 9];
        let closes = vec![101.0; 9];
        let momentum = vec![
            60.0, 70.0, 80.0, 75.0, 65.0, 70.0, 70.0, 60.0, 55.0,
        ];
        ctx.highs = &highs;
        ctx.lows = &lows;
        ctx.closes = &closes;
        ctx.momentum = &momentum;
        let cfg = base_cfg();
        assert!(detect_divergence(&ctx, &cfg));
    }

    #[test]
    fn divergence_no_fire_when_momentum_confirms() {
        let mut ctx = empty_ctx();
        ctx.direction = Direction::Long;
        let highs = vec![100.0, 102.0, 105.0, 103.0, 104.0, 106.0, 108.0, 105.0, 103.0];
        let lows = vec![98.0; 9];
        let closes = vec![101.0; 9];
        let momentum = vec![
            60.0, 70.0, 80.0, 75.0, 75.0, 80.0, 85.0, 75.0, 70.0, // HH momentum too
        ];
        ctx.highs = &highs;
        ctx.lows = &lows;
        ctx.closes = &closes;
        ctx.momentum = &momentum;
        assert!(!detect_divergence(&ctx, &base_cfg()));
    }

    #[test]
    fn exit_quorum_on_three_signals() {
        // Force regime flip + candle reversal + micro break = 3 signals.
        let mut ctx = empty_ctx();
        ctx.direction = Direction::Long;
        ctx.regime_direction = Direction::Short;
        ctx.regime_guven = 0.8;

        // lows = confirmed pivot_low at idx 2 (=95); latest close 94 breaks it.
        let lows = vec![97.0, 96.0, 95.0, 96.0, 97.0, 96.0, 98.0];
        let highs = vec![103.0; 7];
        // Engulfing pair at idx 5 (prev green) / idx 6 (curr red engulfing).
        let opens = vec![98.0, 97.0, 96.0, 97.0, 98.0, 100.0, 102.5];
        let closes = vec![98.5, 97.5, 96.5, 97.5, 98.5, 102.0, 94.0];
        ctx.opens = &opens;
        ctx.closes = &closes;
        ctx.highs = &highs;
        ctx.lows = &lows;

        let cfg = WarningConfig {
            volume_enabled: false,
            divergence_enabled: false,
            ..base_cfg()
        };
        let v = evaluate_warnings(&ctx, &cfg);
        assert!(
            v.triggered.len() >= 3,
            "expected at least 3 signals, got {:?}",
            v.triggered
        );
        assert_eq!(v.action, WarningAction::Exit);
    }
}

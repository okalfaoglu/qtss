//! Wyckoff event catalog — full Phase A–E implementation.
//!
//! Each entry is an [`EventSpec`]: a name and an `eval` function pointer
//! that inspects the trailing pivots + computed [`TradingRange`] and
//! returns a [`EventMatch`] when it fires. Adding a new event is one
//! slice entry, no central match arm to edit (CLAUDE.md rule #1).

use crate::config::WyckoffConfig;
use crate::range::{average_volume, TradingRange};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::pivot::{Pivot, PivotKind};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub struct EventMatch {
    pub score: f64,
    pub invalidation: Decimal,
    pub variant: &'static str,
    /// Anchor labels for the trailing pivots (oldest..newest).
    pub anchor_labels: Vec<&'static str>,
}

/// Event evaluation context — P1-refactor.
///
/// Legacy pivot-only detectors keep using [`EventEval::Pivots`] which
/// receives `(&[Pivot], &WyckoffConfig)`. New / PDF-faithful detectors
/// (SOS/SOW bar-shape, Markup/Markdown, JAC body-ratio …) need bar-level
/// OHLC access; those register as [`EventEval::WithBars`] and receive
/// this context.
///
/// Bar lookup by pivot uses `open_time` equality — independent of
/// whether `pivot.bar_index` is window-relative or absolute, so the
/// refactor never cares about indexing conventions.
pub struct EventContext<'a> {
    pub pivots: &'a [Pivot],
    pub bars: &'a [Bar],
    pub cfg: &'a WyckoffConfig,
}

impl<'a> EventContext<'a> {
    pub fn new(pivots: &'a [Pivot], bars: &'a [Bar], cfg: &'a WyckoffConfig) -> Self {
        Self { pivots, bars, cfg }
    }

    /// Bar whose open_time matches the pivot's timestamp, if the
    /// orchestrator-provided bar window covers that time.
    pub fn bar_for_pivot(&self, pivot: &Pivot) -> Option<&Bar> {
        self.bars.iter().find(|b| b.open_time == pivot.time)
    }

    /// Rolling mean of (high-low) over the trailing `window` bars as an
    /// ATR proxy. Returns None if the window is too short.
    pub fn atr_proxy(&self, window: usize) -> Option<f64> {
        if self.bars.len() < window || window == 0 {
            return None;
        }
        let tail = &self.bars[self.bars.len() - window..];
        let mut sum = 0.0;
        for b in tail {
            sum += (b.high - b.low).to_f64().unwrap_or(0.0);
        }
        Some(sum / window as f64)
    }

    /// Rolling mean of bar volume over the trailing `window` bars.
    pub fn avg_bar_volume(&self, window: usize) -> Option<f64> {
        if self.bars.len() < window || window == 0 {
            return None;
        }
        let tail = &self.bars[self.bars.len() - window..];
        let mut sum = 0.0;
        for b in tail {
            sum += b.volume.to_f64().unwrap_or(0.0);
        }
        Some(sum / window as f64)
    }
}

/// Dual-shape event evaluator. Legacy detectors operate on pivots only;
/// bar-aware detectors (introduced in the P1 refactor) take a full
/// [`EventContext`]. Dispatch lives in `detector.rs`.
pub enum EventEval {
    Pivots(fn(&[Pivot], &WyckoffConfig) -> Option<EventMatch>),
    WithBars(fn(&EventContext) -> Option<EventMatch>),
}

pub struct EventSpec {
    pub name: &'static str,
    pub eval: EventEval,
}

pub const EVENTS: &[EventSpec] = &[
    EventSpec { name: "trading_range", eval: EventEval::Pivots(eval_trading_range) },
    EventSpec { name: "spring",        eval: EventEval::Pivots(eval_spring) },
    EventSpec { name: "upthrust",      eval: EventEval::Pivots(eval_upthrust) },
    // Phase A
    EventSpec { name: "selling_climax",     eval: EventEval::Pivots(eval_selling_climax) },
    EventSpec { name: "buying_climax",      eval: EventEval::Pivots(eval_buying_climax) },
    EventSpec { name: "automatic_rally",    eval: EventEval::Pivots(eval_automatic_rally) },
    EventSpec { name: "automatic_reaction", eval: EventEval::Pivots(eval_automatic_reaction) },
    EventSpec { name: "secondary_test",     eval: EventEval::Pivots(eval_secondary_test) },
    // Phase B
    EventSpec { name: "upthrust_action",    eval: EventEval::Pivots(eval_upthrust_action) },
    // Phase C
    EventSpec { name: "shakeout",           eval: EventEval::Pivots(eval_shakeout) },
    // Phase D
    EventSpec { name: "sign_of_strength",      eval: EventEval::Pivots(eval_sign_of_strength) },
    EventSpec { name: "sign_of_weakness",      eval: EventEval::Pivots(eval_sign_of_weakness) },
    EventSpec { name: "last_point_of_support", eval: EventEval::Pivots(eval_last_point_of_support) },
    EventSpec { name: "last_point_of_supply",  eval: EventEval::Pivots(eval_last_point_of_supply) },
    EventSpec { name: "jump_across_creek",     eval: EventEval::Pivots(eval_jump_across_creek) },
    EventSpec { name: "break_of_ice",          eval: EventEval::Pivots(eval_break_of_ice) },
    // SOT
    EventSpec { name: "shortening_of_thrust",  eval: EventEval::Pivots(eval_shortening_of_thrust) },
    // P13 additions — completes the 16-event vocabulary.
    EventSpec { name: "preliminary_supply",    eval: EventEval::Pivots(eval_preliminary_supply) },
    EventSpec { name: "secondary_test_b",      eval: EventEval::Pivots(eval_secondary_test_b) },
    EventSpec { name: "back_up_edge_creek",    eval: EventEval::Pivots(eval_back_up_edge_creek) },
];

// =========================================================================
// Helpers
// =========================================================================

fn label_for(idx: usize) -> &'static str {
    const LABELS: &[&str] = &[
        "P1", "P2", "P3", "P4", "P5", "P6", "P7", "P8", "P9", "P10", "P11", "P12",
    ];
    LABELS.get(idx).copied().unwrap_or("Pn")
}

/// Find the pivot with the highest volume in a slice.
#[allow(dead_code)]
fn highest_volume_pivot(pivots: &[Pivot]) -> Option<(usize, f64)> {
    let mut best: Option<(usize, f64)> = None;
    for (i, p) in pivots.iter().enumerate() {
        let v = p.volume_at_pivot.to_f64()?;
        if best.map(|(_, bv)| v > bv).unwrap_or(true) {
            best = Some((i, v));
        }
    }
    best
}

/// Average volume as f64.
fn avg_vol_f64(pivots: &[Pivot]) -> Option<f64> {
    average_volume(pivots)?.to_f64()
}

/// Late-half / early-half mean-volume ratio. Returns None if the
/// window is too small to split meaningfully. < 1.0 means volume is
/// drying up (canonical Wyckoff behaviour); > 1.0 means expanding
/// volume (trending market). Used by `eval_trading_range` as a
/// range-quality filter.
fn pivot_volume_expansion(pivots: &[Pivot]) -> Option<f64> {
    if pivots.len() < 4 { return None; }
    let mid = pivots.len() / 2;
    let mean = |slice: &[Pivot]| -> Option<f64> {
        if slice.is_empty() { return None; }
        let sum: f64 = slice.iter()
            .filter_map(|p| p.volume_at_pivot.to_f64())
            .sum();
        Some(sum / slice.len() as f64)
    };
    let early = mean(&pivots[..mid])?;
    let late  = mean(&pivots[mid..])?;
    if early <= 1e-9 { return None; }
    Some(late / early)
}

/// Bar range (high - low) approximated from pivot price and nearby pivots.
fn pivot_bar_range(pivot: &Pivot, pivots: &[Pivot]) -> f64 {
    // Approximation: use the price difference between this pivot and the
    // nearest opposite-kind pivot as a proxy for bar range.
    let price = pivot.price.to_f64().unwrap_or(0.0);
    let mut nearest = f64::MAX;
    for p in pivots {
        if p.kind != pivot.kind {
            let d = (p.price.to_f64().unwrap_or(0.0) - price).abs();
            if d < nearest && d > 0.0 {
                nearest = d;
            }
        }
    }
    if nearest == f64::MAX { 0.0 } else { nearest }
}

fn creek_level(range: &TradingRange, percentile: f64) -> f64 {
    range.support + range.height * percentile
}

/// True if `context` already shows a real range around `range.support`:
/// at least `manipulation_min_edge_tests` prior low pivots sit within the
/// edge-tolerance band of support, AND the earliest such test is at
/// least `manipulation_min_range_age_bars` bars before `candidate_bar`.
///
/// This is the Wyckoff "established range" pre-condition for a Spring.
/// Without it, any trend pullback whose low dips below a trimmed body
/// support would be flagged Spring — noise, not Wyckoff.
fn range_is_established_at_support(
    context: &[Pivot],
    range: &TradingRange,
    cfg: &WyckoffConfig,
    candidate_bar: u64,
) -> bool {
    let tol = range.height.max(1e-9) * cfg.range_edge_tolerance;
    let mut tests: Vec<u64> = context
        .iter()
        .filter(|p| p.kind == PivotKind::Low)
        .filter_map(|p| p.price.to_f64().map(|v| (p.bar_index, v)))
        .filter(|(_, v)| (*v - range.support).abs() <= tol)
        .map(|(b, _)| b)
        .collect();
    if tests.len() < cfg.manipulation_min_edge_tests {
        return false;
    }
    tests.sort_unstable();
    let age = candidate_bar.saturating_sub(tests[0]);
    age >= cfg.manipulation_min_range_age_bars
}

/// Mirror of `range_is_established_at_support` for the resistance edge
/// (UTAD precondition).
fn range_is_established_at_resistance(
    context: &[Pivot],
    range: &TradingRange,
    cfg: &WyckoffConfig,
    candidate_bar: u64,
) -> bool {
    let tol = range.height.max(1e-9) * cfg.range_edge_tolerance;
    let mut tests: Vec<u64> = context
        .iter()
        .filter(|p| p.kind == PivotKind::High)
        .filter_map(|p| p.price.to_f64().map(|v| (p.bar_index, v)))
        .filter(|(_, v)| (*v - range.resistance).abs() <= tol)
        .map(|(b, _)| b)
        .collect();
    if tests.len() < cfg.manipulation_min_edge_tests {
        return false;
    }
    tests.sort_unstable();
    let age = candidate_bar.saturating_sub(tests[0]);
    age >= cfg.manipulation_min_range_age_bars
}

fn ice_level(range: &TradingRange, percentile: f64) -> f64 {
    range.support + range.height * (1.0 - percentile)
}

/// P20 — slope of same-kind pivot prices as fraction of mean price per
/// pivot step. Positive for rising series, negative for falling. Used
/// by Spring/UTAD as a trend filter: a Wyckoff manipulation pierces a
/// HORIZONTAL edge. If the underlying pivot series is itself trending,
/// each "pierce" is just a trend pullback, not a false break.
///
/// Linear regression on (index, price) pairs, slope normalized by mean
/// price to produce a unitless fraction.
fn same_kind_slope_frac(pivots: &[Pivot], kind: PivotKind) -> Option<f64> {
    let prices: Vec<f64> = pivots
        .iter()
        .filter(|p| p.kind == kind)
        .filter_map(|p| p.price.to_f64())
        .collect();
    let n = prices.len();
    if n < 3 { return None; }
    let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let mean_x = xs.iter().sum::<f64>() / n as f64;
    let mean_y = prices.iter().sum::<f64>() / n as f64;
    if mean_y.abs() < 1e-9 { return None; }
    let mut num = 0.0;
    let mut den = 0.0;
    for i in 0..n {
        let dx = xs[i] - mean_x;
        num += dx * (prices[i] - mean_y);
        den += dx * dx;
    }
    if den < 1e-9 { return None; }
    Some((num / den) / mean_y)
}

// =========================================================================
// Trading range (existing)
// =========================================================================

fn eval_trading_range(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < cfg.min_range_pivots {
        return None;
    }
    let range = TradingRange::from_pivots(pivots)?;
    let tightness = range.edge_tightness(pivots, cfg.range_edge_tolerance)?;
    if tightness < 0.4 {
        return None;
    }
    // TF guard #1: reject ranges whose height / midpoint ratio exceeds
    // `max_range_height_pct`. An H1 detector with a 0.08 cap will refuse
    // to surface a 15%-wide range as a Wyckoff range — that geometry
    // belongs to a higher TF.
    let mid = (range.resistance + range.support) * 0.5;
    if mid > 0.0 && (range.height / mid) > cfg.max_range_height_pct {
        return None;
    }
    // TF guard #2: reject ranges whose pivot span exceeds `max_range_
    // age_bars`. A very old range on a fast TF is stale data, not a
    // valid active Wyckoff structure.
    if let (Some(first), Some(last)) = (pivots.first(), pivots.last()) {
        if last.bar_index.saturating_sub(first.bar_index) > cfg.max_range_age_bars {
            return None;
        }
    }
    // Volume-contraction guard (Wyckoff "drying up"). Compare mean
    // volume of the latter half of the pivot window against the
    // earlier half: a canonical range shows supply exhaustion, so
    // late/early < 1.0 is ideal. Values above `max_range_volume_
    // expansion` indicate a trending market with rising participation,
    // not accumulation/distribution — reject.
    if let Some(ratio) = pivot_volume_expansion(pivots) {
        if ratio > cfg.max_range_volume_expansion {
            return None;
        }
    }
    let variant = climactic_variant(pivots, &range, cfg).unwrap_or("neutral");
    let labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
    Some(EventMatch {
        score: tightness,
        invalidation: Decimal::try_from(range.support).ok().unwrap_or(Decimal::ZERO),
        variant,
        anchor_labels: labels,
    })
}

/// Classify a trading range as accumulation vs distribution based on a
/// climactic volume pivot pinned to the correct edge.
///
/// Wyckoff rule:
///   * Accumulation starts with SC (Selling Climax) — a Low pivot on
///     high volume **at or below the range support**.
///   * Distribution starts with BC (Buying Climax) — a High pivot on
///     high volume **at or above the range resistance**.
///
/// The earlier version accepted any volume-spike pivot whose price sat
/// "closer to its matching edge than the opposite edge" — which in an
/// uptrend wrongly classified a mid-range pullback high as a BC and
/// tagged the range as distribution. We now require the climax pivot
/// to be within `range_edge_tolerance` of the actual edge price.
fn climactic_variant(
    pivots: &[Pivot],
    range: &TradingRange,
    cfg: &WyckoffConfig,
) -> Option<&'static str> {
    let avg = avg_vol_f64(pivots)?;
    if avg <= 0.0 {
        return None;
    }
    let threshold = avg * cfg.climax_volume_mult;
    let mut best: Option<(&Pivot, f64)> = None;
    for p in pivots {
        let v = p.volume_at_pivot.to_f64()?;
        if v >= threshold && best.map(|(_, bv)| v > bv).unwrap_or(true) {
            best = Some((p, v));
        }
    }
    let (climax, _) = best?;
    let price = climax.price.to_f64()?;
    let edge_tol = range.height.max(1e-9) * cfg.range_edge_tolerance;
    match climax.kind {
        // SC must sit AT or BELOW support (within edge tolerance).
        PivotKind::Low if price <= range.support + edge_tol => Some("accumulation"),
        // BC must sit AT or ABOVE resistance (within edge tolerance).
        PivotKind::High if price >= range.resistance - edge_tol => Some("distribution"),
        _ => None,
    }
}

// =========================================================================
// Spring (Phase C — bullish false-break)
// =========================================================================

fn eval_spring(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < cfg.min_range_pivots + 1 {
        return None;
    }
    let context = &pivots[..pivots.len() - 1];
    let candidate = pivots.last()?;
    if candidate.kind != PivotKind::Low {
        return None;
    }
    let range = TradingRange::from_pivots(context)?;
    let price = candidate.price.to_f64()?;
    if price >= range.support {
        return None;
    }
    let penetration = (range.support - price) / range.height.max(1e-9);
    if penetration < cfg.min_penetration || penetration > cfg.max_penetration {
        return None;
    }

    // Wyckoff rule: a Spring pierces an ESTABLISHED range. An isolated
    // pivot below some trimmed body is just a trend pullback, not a
    // Spring. Require that the body support was actually tested by
    // multiple prior lows AND that the range has existed for a
    // meaningful time before the candidate. Without these guards the
    // detector fires on every pullback in a trending market.
    if !range_is_established_at_support(context, &range, cfg, candidate.bar_index) {
        return None;
    }
    // P20 — trend filter. A Spring requires a HORIZONTAL support; if
    // the low-pivot series is rising, every pullback's low is below
    // the previous "trimmed body support" but that's trend, not a
    // Wyckoff false break. Reject when slope exceeds the config cap.
    if let Some(slope) = same_kind_slope_frac(context, PivotKind::Low) {
        if slope.abs() > cfg.manipulation_max_edge_slope {
            return None;
        }
    }
    // Pruden Spring variant classification by candidate-bar volume vs
    // average. #1 Terminal (very high vol) = weakest edge, optionally
    // skipped. #3 No-Supply (low vol) = strongest — rewarded with a
    // score bonus. #2 Ordinary = baseline.
    let variant = classify_spring_variant(candidate, pivots, cfg);
    if variant == SpringVariant::Terminal && cfg.skip_terminal_springs {
        return None;
    }
    let center = (cfg.min_penetration + cfg.max_penetration) / 2.0;
    let half = (cfg.max_penetration - cfg.min_penetration) / 2.0;
    let z = (penetration - center) / half.max(1e-9);
    let base = (-(z * z) / 2.0).exp();
    let score = base * variant.score_multiplier();
    let labels: Vec<&'static str> = (0..context.len())
        .map(label_for)
        .chain(std::iter::once("Spring"))
        .collect();
    Some(EventMatch {
        score,
        invalidation: candidate.price,
        variant: variant.as_str(),
        anchor_labels: labels,
    })
}

/// Pruden's three Spring variants, classified by the Spring bar's
/// volume vs the pivot-window average. See `WyckoffConfig::spring_*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpringVariant { Terminal, Ordinary, NoSupply }

impl SpringVariant {
    fn as_str(self) -> &'static str {
        // Single-token variants so the orchestrator's subkind parser
        // (rsplit on '_' to separate event_name / variant) keeps
        // working: "spring_bull" / "spring_nosupply" / "spring_terminal".
        match self {
            Self::Terminal  => "terminal",
            Self::Ordinary  => "bull",
            Self::NoSupply  => "nosupply",
        }
    }
    /// Score bonus/penalty relative to an Ordinary Spring.
    fn score_multiplier(self) -> f64 {
        match self {
            Self::Terminal  => 0.7,   // still fires (if not skipped) but discounted
            Self::Ordinary  => 1.0,
            Self::NoSupply  => 1.25,  // highest-probability variant
        }
    }
}

fn classify_spring_variant(
    candidate: &Pivot,
    pivots: &[Pivot],
    cfg: &WyckoffConfig,
) -> SpringVariant {
    let avg = avg_vol_f64(pivots).unwrap_or(0.0).max(1e-9);
    let v = candidate.volume_at_pivot.to_f64().unwrap_or(0.0);
    let ratio = v / avg;
    match ratio {
        r if r >= cfg.spring_terminal_vol_ratio   => SpringVariant::Terminal,
        r if r <= cfg.spring_no_supply_vol_ratio  => SpringVariant::NoSupply,
        _                                          => SpringVariant::Ordinary,
    }
}

// =========================================================================
// Upthrust (Phase C — bearish false-break)
// =========================================================================

fn eval_upthrust(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < cfg.min_range_pivots + 1 {
        return None;
    }
    let context = &pivots[..pivots.len() - 1];
    let candidate = pivots.last()?;
    if candidate.kind != PivotKind::High {
        return None;
    }
    let range = TradingRange::from_pivots(context)?;
    let price = candidate.price.to_f64()?;
    if price <= range.resistance {
        return None;
    }
    let penetration = (price - range.resistance) / range.height.max(1e-9);
    if penetration < cfg.min_penetration || penetration > cfg.max_penetration {
        return None;
    }
    // Established-range gate, mirror of Spring. Without this any pullback
    // high above a trimmed body is flagged UTAD in trending markets.
    if !range_is_established_at_resistance(context, &range, cfg, candidate.bar_index) {
        return None;
    }
    // P20 — trend filter mirror of Spring guard. A UTAD requires a
    // HORIZONTAL resistance; falling high-pivot series means downtrend,
    // each candidate is a bounce not a false break. Reject.
    if let Some(slope) = same_kind_slope_frac(context, PivotKind::High) {
        if slope.abs() > cfg.manipulation_max_edge_slope {
            return None;
        }
    }
    let center = (cfg.min_penetration + cfg.max_penetration) / 2.0;
    let half = (cfg.max_penetration - cfg.min_penetration) / 2.0;
    let z = (penetration - center) / half.max(1e-9);
    let score = (-(z * z) / 2.0).exp();
    let labels: Vec<&'static str> = (0..context.len())
        .map(label_for)
        .chain(std::iter::once("UTAD"))
        .collect();
    Some(EventMatch {
        score,
        invalidation: candidate.price,
        variant: "bear",
        anchor_labels: labels,
    })
}

// =========================================================================
// Phase A: Selling Climax (SC)
// =========================================================================
// Panic sell-off: highest volume in the window + widest bar + price near
// the support zone. Signals the beginning of accumulation.

fn eval_selling_climax(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 4 {
        return None;
    }
    let avg = avg_vol_f64(pivots)?;
    if avg <= 0.0 { return None; }

    // Find the low pivot with the highest volume
    let mut best: Option<(usize, f64)> = None;
    for (i, p) in pivots.iter().enumerate() {
        if p.kind != PivotKind::Low { continue; }
        let v = p.volume_at_pivot.to_f64()?;
        if v >= avg * cfg.sc_volume_multiplier {
            if best.map(|(_, bv)| v > bv).unwrap_or(true) {
                best = Some((i, v));
            }
        }
    }
    let (idx, vol) = best?;
    let pivot = &pivots[idx];

    // Bar width check (approximate via price swing to nearest opposite pivot)
    let bar_range = pivot_bar_range(pivot, pivots);
    let range = TradingRange::from_pivots(pivots)?;
    let atr_proxy = range.height / (pivots.len() as f64).max(1.0);
    if bar_range < atr_proxy * cfg.sc_bar_width_multiplier {
        // Bar not wide enough — might not be true SC
    }

    let vol_score = (vol / (avg * cfg.sc_volume_multiplier)).min(2.0) / 2.0;
    let score = 0.5 + vol_score * 0.5;

    let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
    if idx < labels.len() {
        labels[idx] = "SC";
    }
    Some(EventMatch {
        score,
        invalidation: pivot.price,
        variant: "accumulation",
        anchor_labels: labels,
    })
}

// =========================================================================
// Phase A: Buying Climax (BC)
// =========================================================================

fn eval_buying_climax(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 4 {
        return None;
    }
    let avg = avg_vol_f64(pivots)?;
    if avg <= 0.0 { return None; }

    let mut best: Option<(usize, f64)> = None;
    for (i, p) in pivots.iter().enumerate() {
        if p.kind != PivotKind::High { continue; }
        let v = p.volume_at_pivot.to_f64()?;
        if v >= avg * cfg.sc_volume_multiplier {
            if best.map(|(_, bv)| v > bv).unwrap_or(true) {
                best = Some((i, v));
            }
        }
    }
    let (idx, vol) = best?;
    let pivot = &pivots[idx];

    let vol_score = (vol / (avg * cfg.sc_volume_multiplier)).min(2.0) / 2.0;
    let score = 0.5 + vol_score * 0.5;

    let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
    if idx < labels.len() {
        labels[idx] = "BC";
    }
    Some(EventMatch {
        score,
        invalidation: pivot.price,
        variant: "distribution",
        anchor_labels: labels,
    })
}

// =========================================================================
// Phase A: Automatic Rally (AR) — after SC
// =========================================================================
// First rebound after SC. Volume drops, price rallies toward resistance.

fn eval_automatic_rally(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 4 {
        return None;
    }
    // Look for: a Low pivot (SC candidate) followed by a High pivot (AR)
    // where the High retraces at least ar_min_retracement of the prior drop.
    let range = TradingRange::from_pivots(pivots)?;
    let avg = avg_vol_f64(pivots)?;

    // Find last SC-like low (high volume low)
    let mut sc_idx: Option<usize> = None;
    for (i, p) in pivots.iter().enumerate().rev() {
        if p.kind == PivotKind::Low {
            let v = p.volume_at_pivot.to_f64().unwrap_or(0.0);
            if v >= avg * cfg.climax_volume_mult {
                sc_idx = Some(i);
                break;
            }
        }
    }
    let sc_i = sc_idx?;
    let sc_price = pivots[sc_i].price.to_f64()?;

    // Next high after SC
    let ar = pivots[sc_i + 1..].iter().find(|p| p.kind == PivotKind::High)?;
    let ar_price = ar.price.to_f64()?;
    let rally = ar_price - sc_price;
    let drop = range.resistance - sc_price;
    if drop <= 0.0 { return None; }
    let retracement = rally / drop;
    if retracement < cfg.ar_min_retracement {
        return None;
    }
    // AR volume should be lower than SC
    let ar_vol = ar.volume_at_pivot.to_f64().unwrap_or(0.0);
    let sc_vol = pivots[sc_i].volume_at_pivot.to_f64().unwrap_or(1.0);
    let vol_decay = if sc_vol > 0.0 { ar_vol / sc_vol } else { 1.0 };

    let score = (retracement.min(1.0) * 0.6) + ((1.0 - vol_decay).max(0.0) * 0.4);

    let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
    if sc_i < labels.len() { labels[sc_i] = "SC"; }
    // Find AR index
    for (i, p) in pivots.iter().enumerate() {
        if i > sc_i && p.kind == PivotKind::High && p.bar_index == ar.bar_index {
            if i < labels.len() { labels[i] = "AR"; }
            break;
        }
    }
    Some(EventMatch {
        score,
        invalidation: pivots[sc_i].price,
        variant: "accumulation",
        anchor_labels: labels,
    })
}

// =========================================================================
// Phase A: Automatic Reaction (after BC) — distribution mirror
// =========================================================================

fn eval_automatic_reaction(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 4 {
        return None;
    }
    let range = TradingRange::from_pivots(pivots)?;
    let avg = avg_vol_f64(pivots)?;

    // Find last BC-like high (high volume high)
    let mut bc_idx: Option<usize> = None;
    for (i, p) in pivots.iter().enumerate().rev() {
        if p.kind == PivotKind::High {
            let v = p.volume_at_pivot.to_f64().unwrap_or(0.0);
            if v >= avg * cfg.climax_volume_mult {
                bc_idx = Some(i);
                break;
            }
        }
    }
    let bc_i = bc_idx?;
    let bc_price = pivots[bc_i].price.to_f64()?;

    // Next low after BC
    let ar = pivots[bc_i + 1..].iter().find(|p| p.kind == PivotKind::Low)?;
    let ar_price = ar.price.to_f64()?;
    let drop = bc_price - ar_price;
    let range_from_top = bc_price - range.support;
    if range_from_top <= 0.0 { return None; }
    let retracement = drop / range_from_top;
    if retracement < cfg.ar_min_retracement {
        return None;
    }

    let ar_vol = ar.volume_at_pivot.to_f64().unwrap_or(0.0);
    let bc_vol = pivots[bc_i].volume_at_pivot.to_f64().unwrap_or(1.0);
    let vol_decay = if bc_vol > 0.0 { ar_vol / bc_vol } else { 1.0 };
    let score = (retracement.min(1.0) * 0.6) + ((1.0 - vol_decay).max(0.0) * 0.4);

    let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
    if bc_i < labels.len() { labels[bc_i] = "BC"; }
    for (i, p) in pivots.iter().enumerate() {
        if i > bc_i && p.kind == PivotKind::Low && p.bar_index == ar.bar_index {
            if i < labels.len() { labels[i] = "AR"; }
            break;
        }
    }
    Some(EventMatch {
        score,
        invalidation: pivots[bc_i].price,
        variant: "distribution",
        anchor_labels: labels,
    })
}

// =========================================================================
// Phase A: Secondary Test (ST)
// =========================================================================
// Re-test of SC/BC zone with diminishing volume.

fn eval_secondary_test(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 5 {
        return None;
    }
    let range = TradingRange::from_pivots(pivots)?;
    let avg = avg_vol_f64(pivots)?;

    // Look for: SC-like low, then a later low that tests SC zone with lower volume
    for (sc_i, sc) in pivots.iter().enumerate() {
        if sc.kind != PivotKind::Low { continue; }
        let sc_vol = sc.volume_at_pivot.to_f64().unwrap_or(0.0);
        if sc_vol < avg * cfg.climax_volume_mult { continue; }
        let sc_price = sc.price.to_f64().unwrap_or(0.0);

        // Find ST after SC
        for (st_i, st) in pivots.iter().enumerate().skip(sc_i + 2) {
            if st.kind != PivotKind::Low { continue; }
            let st_price = st.price.to_f64().unwrap_or(0.0);
            let st_vol = st.volume_at_pivot.to_f64().unwrap_or(0.0);

            // ST should be near SC level (within range tolerance)
            let dist = (st_price - sc_price).abs() / range.height.max(1e-9);
            if dist > 0.15 { continue; }

            // ST volume must be lower than SC
            if sc_vol > 0.0 && (st_vol / sc_vol) > cfg.st_max_volume_ratio {
                continue;
            }

            let vol_ratio = if sc_vol > 0.0 { 1.0 - (st_vol / sc_vol) } else { 0.0 };
            let price_precision = 1.0 - dist.min(0.15) / 0.15;
            let score = vol_ratio * 0.6 + price_precision * 0.4;

            let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
            if sc_i < labels.len() { labels[sc_i] = "SC"; }
            if st_i < labels.len() { labels[st_i] = "ST"; }

            return Some(EventMatch {
                score,
                invalidation: sc.price,
                variant: "accumulation",
                anchor_labels: labels,
            });
        }
    }

    // Distribution side: BC → ST near resistance
    for (bc_i, bc) in pivots.iter().enumerate() {
        if bc.kind != PivotKind::High { continue; }
        let bc_vol = bc.volume_at_pivot.to_f64().unwrap_or(0.0);
        if bc_vol < avg * cfg.climax_volume_mult { continue; }
        let bc_price = bc.price.to_f64().unwrap_or(0.0);

        for (st_i, st) in pivots.iter().enumerate().skip(bc_i + 2) {
            if st.kind != PivotKind::High { continue; }
            let st_price = st.price.to_f64().unwrap_or(0.0);
            let st_vol = st.volume_at_pivot.to_f64().unwrap_or(0.0);

            let dist = (st_price - bc_price).abs() / range.height.max(1e-9);
            if dist > 0.15 { continue; }
            if bc_vol > 0.0 && (st_vol / bc_vol) > cfg.st_max_volume_ratio {
                continue;
            }

            let vol_ratio = if bc_vol > 0.0 { 1.0 - (st_vol / bc_vol) } else { 0.0 };
            let price_precision = 1.0 - dist.min(0.15) / 0.15;
            let score = vol_ratio * 0.6 + price_precision * 0.4;

            let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
            if bc_i < labels.len() { labels[bc_i] = "BC"; }
            if st_i < labels.len() { labels[st_i] = "ST"; }

            return Some(EventMatch {
                score,
                invalidation: bc.price,
                variant: "distribution",
                anchor_labels: labels,
            });
        }
    }
    None
}

// =========================================================================
// Phase B: Upthrust Action (UA)
// =========================================================================
// Short-lived break above AR resistance in Phase B, closes back inside range.

fn eval_upthrust_action(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < cfg.min_range_pivots + 1 {
        return None;
    }
    let context = &pivots[..pivots.len() - 1];
    let candidate = pivots.last()?;
    if candidate.kind != PivotKind::High {
        return None;
    }
    let range = TradingRange::from_pivots(context)?;
    let price = candidate.price.to_f64()?;
    let exceed = (price - range.resistance) / range.resistance.max(1e-9);

    // Must exceed resistance but not by too much (otherwise it's a breakout)
    if exceed <= 0.0 || exceed > cfg.ua_max_exceed_pct {
        return None;
    }

    // Volume should not be extreme (not a true breakout)
    let avg = avg_vol_f64(pivots)?;
    let vol = candidate.volume_at_pivot.to_f64().unwrap_or(0.0);
    let vol_score = if avg > 0.0 && vol < avg * cfg.climax_volume_mult {
        1.0 - (vol / (avg * cfg.climax_volume_mult))
    } else {
        0.0
    };

    let exceed_score = 1.0 - (exceed / cfg.ua_max_exceed_pct);
    let score = exceed_score * 0.5 + vol_score * 0.5;

    let mut labels: Vec<&'static str> = (0..context.len()).map(label_for).collect();
    labels.push("UA");
    Some(EventMatch {
        score,
        invalidation: Decimal::try_from(range.support).ok().unwrap_or(Decimal::ZERO),
        variant: "distribution",
        anchor_labels: labels,
    })
}

// =========================================================================
// Phase C: Shakeout — deeper and more violent Spring
// =========================================================================

fn eval_shakeout(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < cfg.min_range_pivots + 1 {
        return None;
    }
    let context = &pivots[..pivots.len() - 1];
    let candidate = pivots.last()?;
    if candidate.kind != PivotKind::Low {
        return None;
    }
    let range = TradingRange::from_pivots(context)?;
    let price = candidate.price.to_f64()?;
    if price >= range.support {
        return None;
    }
    let penetration = (range.support - price) / range.height.max(1e-9);
    // Shakeout is deeper than a normal spring
    if penetration < cfg.shakeout_min_penetration {
        return None;
    }
    // But must also have high volume (panic)
    let avg = avg_vol_f64(pivots)?;
    let vol = candidate.volume_at_pivot.to_f64().unwrap_or(0.0);
    if avg > 0.0 && vol < avg * cfg.sc_volume_multiplier * 0.8 {
        return None; // Not enough panic volume
    }

    let vol_score = if avg > 0.0 { (vol / (avg * cfg.sc_volume_multiplier)).min(1.0) } else { 0.5 };
    let pen_score = (penetration / cfg.shakeout_min_penetration).min(2.0) / 2.0;
    let score = vol_score * 0.5 + pen_score * 0.5;

    let mut labels: Vec<&'static str> = (0..context.len()).map(label_for).collect();
    labels.push("Shakeout");
    Some(EventMatch {
        score,
        invalidation: candidate.price,
        variant: "bull",
        anchor_labels: labels,
    })
}

// =========================================================================
// Phase D: Sign of Strength (SOS) — strong rally after Spring/Test
// =========================================================================

fn eval_sign_of_strength(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 5 {
        return None;
    }
    let range = TradingRange::from_pivots(pivots)?;
    let avg = avg_vol_f64(pivots)?;
    let creek = creek_level(&range, cfg.creek_level_percentile);

    // Find the last High pivot that is near or above creek level with strong volume
    let candidate = pivots.iter().rev().find(|p| p.kind == PivotKind::High)?;
    let price = candidate.price.to_f64()?;
    if price < creek * 0.98 {
        return None; // Must be near or above creek
    }
    let vol = candidate.volume_at_pivot.to_f64().unwrap_or(0.0);
    if avg > 0.0 && vol < avg * cfg.sos_min_volume_ratio {
        return None; // Not enough volume
    }

    let above_creek = (price - creek) / range.height.max(1e-9);
    let vol_ratio = if avg > 0.0 { (vol / (avg * cfg.sos_min_volume_ratio)).min(2.0) / 2.0 } else { 0.5 };
    let score = above_creek.min(0.5) + vol_ratio * 0.5;

    let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
    // Label the SOS pivot
    for (i, p) in pivots.iter().enumerate().rev() {
        if p.bar_index == candidate.bar_index {
            if i < labels.len() { labels[i] = "SOS"; }
            break;
        }
    }
    Some(EventMatch {
        score: score.min(1.0),
        invalidation: Decimal::try_from(range.support).ok().unwrap_or(Decimal::ZERO),
        variant: "accumulation",
        anchor_labels: labels,
    })
}

// =========================================================================
// Phase D: Sign of Weakness (SOW) — strong drop after UTAD
// =========================================================================

fn eval_sign_of_weakness(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 5 {
        return None;
    }
    let range = TradingRange::from_pivots(pivots)?;
    let avg = avg_vol_f64(pivots)?;
    let ice = ice_level(&range, cfg.creek_level_percentile);

    let candidate = pivots.iter().rev().find(|p| p.kind == PivotKind::Low)?;
    let price = candidate.price.to_f64()?;
    if price > ice * 1.02 {
        return None;
    }
    let vol = candidate.volume_at_pivot.to_f64().unwrap_or(0.0);
    if avg > 0.0 && vol < avg * cfg.sos_min_volume_ratio {
        return None;
    }

    let below_ice = (ice - price) / range.height.max(1e-9);
    let vol_ratio = if avg > 0.0 { (vol / (avg * cfg.sos_min_volume_ratio)).min(2.0) / 2.0 } else { 0.5 };
    let score = below_ice.min(0.5) + vol_ratio * 0.5;

    let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
    for (i, p) in pivots.iter().enumerate().rev() {
        if p.bar_index == candidate.bar_index {
            if i < labels.len() { labels[i] = "SOW"; }
            break;
        }
    }
    Some(EventMatch {
        score: score.min(1.0),
        invalidation: Decimal::try_from(range.resistance).ok().unwrap_or(Decimal::ZERO),
        variant: "distribution",
        anchor_labels: labels,
    })
}

// =========================================================================
// Phase D: Last Point of Support (LPS)
// =========================================================================
// Shallow pullback after SOS, low volume, holds above creek.

fn eval_last_point_of_support(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 6 {
        return None;
    }
    let range = TradingRange::from_pivots(pivots)?;
    let avg = avg_vol_f64(pivots)?;
    let creek = creek_level(&range, cfg.creek_level_percentile);

    // Need a prior SOS (high above creek with volume) followed by a low pullback
    let mut sos_price = 0.0_f64;
    let mut sos_vol = 0.0_f64;
    let mut sos_found = false;
    let mut sos_idx = 0;

    for (i, p) in pivots.iter().enumerate().rev() {
        if p.kind == PivotKind::High {
            let pr = p.price.to_f64().unwrap_or(0.0);
            let v = p.volume_at_pivot.to_f64().unwrap_or(0.0);
            if pr >= creek * 0.98 && avg > 0.0 && v >= avg * cfg.sos_min_volume_ratio {
                sos_price = pr;
                sos_vol = v;
                sos_found = true;
                sos_idx = i;
                break;
            }
        }
    }
    if !sos_found { return None; }

    // LPS: low after SOS, shallow retracement, low volume
    let lps = pivots[sos_idx + 1..].iter().find(|p| p.kind == PivotKind::Low)?;
    let lps_price = lps.price.to_f64()?;
    let lps_vol = lps.volume_at_pivot.to_f64().unwrap_or(0.0);

    // Must hold above creek
    if lps_price < creek * 0.97 {
        return None;
    }

    let retracement = (sos_price - lps_price) / (sos_price - range.support).max(1e-9);
    if retracement > cfg.lps_max_retracement {
        return None;
    }

    if sos_vol > 0.0 && (lps_vol / sos_vol) > cfg.lps_max_volume_ratio {
        return None;
    }

    let ret_score = 1.0 - (retracement / cfg.lps_max_retracement);
    let vol_score = if sos_vol > 0.0 { 1.0 - (lps_vol / sos_vol) } else { 0.5 };
    let score = ret_score * 0.5 + vol_score.max(0.0) * 0.5;

    let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
    if sos_idx < labels.len() { labels[sos_idx] = "SOS"; }
    for (i, p) in pivots.iter().enumerate() {
        if p.bar_index == lps.bar_index {
            if i < labels.len() { labels[i] = "LPS"; }
            break;
        }
    }
    Some(EventMatch {
        score: score.min(1.0),
        invalidation: Decimal::try_from(range.support).ok().unwrap_or(Decimal::ZERO),
        variant: "accumulation",
        anchor_labels: labels,
    })
}

// =========================================================================
// Phase D: Last Point of Supply (LPSY)
// =========================================================================
// Weak rally after SOW, low volume, stays below ice.

fn eval_last_point_of_supply(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 6 {
        return None;
    }
    let range = TradingRange::from_pivots(pivots)?;
    let avg = avg_vol_f64(pivots)?;
    let ice = ice_level(&range, cfg.creek_level_percentile);

    let mut sow_price = 0.0_f64;
    let mut sow_vol = 0.0_f64;
    let mut sow_found = false;
    let mut sow_idx = 0;

    for (i, p) in pivots.iter().enumerate().rev() {
        if p.kind == PivotKind::Low {
            let pr = p.price.to_f64().unwrap_or(0.0);
            let v = p.volume_at_pivot.to_f64().unwrap_or(0.0);
            if pr <= ice * 1.02 && avg > 0.0 && v >= avg * cfg.sos_min_volume_ratio {
                sow_price = pr;
                sow_vol = v;
                sow_found = true;
                sow_idx = i;
                break;
            }
        }
    }
    if !sow_found { return None; }

    let lpsy = pivots[sow_idx + 1..].iter().find(|p| p.kind == PivotKind::High)?;
    let lpsy_price = lpsy.price.to_f64()?;
    let lpsy_vol = lpsy.volume_at_pivot.to_f64().unwrap_or(0.0);

    if lpsy_price > ice * 1.03 {
        return None;
    }

    let retracement = (lpsy_price - sow_price) / (range.resistance - sow_price).max(1e-9);
    if retracement > cfg.lps_max_retracement {
        return None;
    }

    if sow_vol > 0.0 && (lpsy_vol / sow_vol) > cfg.lps_max_volume_ratio {
        return None;
    }

    let ret_score = 1.0 - (retracement / cfg.lps_max_retracement);
    let vol_score = if sow_vol > 0.0 { 1.0 - (lpsy_vol / sow_vol) } else { 0.5 };
    let score = ret_score * 0.5 + vol_score.max(0.0) * 0.5;

    let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
    if sow_idx < labels.len() { labels[sow_idx] = "SOW"; }
    for (i, p) in pivots.iter().enumerate() {
        if p.bar_index == lpsy.bar_index {
            if i < labels.len() { labels[i] = "LPSY"; }
            break;
        }
    }
    Some(EventMatch {
        score: score.min(1.0),
        invalidation: Decimal::try_from(range.resistance).ok().unwrap_or(Decimal::ZERO),
        variant: "distribution",
        anchor_labels: labels,
    })
}

// =========================================================================
// Phase D: Jump Across Creek (JAC)
// =========================================================================
// Strong move above creek with expanding volume. Accumulation confirmation.

fn eval_jump_across_creek(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 5 {
        return None;
    }
    let range = TradingRange::from_pivots(pivots)?;
    let avg = avg_vol_f64(pivots)?;
    let creek = creek_level(&range, cfg.creek_level_percentile);

    let candidate = pivots.last()?;
    if candidate.kind != PivotKind::High {
        return None;
    }
    let price = candidate.price.to_f64()?;
    if price < creek {
        return None;
    }

    let vol = candidate.volume_at_pivot.to_f64().unwrap_or(0.0);
    if avg > 0.0 && vol < avg * cfg.sos_min_volume_ratio {
        return None;
    }

    let clearance = (price - creek) / range.height.max(1e-9);
    let vol_score = if avg > 0.0 { (vol / (avg * cfg.sos_min_volume_ratio)).min(2.0) / 2.0 } else { 0.5 };
    let score = clearance.min(0.5) * 1.0 + vol_score * 0.5;

    let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
    if let Some(last_label) = labels.last_mut() {
        *last_label = "JAC";
    }
    Some(EventMatch {
        score: score.min(1.0),
        invalidation: Decimal::try_from(creek).ok().unwrap_or(Decimal::ZERO),
        variant: "accumulation",
        anchor_labels: labels,
    })
}

// =========================================================================
// Phase D: Break of Ice
// =========================================================================
// Strong move below ice with expanding volume. Distribution confirmation.

fn eval_break_of_ice(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 5 {
        return None;
    }
    let range = TradingRange::from_pivots(pivots)?;
    let avg = avg_vol_f64(pivots)?;
    let ice = ice_level(&range, cfg.creek_level_percentile);

    let candidate = pivots.last()?;
    if candidate.kind != PivotKind::Low {
        return None;
    }
    let price = candidate.price.to_f64()?;
    if price > ice {
        return None;
    }

    let vol = candidate.volume_at_pivot.to_f64().unwrap_or(0.0);
    if avg > 0.0 && vol < avg * cfg.sos_min_volume_ratio {
        return None;
    }

    let clearance = (ice - price) / range.height.max(1e-9);
    let vol_score = if avg > 0.0 { (vol / (avg * cfg.sos_min_volume_ratio)).min(2.0) / 2.0 } else { 0.5 };
    let score = clearance.min(0.5) * 1.0 + vol_score * 0.5;

    let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
    if let Some(last_label) = labels.last_mut() {
        *last_label = "BreakOfIce";
    }
    Some(EventMatch {
        score: score.min(1.0),
        invalidation: Decimal::try_from(ice).ok().unwrap_or(Decimal::ZERO),
        variant: "distribution",
        anchor_labels: labels,
    })
}

// =========================================================================
// Shortening of Thrust (SOT)
// =========================================================================
// Each successive SOS/SOW thrust covers less distance — momentum is
// waning. We look for 3+ same-kind pivots (all highs or all lows) where
// successive thrust distances decay by cfg.sot_thrust_decay_ratio.

fn eval_shortening_of_thrust(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 5 {
        return None;
    }
    let range = TradingRange::from_pivots(pivots)?;

    // Check highs (bullish SOT → weakening SOS thrusts)
    let highs: Vec<(usize, f64)> = pivots
        .iter()
        .enumerate()
        .filter(|(_, p)| p.kind == PivotKind::High)
        .filter_map(|(i, p)| Some((i, p.price.to_f64()?)))
        .collect();

    if let Some(m) = check_sot_sequence(&highs, cfg.sot_thrust_decay_ratio, pivots, &range, "distribution") {
        return Some(m);
    }

    // Check lows (bearish SOT → weakening SOW thrusts)
    let lows: Vec<(usize, f64)> = pivots
        .iter()
        .enumerate()
        .filter(|(_, p)| p.kind == PivotKind::Low)
        .filter_map(|(i, p)| Some((i, p.price.to_f64()?)))
        .collect();

    check_sot_sequence(&lows, cfg.sot_thrust_decay_ratio, pivots, &range, "accumulation")
}

// =========================================================================
// Phase A: Preliminary Supply (PS)
// =========================================================================
// First high-volume warning in an uptrend that supply is entering. A PS
// precedes the BC by one or more pivots — think of it as the market
// stepping back for the first time in a long markup. Canonical rule:
//   - A High pivot with elevated (but not yet climactic) volume.
//   - Followed by a higher High that IS the climactic BC.
// This is the mirror of how SC does *not* need a predecessor; PS is the
// predecessor. Variant: distribution (PS only precedes distribution).
fn eval_preliminary_supply(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 4 {
        return None;
    }
    let avg = avg_vol_f64(pivots)?;
    let climax_thresh = avg * cfg.climax_volume_mult;
    // PS volume band: elevated but sub-climactic. Use half of climactic
    // as the floor (the pivot must stand out vs noise) and full climax
    // as the ceiling (anything above belongs to the BC, not the PS).
    let ps_lo = avg * (cfg.climax_volume_mult * 0.5).max(1.2);
    let ps_hi = climax_thresh;

    // Walk highs, find PS candidate, then require a subsequent higher-
    // High with >= climactic volume (= the BC). Return the PS match.
    for (ps_i, ps) in pivots.iter().enumerate() {
        if ps.kind != PivotKind::High { continue; }
        let ps_vol = ps.volume_at_pivot.to_f64().unwrap_or(0.0);
        if ps_vol < ps_lo || ps_vol >= ps_hi { continue; }
        let ps_price = ps.price.to_f64().unwrap_or(0.0);

        for bc in pivots.iter().skip(ps_i + 1) {
            if bc.kind != PivotKind::High { continue; }
            let bc_vol = bc.volume_at_pivot.to_f64().unwrap_or(0.0);
            let bc_price = bc.price.to_f64().unwrap_or(0.0);
            if bc_price <= ps_price { continue; }
            if bc_vol < climax_thresh { continue; }

            // Score by how cleanly PS sits in the "elevated but not
            // climactic" band, plus how much higher the BC is.
            let vol_fit = ((ps_vol - ps_lo) / (ps_hi - ps_lo).max(1e-9)).clamp(0.0, 1.0);
            let bc_lift = ((bc_price - ps_price) / ps_price.max(1e-9)).clamp(0.0, 0.1) / 0.1;
            let score = (vol_fit * 0.5 + bc_lift * 0.5).clamp(0.0, 1.0);

            let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
            if ps_i < labels.len() { labels[ps_i] = "PS"; }
            return Some(EventMatch {
                score,
                invalidation: ps.price,
                variant: "distribution",
                anchor_labels: labels,
            });
        }
    }
    None
}

// =========================================================================
// Phase B: Secondary Test in Phase B (ST-B)
// =========================================================================
// Subsequent tests of the range edges *after* Phase A has completed.
// Distinguished from ST by coming after ≥1 UA/ST/other Phase-B activity
// and by generally showing further volume diminishment. We don't have
// direct phase context at detection time, so we approximate: a low/high
// that tests an edge, with even lower volume than a canonical ST would
// accept, AND preceded by at least one earlier edge test.
fn eval_secondary_test_b(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 6 {
        return None;
    }
    let range = TradingRange::from_pivots(pivots)?;
    let _avg = avg_vol_f64(pivots)?; // gate: must have at least some volume data
    let tol = 0.12_f64; // a bit tighter than ST
    let max_vol_ratio = cfg.st_max_volume_ratio * 0.75; // stricter volume decay

    // Collect edge-touching pivots (near support or resistance).
    #[derive(Clone, Copy)]
    struct Touch { i: usize, #[allow(dead_code)] price: f64, vol: f64, is_low: bool }
    let mut touches: Vec<Touch> = Vec::new();
    for (i, p) in pivots.iter().enumerate() {
        let price = p.price.to_f64().unwrap_or(0.0);
        let vol = p.volume_at_pivot.to_f64().unwrap_or(0.0);
        let d_low = (price - range.support).abs() / range.height.max(1e-9);
        let d_high = (price - range.resistance).abs() / range.height.max(1e-9);
        if p.kind == PivotKind::Low && d_low < tol {
            touches.push(Touch { i, price, vol, is_low: true });
        } else if p.kind == PivotKind::High && d_high < tol {
            touches.push(Touch { i, price, vol, is_low: false });
        }
    }
    if touches.len() < 2 { return None; }

    // Best ST-B = latest touch with the largest volume decay vs any earlier
    // same-side touch.
    let last = touches.last().copied()?;
    let mut best_decay = 0.0_f64;
    let mut anchor_prev: Option<usize> = None;
    for prev in &touches[..touches.len() - 1] {
        if prev.is_low != last.is_low { continue; }
        if prev.vol <= 0.0 { continue; }
        let ratio = last.vol / prev.vol;
        if ratio > max_vol_ratio { continue; }
        let decay = 1.0 - ratio;
        if decay > best_decay {
            best_decay = decay;
            anchor_prev = Some(prev.i);
        }
    }
    if best_decay < 0.1 { return None; }
    let _ = anchor_prev; // anchor label below marks only the STB pivot

    let variant = if last.is_low { "accumulation" } else { "distribution" };
    let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
    if last.i < labels.len() { labels[last.i] = "STB"; }

    Some(EventMatch {
        score: best_decay.clamp(0.0, 1.0),
        invalidation: last.is_low
            .then(|| Decimal::try_from(range.support).ok().unwrap_or(Decimal::ZERO))
            .unwrap_or_else(|| Decimal::try_from(range.resistance).ok().unwrap_or(Decimal::ZERO)),
        variant,
        anchor_labels: labels,
    })
}

// =========================================================================
// Phase D: Back Up to Edge of Creek (BUEC)
// =========================================================================
// After JAC (jump above creek = range resistance), price pulls back to
// test the creek from above. The creek used to be resistance and should
// now act as support. Canonical test: a High (JAC) above creek, followed
// by a Low that lands *at* the creek (±tol) with low volume, above the
// creek (did not re-enter the range).
fn eval_back_up_edge_creek(pivots: &[Pivot], cfg: &WyckoffConfig) -> Option<EventMatch> {
    if pivots.len() < 4 {
        return None;
    }
    let range = TradingRange::from_pivots(pivots)?;
    let creek = range.resistance;
    let avg = avg_vol_f64(pivots)?;
    let tol = 0.08_f64;

    // Find a JAC-like high (above creek) then a subsequent low at creek.
    for (jac_i, jac) in pivots.iter().enumerate() {
        if jac.kind != PivotKind::High { continue; }
        let jac_price = jac.price.to_f64().unwrap_or(0.0);
        if jac_price <= creek { continue; }
        // JAC confidence: clearance above creek relative to range height.
        let clearance = (jac_price - creek) / range.height.max(1e-9);
        if clearance < 0.05 { continue; }

        for (buec_i, buec) in pivots.iter().enumerate().skip(jac_i + 1) {
            if buec.kind != PivotKind::Low { continue; }
            let buec_price = buec.price.to_f64().unwrap_or(0.0);
            let buec_vol = buec.volume_at_pivot.to_f64().unwrap_or(0.0);
            // Must land at creek ±tol and stay above it.
            if buec_price < creek { continue; }
            let dist = (buec_price - creek).abs() / range.height.max(1e-9);
            if dist > tol { continue; }
            // Volume decay vs average — BUEC is low-effort test.
            let vol_ratio = if avg > 0.0 { buec_vol / avg } else { 1.0 };
            if vol_ratio > (cfg.st_max_volume_ratio.max(0.8)) { continue; }

            let precision = 1.0 - (dist / tol).min(1.0);
            let volume_score = (1.0 - vol_ratio).clamp(0.0, 1.0);
            let score = (precision * 0.55 + volume_score * 0.25 + clearance.min(0.2) / 0.2 * 0.2)
                .clamp(0.0, 1.0);

            let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
            if jac_i < labels.len() { labels[jac_i] = "JAC"; }
            if buec_i < labels.len() { labels[buec_i] = "BUEC"; }

            return Some(EventMatch {
                score,
                invalidation: Decimal::try_from(creek).ok().unwrap_or(Decimal::ZERO),
                variant: "accumulation",
                anchor_labels: labels,
            });
        }
    }
    None
}

fn check_sot_sequence(
    pts: &[(usize, f64)],
    decay_ratio: f64,
    pivots: &[Pivot],
    range: &TradingRange,
    variant: &'static str,
) -> Option<EventMatch> {
    if pts.len() < 3 {
        return None;
    }
    // Compute thrust distances between consecutive same-kind pivots
    let mut thrusts: Vec<f64> = Vec::new();
    for w in pts.windows(2) {
        thrusts.push((w[1].1 - w[0].1).abs());
    }
    if thrusts.len() < 2 {
        return None;
    }
    // Check decay: each thrust <= decay_ratio * previous
    let mut sot_count = 0;
    for w in thrusts.windows(2) {
        if w[0] > 0.0 && w[1] <= w[0] * decay_ratio {
            sot_count += 1;
        }
    }
    if sot_count == 0 {
        return None;
    }
    let score = (sot_count as f64 / (thrusts.len() - 1) as f64).min(1.0);
    if score < 0.3 {
        return None;
    }

    let mut labels: Vec<&'static str> = (0..pivots.len()).map(label_for).collect();
    // Mark the last few pivots involved
    for &(idx, _) in pts.iter().rev().take(3) {
        if idx < labels.len() {
            labels[idx] = "SOT";
        }
    }
    Some(EventMatch {
        score,
        invalidation: Decimal::try_from(range.support).ok().unwrap_or(Decimal::ZERO),
        variant,
        anchor_labels: labels,
    })
}

//! Candlestick spec dispatch table — each row is a `(name, bars_needed,
//! eval)` triple. The detector iterates top-to-bottom on the latest
//! window and keeps the highest-scoring match. Adding a new pattern =
//! append a row; no central `match` to edit (CLAUDE.md #1).

use crate::config::CandleConfig;
use qtss_domain::v2::bar::Bar;
use rust_decimal::prelude::ToPrimitive;

#[derive(Debug, Clone)]
pub struct CandleMatch {
    pub score: f64,
    /// "bull", "bear", or "neutral".
    pub variant: &'static str,
    /// Bar index range of the pattern, inclusive-inclusive, within the
    /// caller-provided `bars` slice. `end` is always `bars.len() - 1`
    /// (patterns anchor on the most recent closed bar).
    pub start_idx: usize,
    pub end_idx: usize,
}

pub struct CandleSpec {
    pub name: &'static str,
    pub bars_needed: usize,
    pub eval: fn(&[Bar], &CandleConfig) -> Option<CandleMatch>,
}

/// Priority order: more specific 3-bar patterns first so they outrank
/// 1-bar fallbacks when structure overlaps (e.g. morning_star's 3rd bar
/// is also a bullish candle).
pub static CANDLE_SPECS: &[CandleSpec] = &[
    // 3-bar reversal & continuation
    CandleSpec { name: "morning_star",        bars_needed: 3, eval: eval_morning_star },
    CandleSpec { name: "evening_star",        bars_needed: 3, eval: eval_evening_star },
    CandleSpec { name: "three_white_soldiers",bars_needed: 3, eval: eval_three_white_soldiers },
    CandleSpec { name: "three_black_crows",   bars_needed: 3, eval: eval_three_black_crows },
    CandleSpec { name: "three_inside_up",     bars_needed: 3, eval: eval_three_inside_up },
    CandleSpec { name: "three_inside_down",   bars_needed: 3, eval: eval_three_inside_down },
    CandleSpec { name: "three_outside_up",    bars_needed: 3, eval: eval_three_outside_up },
    CandleSpec { name: "three_outside_down",  bars_needed: 3, eval: eval_three_outside_down },
    // 2-bar patterns
    CandleSpec { name: "engulfing",           bars_needed: 2, eval: eval_engulfing },
    CandleSpec { name: "harami",              bars_needed: 2, eval: eval_harami },
    CandleSpec { name: "piercing_line",       bars_needed: 2, eval: eval_piercing_line },
    CandleSpec { name: "dark_cloud_cover",    bars_needed: 2, eval: eval_dark_cloud_cover },
    CandleSpec { name: "tweezer_top",         bars_needed: 2, eval: eval_tweezer_top },
    CandleSpec { name: "tweezer_bottom",      bars_needed: 2, eval: eval_tweezer_bottom },
    // 1-bar patterns
    CandleSpec { name: "dragonfly_doji",      bars_needed: 1, eval: eval_dragonfly_doji },
    CandleSpec { name: "gravestone_doji",     bars_needed: 1, eval: eval_gravestone_doji },
    CandleSpec { name: "long_legged_doji",    bars_needed: 1, eval: eval_long_legged_doji },
    CandleSpec { name: "doji",                bars_needed: 1, eval: eval_doji },
    CandleSpec { name: "hammer",              bars_needed: 1, eval: eval_hammer },
    CandleSpec { name: "inverted_hammer",     bars_needed: 1, eval: eval_inverted_hammer },
    CandleSpec { name: "hanging_man",         bars_needed: 1, eval: eval_hanging_man },
    CandleSpec { name: "shooting_star",       bars_needed: 1, eval: eval_shooting_star },
    CandleSpec { name: "marubozu",            bars_needed: 1, eval: eval_marubozu },
    CandleSpec { name: "spinning_top",        bars_needed: 1, eval: eval_spinning_top },
];

// ---------------------------------------------------------------------------
// Shared candle geometry helpers
// ---------------------------------------------------------------------------

fn f(d: rust_decimal::Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

struct Geom {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
}

impl Geom {
    fn from(b: &Bar) -> Self {
        Self {
            open: f(b.open),
            high: f(b.high),
            low: f(b.low),
            close: f(b.close),
        }
    }
    fn range(&self) -> f64 {
        (self.high - self.low).max(f64::EPSILON)
    }
    fn body(&self) -> f64 {
        (self.close - self.open).abs()
    }
    fn upper_shadow(&self) -> f64 {
        self.high - self.open.max(self.close)
    }
    fn lower_shadow(&self) -> f64 {
        self.open.min(self.close) - self.low
    }
    fn body_ratio(&self) -> f64 {
        self.body() / self.range()
    }
    fn is_bull(&self) -> bool {
        self.close > self.open
    }
    fn is_bear(&self) -> bool {
        self.close < self.open
    }
    fn body_high(&self) -> f64 {
        self.open.max(self.close)
    }
    fn body_low(&self) -> f64 {
        self.open.min(self.close)
    }
}

/// Cumulative return over the last `n` bars strictly preceding `end_exclusive`.
fn prior_trend_pct(bars: &[Bar], end_exclusive: usize, n: usize) -> f64 {
    if end_exclusive < n + 1 {
        return 0.0;
    }
    let start = end_exclusive - n;
    let base = f(bars[start - 1].close);
    if base.abs() < f64::EPSILON {
        return 0.0;
    }
    (f(bars[end_exclusive - 1].close) - base) / base
}

fn has_prior_uptrend(bars: &[Bar], end_exclusive: usize, cfg: &CandleConfig) -> bool {
    prior_trend_pct(bars, end_exclusive, cfg.trend_context_bars) >= cfg.trend_context_min_pct
}

fn has_prior_downtrend(bars: &[Bar], end_exclusive: usize, cfg: &CandleConfig) -> bool {
    prior_trend_pct(bars, end_exclusive, cfg.trend_context_bars) <= -cfg.trend_context_min_pct
}

fn pct_eq(a: f64, b: f64, tol: f64) -> bool {
    let mid = (a.abs() + b.abs()) * 0.5;
    if mid < f64::EPSILON {
        return (a - b).abs() < tol;
    }
    ((a - b).abs() / mid) <= tol
}

// ---------------------------------------------------------------------------
// Single-bar specs
// ---------------------------------------------------------------------------

fn eval_doji(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let g = Geom::from(bars.last()?);
    if g.body_ratio() > cfg.doji_body_ratio_max {
        return None;
    }
    let score = (1.0 - g.body_ratio() / cfg.doji_body_ratio_max).clamp(0.0, 1.0) * 0.6 + 0.3;
    Some(CandleMatch {
        score,
        variant: "neutral",
        start_idx: bars.len() - 1,
        end_idx: bars.len() - 1,
    })
}

fn eval_dragonfly_doji(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let g = Geom::from(bars.last()?);
    if g.body_ratio() > cfg.doji_body_ratio_max {
        return None;
    }
    let upper = g.upper_shadow() / g.range();
    let lower = g.lower_shadow() / g.range();
    if upper > 0.1 || lower < 0.6 {
        return None;
    }
    Some(CandleMatch {
        score: 0.85,
        variant: "bull",
        start_idx: bars.len() - 1,
        end_idx: bars.len() - 1,
    })
}

fn eval_gravestone_doji(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let g = Geom::from(bars.last()?);
    if g.body_ratio() > cfg.doji_body_ratio_max {
        return None;
    }
    let upper = g.upper_shadow() / g.range();
    let lower = g.lower_shadow() / g.range();
    if lower > 0.1 || upper < 0.6 {
        return None;
    }
    Some(CandleMatch {
        score: 0.85,
        variant: "bear",
        start_idx: bars.len() - 1,
        end_idx: bars.len() - 1,
    })
}

fn eval_long_legged_doji(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let g = Geom::from(bars.last()?);
    if g.body_ratio() > cfg.doji_body_ratio_max {
        return None;
    }
    let upper = g.upper_shadow() / g.range();
    let lower = g.lower_shadow() / g.range();
    if upper < 0.35 || lower < 0.35 {
        return None;
    }
    Some(CandleMatch {
        score: 0.75,
        variant: "neutral",
        start_idx: bars.len() - 1,
        end_idx: bars.len() - 1,
    })
}

fn eval_hammer(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let g = Geom::from(bars.last()?);
    let body = g.body().max(f64::EPSILON);
    if g.lower_shadow() / body < cfg.hammer_lower_shadow_ratio_min {
        return None;
    }
    if g.upper_shadow() / body > cfg.hammer_upper_shadow_ratio_max {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 1, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.8,
        variant: "bull",
        start_idx: bars.len() - 1,
        end_idx: bars.len() - 1,
    })
}

fn eval_inverted_hammer(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let g = Geom::from(bars.last()?);
    let body = g.body().max(f64::EPSILON);
    if g.upper_shadow() / body < cfg.hammer_lower_shadow_ratio_min {
        return None;
    }
    if g.lower_shadow() / body > cfg.hammer_upper_shadow_ratio_max {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 1, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.75,
        variant: "bull",
        start_idx: bars.len() - 1,
        end_idx: bars.len() - 1,
    })
}

fn eval_hanging_man(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let g = Geom::from(bars.last()?);
    let body = g.body().max(f64::EPSILON);
    if g.lower_shadow() / body < cfg.hammer_lower_shadow_ratio_min {
        return None;
    }
    if g.upper_shadow() / body > cfg.hammer_upper_shadow_ratio_max {
        return None;
    }
    if !has_prior_uptrend(bars, bars.len() - 1, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.75,
        variant: "bear",
        start_idx: bars.len() - 1,
        end_idx: bars.len() - 1,
    })
}

fn eval_shooting_star(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let g = Geom::from(bars.last()?);
    let body = g.body().max(f64::EPSILON);
    if g.upper_shadow() / body < cfg.hammer_lower_shadow_ratio_min {
        return None;
    }
    if g.lower_shadow() / body > cfg.hammer_upper_shadow_ratio_max {
        return None;
    }
    if !has_prior_uptrend(bars, bars.len() - 1, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.8,
        variant: "bear",
        start_idx: bars.len() - 1,
        end_idx: bars.len() - 1,
    })
}

fn eval_marubozu(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let g = Geom::from(bars.last()?);
    let shadows = g.upper_shadow() + g.lower_shadow();
    if shadows / g.range() > cfg.marubozu_shadow_ratio_max {
        return None;
    }
    if g.body_ratio() < 0.9 {
        return None;
    }
    let variant = if g.is_bull() { "bull" } else if g.is_bear() { "bear" } else { return None };
    Some(CandleMatch {
        score: 0.8,
        variant,
        start_idx: bars.len() - 1,
        end_idx: bars.len() - 1,
    })
}

fn eval_spinning_top(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let g = Geom::from(bars.last()?);
    if g.body_ratio() > cfg.spinning_top_body_ratio_max {
        return None;
    }
    if g.body_ratio() <= cfg.doji_body_ratio_max {
        return None; // that's a doji, handled earlier
    }
    let upper = g.upper_shadow() / g.range();
    let lower = g.lower_shadow() / g.range();
    if upper < 0.25 || lower < 0.25 {
        return None;
    }
    Some(CandleMatch {
        score: 0.6,
        variant: "neutral",
        start_idx: bars.len() - 1,
        end_idx: bars.len() - 1,
    })
}

// ---------------------------------------------------------------------------
// Two-bar specs
// ---------------------------------------------------------------------------

fn last_two(bars: &[Bar]) -> Option<(Geom, Geom)> {
    if bars.len() < 2 {
        return None;
    }
    let a = Geom::from(&bars[bars.len() - 2]);
    let b = Geom::from(&bars[bars.len() - 1]);
    Some((a, b))
}

fn eval_engulfing(bars: &[Bar], _cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b) = last_two(bars)?;
    let start = bars.len() - 2;
    let end = bars.len() - 1;
    // Bullish: prev bear, curr bull, curr body fully engulfs prev body.
    if a.is_bear() && b.is_bull()
        && b.body_low() <= a.body_low()
        && b.body_high() >= a.body_high()
        && b.body() > a.body() * 1.0
    {
        return Some(CandleMatch { score: 0.85, variant: "bull", start_idx: start, end_idx: end });
    }
    // Bearish: prev bull, curr bear, engulfs.
    if a.is_bull() && b.is_bear()
        && b.body_low() <= a.body_low()
        && b.body_high() >= a.body_high()
        && b.body() > a.body() * 1.0
    {
        return Some(CandleMatch { score: 0.85, variant: "bear", start_idx: start, end_idx: end });
    }
    None
}

fn eval_harami(bars: &[Bar], _cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b) = last_two(bars)?;
    let start = bars.len() - 2;
    let end = bars.len() - 1;
    // Prev body must contain curr body entirely, with opposite colours.
    if b.body_low() >= a.body_low() && b.body_high() <= a.body_high() && b.body() < a.body() * 0.7 {
        if a.is_bear() && b.is_bull() {
            return Some(CandleMatch { score: 0.7, variant: "bull", start_idx: start, end_idx: end });
        }
        if a.is_bull() && b.is_bear() {
            return Some(CandleMatch { score: 0.7, variant: "bear", start_idx: start, end_idx: end });
        }
    }
    None
}

fn eval_piercing_line(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b) = last_two(bars)?;
    if !a.is_bear() || !b.is_bull() {
        return None;
    }
    // Curr opens below prev low and closes above prev mid-body.
    let prev_mid = (a.open + a.close) * 0.5;
    if b.open >= a.low || b.close < prev_mid || b.close >= a.open {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 1, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.8,
        variant: "bull",
        start_idx: bars.len() - 2,
        end_idx: bars.len() - 1,
    })
}

fn eval_dark_cloud_cover(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b) = last_two(bars)?;
    if !a.is_bull() || !b.is_bear() {
        return None;
    }
    let prev_mid = (a.open + a.close) * 0.5;
    if b.open <= a.high || b.close > prev_mid || b.close <= a.open {
        return None;
    }
    if !has_prior_uptrend(bars, bars.len() - 1, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.8,
        variant: "bear",
        start_idx: bars.len() - 2,
        end_idx: bars.len() - 1,
    })
}

fn eval_tweezer_top(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b) = last_two(bars)?;
    if !pct_eq(a.high, b.high, cfg.tweezer_price_tol) {
        return None;
    }
    if !has_prior_uptrend(bars, bars.len() - 1, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.7,
        variant: "bear",
        start_idx: bars.len() - 2,
        end_idx: bars.len() - 1,
    })
}

fn eval_tweezer_bottom(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b) = last_two(bars)?;
    if !pct_eq(a.low, b.low, cfg.tweezer_price_tol) {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 1, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.7,
        variant: "bull",
        start_idx: bars.len() - 2,
        end_idx: bars.len() - 1,
    })
}

// ---------------------------------------------------------------------------
// Three-bar specs
// ---------------------------------------------------------------------------

fn last_three(bars: &[Bar]) -> Option<(Geom, Geom, Geom)> {
    if bars.len() < 3 {
        return None;
    }
    Some((
        Geom::from(&bars[bars.len() - 3]),
        Geom::from(&bars[bars.len() - 2]),
        Geom::from(&bars[bars.len() - 1]),
    ))
}

fn eval_morning_star(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    // a: big bear, b: small body (doji-ish) below a's body, c: big bull
    // closing past a's midpoint.
    if !a.is_bear() || !c.is_bull() {
        return None;
    }
    if a.body_ratio() < 0.5 || c.body_ratio() < 0.5 {
        return None;
    }
    if b.body() > a.body() * 0.4 {
        return None;
    }
    let a_mid = (a.open + a.close) * 0.5;
    if c.close < a_mid {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 2, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.9,
        variant: "bull",
        start_idx: bars.len() - 3,
        end_idx: bars.len() - 1,
    })
}

fn eval_evening_star(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    if !a.is_bull() || !c.is_bear() {
        return None;
    }
    if a.body_ratio() < 0.5 || c.body_ratio() < 0.5 {
        return None;
    }
    if b.body() > a.body() * 0.4 {
        return None;
    }
    let a_mid = (a.open + a.close) * 0.5;
    if c.close > a_mid {
        return None;
    }
    if !has_prior_uptrend(bars, bars.len() - 2, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.9,
        variant: "bear",
        start_idx: bars.len() - 3,
        end_idx: bars.len() - 1,
    })
}

fn eval_three_white_soldiers(bars: &[Bar], _cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    if !a.is_bull() || !b.is_bull() || !c.is_bull() {
        return None;
    }
    // Each opens within prior body, closes higher than prior close.
    if b.open <= a.open || b.open >= a.close || b.close <= a.close {
        return None;
    }
    if c.open <= b.open || c.open >= b.close || c.close <= b.close {
        return None;
    }
    if a.body_ratio() < 0.5 || b.body_ratio() < 0.5 || c.body_ratio() < 0.5 {
        return None;
    }
    Some(CandleMatch {
        score: 0.85,
        variant: "bull",
        start_idx: bars.len() - 3,
        end_idx: bars.len() - 1,
    })
}

fn eval_three_black_crows(bars: &[Bar], _cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    if !a.is_bear() || !b.is_bear() || !c.is_bear() {
        return None;
    }
    if b.open >= a.open || b.open <= a.close || b.close >= a.close {
        return None;
    }
    if c.open >= b.open || c.open <= b.close || c.close >= b.close {
        return None;
    }
    if a.body_ratio() < 0.5 || b.body_ratio() < 0.5 || c.body_ratio() < 0.5 {
        return None;
    }
    Some(CandleMatch {
        score: 0.85,
        variant: "bear",
        start_idx: bars.len() - 3,
        end_idx: bars.len() - 1,
    })
}

fn eval_three_inside_up(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    // bullish harami on (a,b) then confirming bull close above a.high
    if !(a.is_bear()
        && b.is_bull()
        && b.body_low() >= a.body_low()
        && b.body_high() <= a.body_high())
    {
        return None;
    }
    if !c.is_bull() || c.close <= a.open {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 2, cfg) {
        return None;
    }
    Some(CandleMatch { score: 0.8, variant: "bull", start_idx: bars.len() - 3, end_idx: bars.len() - 1 })
}

fn eval_three_inside_down(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    if !(a.is_bull()
        && b.is_bear()
        && b.body_low() >= a.body_low()
        && b.body_high() <= a.body_high())
    {
        return None;
    }
    if !c.is_bear() || c.close >= a.open {
        return None;
    }
    if !has_prior_uptrend(bars, bars.len() - 2, cfg) {
        return None;
    }
    Some(CandleMatch { score: 0.8, variant: "bear", start_idx: bars.len() - 3, end_idx: bars.len() - 1 })
}

fn eval_three_outside_up(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    // Bull engulfing on (a,b), then continuing bull close.
    if !(a.is_bear()
        && b.is_bull()
        && b.body_low() <= a.body_low()
        && b.body_high() >= a.body_high())
    {
        return None;
    }
    if !c.is_bull() || c.close <= b.close {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 2, cfg) {
        return None;
    }
    Some(CandleMatch { score: 0.85, variant: "bull", start_idx: bars.len() - 3, end_idx: bars.len() - 1 })
}

fn eval_three_outside_down(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    if !(a.is_bull()
        && b.is_bear()
        && b.body_low() <= a.body_low()
        && b.body_high() >= a.body_high())
    {
        return None;
    }
    if !c.is_bear() || c.close >= b.close {
        return None;
    }
    if !has_prior_uptrend(bars, bars.len() - 2, cfg) {
        return None;
    }
    Some(CandleMatch { score: 0.85, variant: "bear", start_idx: bars.len() - 3, end_idx: bars.len() - 1 })
}

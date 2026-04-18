//! Candlestick spec dispatch table — each row is a `(name, bars_needed,
//! eval)` triple. The detector iterates top-to-bottom on the latest
//! window and keeps the highest-scoring match. Adding a new pattern =
//! append a row; no central `match` to edit (CLAUDE.md #1).

use crate::config::{CandleConfig, TrendMode};
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
    // 2-bar bearish continuations (Faz 10 — neck family, weaker than dark_cloud)
    CandleSpec { name: "on_neck",             bars_needed: 2, eval: eval_on_neck },
    CandleSpec { name: "in_neck",             bars_needed: 2, eval: eval_in_neck },
    CandleSpec { name: "thrusting_line",      bars_needed: 2, eval: eval_thrusting_line },
    // 3-bar reversals with gaps (Faz 10 — abandoned baby, tri-star)
    CandleSpec { name: "abandoned_baby_bull", bars_needed: 3, eval: eval_abandoned_baby_bull },
    CandleSpec { name: "abandoned_baby_bear", bars_needed: 3, eval: eval_abandoned_baby_bear },
    CandleSpec { name: "tri_star_bull",       bars_needed: 3, eval: eval_tri_star_bull },
    CandleSpec { name: "tri_star_bear",       bars_needed: 3, eval: eval_tri_star_bear },
    // 5-bar continuations (Faz 10 — three methods)
    CandleSpec { name: "rising_three_methods", bars_needed: 5, eval: eval_rising_three_methods },
    CandleSpec { name: "falling_three_methods", bars_needed: 5, eval: eval_falling_three_methods },
    // TV parity additions (Aşama post-5.C) — closing the gap against
    // tr.tradingview.com/support/folders/43000570503 catalog.
    // 3-bar strict-doji star variants (ahead of plain *_star so the
    // stricter match wins on overlap).
    CandleSpec { name: "morning_doji_star",   bars_needed: 3, eval: eval_morning_doji_star },
    CandleSpec { name: "evening_doji_star",   bars_needed: 3, eval: eval_evening_doji_star },
    // 3-bar tasuki gap continuations.
    CandleSpec { name: "upside_tasuki_gap",   bars_needed: 3, eval: eval_upside_tasuki_gap },
    CandleSpec { name: "downside_tasuki_gap", bars_needed: 3, eval: eval_downside_tasuki_gap },
    // 2-bar harami with doji inside bar.
    CandleSpec { name: "harami_cross",        bars_needed: 2, eval: eval_harami_cross },
    // 2-bar doji star (precursor to full morning/evening doji star — no
    // 3rd confirming bar yet, so scored lower).
    CandleSpec { name: "doji_star",           bars_needed: 2, eval: eval_doji_star },
    // 2-bar kicking — opposite-color marubozus separated by a gap.
    CandleSpec { name: "kicking",             bars_needed: 2, eval: eval_kicking },
    // 2-bar separating lines — same-direction continuation, opposite-
    // colored bars with identical opens.
    CandleSpec { name: "separating_lines",    bars_needed: 2, eval: eval_separating_lines },
    // 1-bar long-shadow exhaustion reversals.
    CandleSpec { name: "long_lower_shadow",   bars_needed: 1, eval: eval_long_lower_shadow },
    CandleSpec { name: "long_upper_shadow",   bars_needed: 1, eval: eval_long_upper_shadow },
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

/// Simple moving average of `close` over bars[end_exclusive - n .. end_exclusive].
/// Returns `None` when insufficient bars.
fn sma_close(bars: &[Bar], end_exclusive: usize, n: usize) -> Option<f64> {
    if n == 0 || end_exclusive < n {
        return None;
    }
    let start = end_exclusive - n;
    let sum: f64 = bars[start..end_exclusive].iter().map(|b| f(b.close)).sum();
    Some(sum / n as f64)
}

/// Trend classifier dispatch — TV parity. Single source of truth for
/// every reversal pattern guard (CLAUDE.md #1). Returns `(uptrend,
/// downtrend)` in one pass to avoid redundant SMA computation.
///
/// Fallback order when SMA modes lack bars: `Sma50And200` → `Sma50` →
/// `Pct`. Legacy code paths (test fixtures with ~10 bars) auto-degrade
/// to `Pct` without silent guard bypass.
fn classify_trend(bars: &[Bar], end_exclusive: usize, cfg: &CandleConfig) -> (bool, bool) {
    // `None` short-circuits both directions to `true` — pattern emits
    // without trend context (TV'nin "Tespit yok" seçeneği).
    if matches!(cfg.trend_mode, TrendMode::None) {
        return (true, true);
    }
    // SMA candidates — price = last closed bar before `end_exclusive`.
    if end_exclusive == 0 {
        return (false, false);
    }
    let price = f(bars[end_exclusive - 1].close);
    let sma50 = sma_close(bars, end_exclusive, 50);
    let sma200 = sma_close(bars, end_exclusive, 200);

    match cfg.trend_mode {
        TrendMode::Sma50And200 => {
            if let (Some(s50), Some(s200)) = (sma50, sma200) {
                return (price > s50 && s50 > s200, price < s50 && s50 < s200);
            }
            // Fall through to Sma50.
            if let Some(s50) = sma50 {
                return (price > s50, price < s50);
            }
            // Fall through to Pct.
        }
        TrendMode::Sma50 => {
            if let Some(s50) = sma50 {
                return (price > s50, price < s50);
            }
            // Fall through to Pct.
        }
        TrendMode::Pct | TrendMode::None => {}
    }

    let ret = prior_trend_pct(bars, end_exclusive, cfg.trend_context_bars);
    (ret >= cfg.trend_context_min_pct, ret <= -cfg.trend_context_min_pct)
}

fn has_prior_uptrend(bars: &[Bar], end_exclusive: usize, cfg: &CandleConfig) -> bool {
    classify_trend(bars, end_exclusive, cfg).0
}

fn has_prior_downtrend(bars: &[Bar], end_exclusive: usize, cfg: &CandleConfig) -> bool {
    classify_trend(bars, end_exclusive, cfg).1
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

fn eval_harami(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b) = last_two(bars)?;
    let start = bars.len() - 2;
    let end = bars.len() - 1;
    // Geometry: prev body contains curr body entirely; curr body
    // meaningfully smaller (<70% of prev) — the classical inside bar.
    let contained = b.body_low() >= a.body_low()
        && b.body_high() <= a.body_high()
        && b.body() < a.body() * 0.7;
    if !contained {
        return None;
    }
    // Semantic: harami is a *reversal* pattern. Without a prior trend
    // in the opposite direction, the inside bar is just consolidation.
    // Prior context guard matches engulfing / piercing / dark-cloud.
    if a.is_bear() && b.is_bull() && has_prior_downtrend(bars, bars.len() - 1, cfg) {
        return Some(CandleMatch { score: 0.7, variant: "bull", start_idx: start, end_idx: end });
    }
    if a.is_bull() && b.is_bear() && has_prior_uptrend(bars, bars.len() - 1, cfg) {
        return Some(CandleMatch { score: 0.7, variant: "bear", start_idx: start, end_idx: end });
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
    // TV parity (43000592710): 1. bar yeşil (uzun bull), 2. bar kırmızı
    // (bear). Sadece high eşitliği yetmez — renk dönüşü reversal'ın özü.
    if !a.is_bull() || !b.is_bear() {
        return None;
    }
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
    // TV parity (43000592709): 1. bar uzun kırmızı, 2. bar yeşil.
    if !a.is_bear() || !b.is_bull() {
        return None;
    }
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

// ---------------------------------------------------------------------------
// Faz 10 — neck family (2-bar bearish continuation)
//
// Shared geometry: prior bar is a big bear, current bar is a small bull
// that gaps down to open below prev.low then closes *back inside* prev
// body to varying depths:
//   * on_neck        → close ≈ prev.low            (weakest bounce)
//   * in_neck        → close just *into* prev body (≤ ~5%)
//   * thrusting_line → close further in but below mid-body (piercing
//                     above 50% would be piercing_line instead)
// All three suggest the downtrend is intact — buyers couldn't reclaim
// mid-body. Needs a prior downtrend context (reversal pattern family).
// ---------------------------------------------------------------------------

fn eval_on_neck(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b) = last_two(bars)?;
    if !a.is_bear() || !b.is_bull() {
        return None;
    }
    if b.open >= a.low {
        return None;
    }
    // Close within `tweezer_price_tol` of prev.low — riding the neck.
    if !pct_eq(b.close, a.low, cfg.tweezer_price_tol.max(0.005)) {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 1, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.65,
        variant: "bear",
        start_idx: bars.len() - 2,
        end_idx: bars.len() - 1,
    })
}

fn eval_in_neck(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b) = last_two(bars)?;
    if !a.is_bear() || !b.is_bull() {
        return None;
    }
    if b.open >= a.low {
        return None;
    }
    // Close just inside prev body: slightly above prev.close, well
    // below mid. Upper bound ~5% into body; lower bound = prev.close.
    let prev_close = a.close;
    let body = (a.open - a.close).abs().max(f64::EPSILON);
    let top_limit = prev_close + body * 0.05;
    if b.close <= prev_close || b.close > top_limit {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 1, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.7,
        variant: "bear",
        start_idx: bars.len() - 2,
        end_idx: bars.len() - 1,
    })
}

fn eval_thrusting_line(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b) = last_two(bars)?;
    if !a.is_bear() || !b.is_bull() {
        return None;
    }
    if b.open >= a.low {
        return None;
    }
    // Close between prev body mid-point and upper boundary of "in_neck"
    // zone. Deeper than in_neck but shallower than piercing_line (which
    // demands close > mid).
    let prev_close = a.close;
    let body = (a.open - a.close).abs().max(f64::EPSILON);
    let lower = prev_close + body * 0.05;
    let mid = (a.open + a.close) * 0.5;
    if b.close <= lower || b.close >= mid {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 1, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.72,
        variant: "bear",
        start_idx: bars.len() - 2,
        end_idx: bars.len() - 1,
    })
}

// ---------------------------------------------------------------------------
// Faz 10 — abandoned baby (3-bar gap reversal)
//
// Classic rare reversal: trend bar, gap-isolated doji (island), trend
// bar the other way. Distinguishes itself from morning/evening star by
// demanding true gaps on both sides of the doji — the middle bar shares
// no price with either neighbor.
// ---------------------------------------------------------------------------

fn eval_abandoned_baby_bull(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    if !a.is_bear() || !c.is_bull() {
        return None;
    }
    if b.body_ratio() > cfg.doji_body_ratio_max {
        return None;
    }
    // Gap-down on b, gap-up on c (no wick overlap).
    if b.high >= a.low || c.low <= b.high {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 2, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.92,
        variant: "bull",
        start_idx: bars.len() - 3,
        end_idx: bars.len() - 1,
    })
}

fn eval_abandoned_baby_bear(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    if !a.is_bull() || !c.is_bear() {
        return None;
    }
    if b.body_ratio() > cfg.doji_body_ratio_max {
        return None;
    }
    if b.low <= a.high || c.high >= b.low {
        return None;
    }
    if !has_prior_uptrend(bars, bars.len() - 2, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.92,
        variant: "bear",
        start_idx: bars.len() - 3,
        end_idx: bars.len() - 1,
    })
}

// ---------------------------------------------------------------------------
// Faz 10 — tri-star (3 dojis at extreme)
//
// Three consecutive doji bars at trend exhaustion. Directional bias
// comes from the gap between dojis 2 and 3 (away from prior trend).
// Low-frequency but high-weight reversal signal.
// ---------------------------------------------------------------------------

fn eval_tri_star_bull(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    let all_doji = a.body_ratio() <= cfg.doji_body_ratio_max
        && b.body_ratio() <= cfg.doji_body_ratio_max
        && c.body_ratio() <= cfg.doji_body_ratio_max;
    if !all_doji {
        return None;
    }
    // Middle doji gapped down, third gapped up — bullish reversal.
    if b.high >= a.low || c.low <= b.high {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 2, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.88,
        variant: "bull",
        start_idx: bars.len() - 3,
        end_idx: bars.len() - 1,
    })
}

fn eval_tri_star_bear(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    let all_doji = a.body_ratio() <= cfg.doji_body_ratio_max
        && b.body_ratio() <= cfg.doji_body_ratio_max
        && c.body_ratio() <= cfg.doji_body_ratio_max;
    if !all_doji {
        return None;
    }
    if b.low <= a.high || c.high >= b.low {
        return None;
    }
    if !has_prior_uptrend(bars, bars.len() - 2, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.88,
        variant: "bear",
        start_idx: bars.len() - 3,
        end_idx: bars.len() - 1,
    })
}

// ---------------------------------------------------------------------------
// Faz 10 — rising / falling three methods (5-bar continuations)
//
// Geometry:
//   bar 1: strong trend bar (big body in trend direction)
//   bars 2-4: three small counter-trend bars contained inside bar 1's range
//   bar 5: strong trend bar closing beyond bar 1's close
// The three pullback bars never close outside bar 1, confirming the
// trend absorbed the rest attempt. Prior-trend guard keeps it honest.
// ---------------------------------------------------------------------------

fn last_five(bars: &[Bar]) -> Option<[Geom; 5]> {
    if bars.len() < 5 {
        return None;
    }
    let n = bars.len();
    Some([
        Geom::from(&bars[n - 5]),
        Geom::from(&bars[n - 4]),
        Geom::from(&bars[n - 3]),
        Geom::from(&bars[n - 2]),
        Geom::from(&bars[n - 1]),
    ])
}

fn eval_rising_three_methods(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let g = last_five(bars)?;
    let [b1, b2, b3, b4, b5] = g;
    if !b1.is_bull() || !b5.is_bull() {
        return None;
    }
    if b1.body_ratio() < 0.5 || b5.body_ratio() < 0.5 {
        return None;
    }
    // TV parity (43000592711): ortadaki 3 bar kırmızı (karşı-renk).
    // Range-inside + small body de birlikte aranır.
    for m in [&b2, &b3, &b4] {
        if !m.is_bear() {
            return None;
        }
        if m.high > b1.high || m.low < b1.low {
            return None;
        }
        if m.body_ratio() > cfg.spinning_top_body_ratio_max {
            return None;
        }
    }
    if b5.close <= b1.close {
        return None;
    }
    if !has_prior_uptrend(bars, bars.len() - 4, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.86,
        variant: "bull",
        start_idx: bars.len() - 5,
        end_idx: bars.len() - 1,
    })
}

fn eval_falling_three_methods(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let g = last_five(bars)?;
    let [b1, b2, b3, b4, b5] = g;
    if !b1.is_bear() || !b5.is_bear() {
        return None;
    }
    if b1.body_ratio() < 0.5 || b5.body_ratio() < 0.5 {
        return None;
    }
    // TV parity (43000592712): ortadaki 3 bar yeşil (karşı-renk).
    for m in [&b2, &b3, &b4] {
        if !m.is_bull() {
            return None;
        }
        if m.high > b1.high || m.low < b1.low {
            return None;
        }
        if m.body_ratio() > cfg.spinning_top_body_ratio_max {
            return None;
        }
    }
    if b5.close >= b1.close {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 4, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.86,
        variant: "bear",
        start_idx: bars.len() - 5,
        end_idx: bars.len() - 1,
    })
}

// ---------------------------------------------------------------------------
// TV parity additions (post-Aşama 5.C)
//
// Dispatch satırları yukarıda CANDLE_SPECS içine eklendi. Gövdeler klasik
// literatüre dayalıdır; TV'nin bireysel sayfalarıyla bir sonraki turda
// (A adımı) satır satır doğrulanacak — sapma varsa bu fonksiyonlar revize
// edilecek. Hepsi CLAUDE.md #2 gereği eşikleri `CandleConfig`'ten alır.
// ---------------------------------------------------------------------------

/// Morning doji star — morning_star ile aynı yapı, ama ortadaki bar
/// gerçek bir doji (body_ratio ≤ doji eşiği). Daha nadir ve daha güçlü
/// sinyal olduğundan skor biraz daha yüksek.
fn eval_morning_doji_star(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    if !a.is_bear() || !c.is_bull() {
        return None;
    }
    if a.body_ratio() < 0.5 || c.body_ratio() < 0.5 {
        return None;
    }
    if b.body_ratio() > cfg.doji_body_ratio_max {
        return None;
    }
    // Orta doji, a'nın gövdesinin altına sarkmalı (star pozisyonu).
    if b.body_high() >= a.body_low() {
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
        score: 0.93,
        variant: "bull",
        start_idx: bars.len() - 3,
        end_idx: bars.len() - 1,
    })
}

/// Evening doji star — evening_star mirror, orta bar strict doji.
fn eval_evening_doji_star(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    if !a.is_bull() || !c.is_bear() {
        return None;
    }
    if a.body_ratio() < 0.5 || c.body_ratio() < 0.5 {
        return None;
    }
    if b.body_ratio() > cfg.doji_body_ratio_max {
        return None;
    }
    if b.body_low() <= a.body_high() {
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
        score: 0.93,
        variant: "bear",
        start_idx: bars.len() - 3,
        end_idx: bars.len() - 1,
    })
}

/// Upside tasuki gap — bullish continuation:
///   a: güçlü bull
///   b: gap-up ile açılıp bull kapatan bar (b.low > a.high)
///   c: bear bar; b'nin gövdesi içinde açılır, a ile b arasındaki gap'i
///      *kapatmaz* (c.close > a.high).
fn eval_upside_tasuki_gap(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    if !a.is_bull() || !b.is_bull() || !c.is_bear() {
        return None;
    }
    if a.body_ratio() < 0.5 || b.body_ratio() < 0.5 {
        return None;
    }
    // Gap between a and b.
    if b.low <= a.high {
        return None;
    }
    // c opens inside b's body.
    if c.open <= b.body_low() || c.open >= b.body_high() {
        return None;
    }
    // c does not close the gap.
    if c.close <= a.high {
        return None;
    }
    if !has_prior_uptrend(bars, bars.len() - 2, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.78,
        variant: "bull",
        start_idx: bars.len() - 3,
        end_idx: bars.len() - 1,
    })
}

/// Downside tasuki gap — mirror of upside.
fn eval_downside_tasuki_gap(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b, c) = last_three(bars)?;
    if !a.is_bear() || !b.is_bear() || !c.is_bull() {
        return None;
    }
    if a.body_ratio() < 0.5 || b.body_ratio() < 0.5 {
        return None;
    }
    if b.high >= a.low {
        return None;
    }
    if c.open <= b.body_low() || c.open >= b.body_high() {
        return None;
    }
    if c.close >= a.low {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 2, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.78,
        variant: "bear",
        start_idx: bars.len() - 3,
        end_idx: bars.len() - 1,
    })
}

/// Harami cross — harami ama inside bar gerçek doji. Daha güçlü
/// reversal varyantı.
fn eval_harami_cross(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b) = last_two(bars)?;
    if b.body_ratio() > cfg.doji_body_ratio_max {
        return None;
    }
    let contained = b.body_low() >= a.body_low() && b.body_high() <= a.body_high();
    if !contained {
        return None;
    }
    if a.body_ratio() < 0.5 {
        return None;
    }
    let start = bars.len() - 2;
    let end = bars.len() - 1;
    if a.is_bear() && has_prior_downtrend(bars, bars.len() - 1, cfg) {
        return Some(CandleMatch { score: 0.78, variant: "bull", start_idx: start, end_idx: end });
    }
    if a.is_bull() && has_prior_uptrend(bars, bars.len() - 1, cfg) {
        return Some(CandleMatch { score: 0.78, variant: "bear", start_idx: start, end_idx: end });
    }
    None
}

/// Doji star — 2-bar precursor to morning/evening doji star. Büyük trend
/// barı + gap'li doji. 3. teyit barı yok, bu yüzden skor daha düşük.
fn eval_doji_star(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b) = last_two(bars)?;
    if b.body_ratio() > cfg.doji_body_ratio_max {
        return None;
    }
    if a.body_ratio() < 0.5 {
        return None;
    }
    let start = bars.len() - 2;
    let end = bars.len() - 1;
    // Bull setup: downtrend, a bear, b gap-down doji.
    if a.is_bear()
        && b.body_high() < a.body_low()
        && has_prior_downtrend(bars, bars.len() - 1, cfg)
    {
        return Some(CandleMatch { score: 0.65, variant: "bull", start_idx: start, end_idx: end });
    }
    // Bear setup: uptrend, a bull, b gap-up doji.
    if a.is_bull()
        && b.body_low() > a.body_high()
        && has_prior_uptrend(bars, bars.len() - 1, cfg)
    {
        return Some(CandleMatch { score: 0.65, variant: "bear", start_idx: start, end_idx: end });
    }
    None
}

/// Kicking — iki zıt yönlü marubozu ve aralarında gap. Şiddetli reversal;
/// prior-trend guard'ı yok (pattern kendi başına kicker).
fn eval_kicking(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b) = last_two(bars)?;
    // Her iki bar da marubozu — gölgeler ihmal edilebilir, body dominant.
    let shadow_limit = cfg.marubozu_shadow_ratio_max;
    let a_shadows = (a.upper_shadow() + a.lower_shadow()) / a.range();
    let b_shadows = (b.upper_shadow() + b.lower_shadow()) / b.range();
    if a_shadows > shadow_limit || b_shadows > shadow_limit {
        return None;
    }
    if a.body_ratio() < 0.9 || b.body_ratio() < 0.9 {
        return None;
    }
    let start = bars.len() - 2;
    let end = bars.len() - 1;
    // Bull kicking: a bear marubozu, b bull marubozu gap-up.
    if a.is_bear() && b.is_bull() && b.low > a.high {
        return Some(CandleMatch { score: 0.9, variant: "bull", start_idx: start, end_idx: end });
    }
    // Bear kicking: a bull marubozu, b bear marubozu gap-down.
    if a.is_bull() && b.is_bear() && b.high < a.low {
        return Some(CandleMatch { score: 0.9, variant: "bear", start_idx: start, end_idx: end });
    }
    None
}

/// Separating lines — mevcut trend yönünde devam. Zıt renkli iki bar,
/// (neredeyse) aynı açılış fiyatı. Bull: uptrend + bear bar + bull bar
/// (bear'in açılışından açılıp üstüne kapatır).
fn eval_separating_lines(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let (a, b) = last_two(bars)?;
    if !pct_eq(a.open, b.open, cfg.tweezer_price_tol) {
        return None;
    }
    if b.body_ratio() < 0.6 {
        return None;
    }
    let start = bars.len() - 2;
    let end = bars.len() - 1;
    if a.is_bear() && b.is_bull() && has_prior_uptrend(bars, bars.len() - 1, cfg) {
        return Some(CandleMatch { score: 0.7, variant: "bull", start_idx: start, end_idx: end });
    }
    if a.is_bull() && b.is_bear() && has_prior_downtrend(bars, bars.len() - 1, cfg) {
        return Some(CandleMatch { score: 0.7, variant: "bear", start_idx: start, end_idx: end });
    }
    None
}

/// Long lower shadow — 1-bar exhaustion (hammer'ın body-agnostic
/// akrabası). Lower shadow ≥ 60% range, upper shadow küçük, prior
/// downtrend. Hammer'dan farkı: body-oranı kısıtı yok; bu yüzden hammer
/// tetiklemeyen küçük-gövde vakalarını yakalar, skor daha düşük.
fn eval_long_lower_shadow(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let g = Geom::from(bars.last()?);
    let lower = g.lower_shadow() / g.range();
    let upper = g.upper_shadow() / g.range();
    if lower < 0.6 || upper > 0.2 {
        return None;
    }
    if !has_prior_downtrend(bars, bars.len() - 1, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.6,
        variant: "bull",
        start_idx: bars.len() - 1,
        end_idx: bars.len() - 1,
    })
}

/// Long upper shadow — mirror. Upper shadow ≥ 60%, prior uptrend.
fn eval_long_upper_shadow(bars: &[Bar], cfg: &CandleConfig) -> Option<CandleMatch> {
    let g = Geom::from(bars.last()?);
    let upper = g.upper_shadow() / g.range();
    let lower = g.lower_shadow() / g.range();
    if upper < 0.6 || lower > 0.2 {
        return None;
    }
    if !has_prior_uptrend(bars, bars.len() - 1, cfg) {
        return None;
    }
    Some(CandleMatch {
        score: 0.6,
        variant: "bear",
        start_idx: bars.len() - 1,
        end_idx: bars.len() - 1,
    })
}

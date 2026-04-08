//! Generic XABCD matcher.
//!
//! The matcher is independent of any specific harmonic — it takes a set
//! of [`XabcdPoints`] (already normalised so the first leg is positive)
//! and a [`HarmonicSpec`] describing the four ratio ranges, then returns
//! either the proximity score (closer to 1 = better fit) or `None` if
//! any range check fails.

use crate::patterns::HarmonicSpec;

/// Inclusive ratio interval. The matcher accepts any observed ratio
/// inside `[lo, hi]` (after applying the global slack from config).
#[derive(Debug, Clone, Copy)]
pub struct RatioRange {
    pub lo: f64,
    pub hi: f64,
}

impl RatioRange {
    pub const fn new(lo: f64, hi: f64) -> Self {
        Self { lo, hi }
    }

    pub fn contains(&self, observed: f64, slack: f64) -> bool {
        let span = (self.hi - self.lo).max(0.0);
        let pad = span * slack + slack;
        observed >= self.lo - pad && observed <= self.hi + pad
    }

    pub fn center(&self) -> f64 {
        (self.lo + self.hi) / 2.0
    }
}

/// Five-point structure (X, A, B, C, D) in normalised (bullish-positive)
/// form. The detector negates prices for the bearish branch before
/// constructing this so a single matcher loop covers both directions.
#[derive(Debug, Clone, Copy)]
pub struct XabcdPoints {
    pub x: f64,
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
}

impl XabcdPoints {
    /// Returns `(xa_ab, ab_bc, bc_cd, xa_ad)` — the four ratios harmonic
    /// patterns are characterised by. Caller has already normalised so
    /// `a > x`, `b < a`, `c > b`, `d < c` (a bullish-shaped XABCD).
    pub fn ratios(&self) -> Option<(f64, f64, f64, f64)> {
        let xa = self.a - self.x;
        let ab = self.a - self.b;
        let bc = self.c - self.b;
        let cd = self.c - self.d;
        if xa <= 0.0 || ab <= 0.0 || bc <= 0.0 || cd <= 0.0 {
            return None;
        }
        let r_ab = ab / xa;
        let r_bc = bc / ab;
        let r_cd = cd / bc;
        let r_ad = (self.a - self.d) / xa;
        Some((r_ab, r_bc, r_cd, r_ad))
    }
}

/// Match a single spec against a set of points. Returns the proximity
/// score (mean of per-ratio Gaussian closeness to range center) when
/// every range check passes; `None` otherwise.
pub fn match_pattern(spec: &HarmonicSpec, pts: &XabcdPoints, slack: f64) -> Option<f64> {
    let (r_ab, r_bc, r_cd, r_ad) = pts.ratios()?;
    let observed = [r_ab, r_bc, r_cd, r_ad];
    let ranges = [spec.ab, spec.bc, spec.cd, spec.ad];
    for (o, r) in observed.iter().zip(ranges.iter()) {
        if !r.contains(*o, slack) {
            return None;
        }
    }
    Some(score_against_ranges(&observed, &ranges))
}

/// Per-ratio score: 1.0 at the range center, falling off Gaussian-style
/// as the observed ratio drifts toward the edges. We use the half-width
/// as the kernel width so the value at an edge is roughly e^(-1/2) ~ 0.6.
fn score_against_ranges(observed: &[f64; 4], ranges: &[RatioRange; 4]) -> f64 {
    let mut sum = 0.0;
    for (o, r) in observed.iter().zip(ranges.iter()) {
        let center = r.center();
        let half = ((r.hi - r.lo) / 2.0).max(0.01);
        let z = (*o - center) / half;
        sum += (-(z * z) / 2.0).exp();
    }
    sum / 4.0
}

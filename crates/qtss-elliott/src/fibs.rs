//! Fibonacci proximity scoring.
//!
//! Each wave checked against a small set of canonical Fibonacci ratios.
//! The score is the proximity of the observed ratio to the closest
//! reference, mapped through a Gaussian-ish kernel so a perfect hit
//! scores 1.0 and a miss tapers smoothly to zero.

/// Reference ratios per wave. Kept as plain `&[f64]` so adding a ratio
/// is one literal edit, not a code path change.
pub const WAVE2_REFS: &[f64] = &[0.382, 0.5, 0.618, 0.786];
pub const WAVE3_REFS: &[f64] = &[1.618, 2.0, 2.618];
pub const WAVE4_REFS: &[f64] = &[0.236, 0.382, 0.5];

/// Map an observed ratio to a 0..1 closeness score against the nearest
/// reference. The kernel width controls how forgiving the score is —
/// 0.05 means a 5% deviation is roughly half the perfect score.
pub fn proximity_score(observed: f64, refs: &[f64]) -> f64 {
    let nearest = refs
        .iter()
        .map(|r| (observed - r).abs())
        .fold(f64::INFINITY, f64::min);
    let width = 0.05;
    (-(nearest * nearest) / (2.0 * width * width)).exp()
}

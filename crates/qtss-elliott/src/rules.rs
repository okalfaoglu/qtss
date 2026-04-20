//! Elliott impulse validity rules.
//!
//! Each rule is a single function returning `Result<(), &'static str>`.
//! The detector runs them through a slice of function pointers — adding
//! a new rule = appending one function, no central match arm to edit
//! (CLAUDE.md rule #1).

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

/// Five canonical impulse points (price only — direction is encoded by
/// the order). For a bullish impulse: p0 < p1, p2 > p0 < p1, etc.
/// The detector normalizes bearish impulses by negating prices before
/// checking, so a single rule set covers both directions.
#[derive(Debug, Clone, Copy)]
pub struct ImpulsePoints {
    pub p0: Decimal,
    pub p1: Decimal,
    pub p2: Decimal,
    pub p3: Decimal,
    pub p4: Decimal,
    pub p5: Decimal,
}

impl ImpulsePoints {
    /// Convert to f64 in normalized (bullish-positive) form. Bearish
    /// callers pass negated prices; the rules don't care.
    pub fn as_f64(&self) -> [f64; 6] {
        [
            self.p0.to_f64().unwrap_or(0.0),
            self.p1.to_f64().unwrap_or(0.0),
            self.p2.to_f64().unwrap_or(0.0),
            self.p3.to_f64().unwrap_or(0.0),
            self.p4.to_f64().unwrap_or(0.0),
            self.p5.to_f64().unwrap_or(0.0),
        ]
    }
}

pub type Rule = fn(&[f64; 6]) -> Result<(), &'static str>;

/// Ordered rule list. Strictly sequential — first failure short-circuits.
pub const RULES: &[Rule] = &[
    rule_alternation,
    rule_wave2_no_break_below_start,
    rule_wave3_not_shortest,
    rule_wave4_no_overlap,
];

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

/// Sanity: alternating extremes — p0<p1>p2<p3>p4<p5 (in normalized form).
fn rule_alternation(p: &[f64; 6]) -> Result<(), &'static str> {
    let ok = p[0] < p[1] && p[1] > p[2] && p[2] < p[3] && p[3] > p[4] && p[4] < p[5];
    if ok {
        Ok(())
    } else {
        Err("alternation broken")
    }
}

/// Rule 1: wave 2 may not retrace **past** the start of wave 1.
/// Frost & Prechter phrasing — a *touch* of p0 is tolerated, only a
/// clean break below (bullish frame) violates the rule. Earlier `p[2]
/// > p[0]` rejected valid impulses that retraced to round-number
/// levels pinned to p0 (common at 1.0000, 100, 100k). We allow a
/// 0.1% tolerance to absorb float / decimal-to-f64 rounding too.
fn rule_wave2_no_break_below_start(p: &[f64; 6]) -> Result<(), &'static str> {
    let tolerance = p[0].abs() * 1e-3;
    if p[2] >= p[0] - tolerance {
        Ok(())
    } else {
        Err("wave 2 retraced past wave 1 start")
    }
}

/// Rule 2: wave 3 may not be the shortest of waves 1, 3, 5.
fn rule_wave3_not_shortest(p: &[f64; 6]) -> Result<(), &'static str> {
    let w1 = p[1] - p[0];
    let w3 = p[3] - p[2];
    let w5 = p[5] - p[4];
    if w3 >= w1 || w3 >= w5 {
        Ok(())
    } else {
        Err("wave 3 is the shortest")
    }
}

/// Rule 3: wave 4 may not enter wave 1 territory (no overlap).
fn rule_wave4_no_overlap(p: &[f64; 6]) -> Result<(), &'static str> {
    if p[4] > p[1] {
        Ok(())
    } else {
        Err("wave 4 overlaps wave 1")
    }
}

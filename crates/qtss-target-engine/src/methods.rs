//! Target projection methods.
//!
//! Each method implements [`TargetMethodCalc`] and decides for itself
//! whether the candidate detection has the right shape to project from.
//! Methods that don't apply simply return an empty `Vec`. Adding a new
//! method (Wolfe-wave projection, ATR multiples, …) is one impl + one
//! `engine.register(...)` call — no central match arm to edit
//! (CLAUDE.md rule #1).

use qtss_domain::v2::detection::{Detection, PatternKind, PivotRef, Target, TargetMethod};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

pub trait TargetMethodCalc: Send + Sync {
    fn name(&self) -> &'static str;
    fn project(&self, det: &Detection) -> Vec<Target>;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Long,
    Short,
}

/// Direction inferred from the kind subkind suffix. Returns `None` for
/// neutral / ambiguous patterns.
pub fn direction_of(kind: &PatternKind) -> Option<Direction> {
    let subkind = subkind_str(kind);
    if subkind.contains("bull") || subkind.contains("bottom") || subkind.contains("accumulation") {
        Some(Direction::Long)
    } else if subkind.contains("bear")
        || subkind.contains("top")
        || subkind.contains("distribution")
        || subkind.contains("upthrust")
    {
        Some(Direction::Short)
    } else {
        None
    }
}

fn subkind_str(kind: &PatternKind) -> &str {
    match kind {
        PatternKind::Elliott(s) => s,
        PatternKind::Harmonic(s) => s,
        PatternKind::Classical(s) => s,
        PatternKind::Wyckoff(s) => s,
        PatternKind::Range(s) => s,
        PatternKind::Custom(s) => s,
    }
}

fn anchor<'a>(det: &'a Detection, label: &str) -> Option<&'a PivotRef> {
    det.anchors.iter().find(|a| a.label.as_deref() == Some(label))
}

fn project_price(base: f64, distance: f64, dir: Direction) -> f64 {
    match dir {
        Direction::Long => base + distance,
        Direction::Short => base - distance,
    }
}

fn d(value: f64) -> Decimal {
    Decimal::from_f64_retain(value).unwrap_or(Decimal::ZERO)
}

// ---------------------------------------------------------------------------
// Measured Move (classical)
// ---------------------------------------------------------------------------
//
// For double top/bottom and head & shoulders, the textbook measured-move
// target is `breakout_level ± pattern_height`. We approximate the
// breakout level with the highest neckline / lowest neckline pivot.

pub struct MeasuredMoveMethod;

impl TargetMethodCalc for MeasuredMoveMethod {
    fn name(&self) -> &'static str {
        "measured_move"
    }

    fn project(&self, det: &Detection) -> Vec<Target> {
        let dir = match direction_of(&det.kind) {
            Some(d) => d,
            None => return Vec::new(),
        };
        let (label_top, label_base) = match &det.kind {
            PatternKind::Classical(name) if name.starts_with("double_top") => ("H1", "T"),
            PatternKind::Classical(name) if name.starts_with("double_bottom") => ("L1", "T"),
            PatternKind::Classical(name) if name.starts_with("head_and_shoulders") => ("H", "N1"),
            PatternKind::Classical(name) if name.starts_with("inverse_head_and_shoulders") => {
                ("H", "N1")
            }
            _ => return Vec::new(),
        };
        let top: f64 = match anchor(det, label_top).and_then(|p| p.price.to_f64()) {
            Some(v) => v,
            None => return Vec::new(),
        };
        let base: f64 = match anchor(det, label_base).and_then(|p| p.price.to_f64()) {
            Some(v) => v,
            None => return Vec::new(),
        };
        let height: f64 = (top - base).abs();
        if height <= 0.0 {
            return Vec::new();
        }
        // Two graduated targets: 100% and 161.8% of the height projected
        // from the breakout level (= the base/neckline).
        let t1 = project_price(base, height, dir);
        let t2 = project_price(base, height * 1.618, dir);
        vec![
            Target {
                price: d(t1),
                method: TargetMethod::MeasuredMove,
                weight: 0.8,
                label: Some("MM 1.0x".into()),
            },
            Target {
                price: d(t2),
                method: TargetMethod::MeasuredMove,
                weight: 0.5,
                label: Some("MM 1.618x".into()),
            },
        ]
    }
}

// ---------------------------------------------------------------------------
// Fibonacci extension (Elliott impulse)
// ---------------------------------------------------------------------------
//
// For an Elliott impulse the conventional projection is `0 → 1 + (1.0,
// 1.618, 2.618) × wave1` measured from wave 4 / point 4.

pub struct FibExtensionMethod;

impl TargetMethodCalc for FibExtensionMethod {
    fn name(&self) -> &'static str {
        "fib_extension"
    }

    fn project(&self, det: &Detection) -> Vec<Target> {
        let dir = match direction_of(&det.kind) {
            Some(d) => d,
            None => return Vec::new(),
        };
        if !matches!(&det.kind, PatternKind::Elliott(name) if name.starts_with("impulse")) {
            return Vec::new();
        }
        let p0_o: Option<f64> = anchor(det, "0").and_then(|p| p.price.to_f64());
        let p1_o: Option<f64> = anchor(det, "1").and_then(|p| p.price.to_f64());
        let p4_o: Option<f64> = anchor(det, "4").and_then(|p| p.price.to_f64());
        let (p0, p1, p4): (f64, f64, f64) = match (p0_o, p1_o, p4_o) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => return Vec::new(),
        };
        let wave1: f64 = (p1 - p0).abs();
        if wave1 <= 0.0 {
            return Vec::new();
        }
        let levels = [(1.0, 0.7, "fib 1.0"), (1.618, 0.85, "fib 1.618"), (2.618, 0.55, "fib 2.618")];
        levels
            .into_iter()
            .map(|(mult, w, label)| Target {
                price: d(project_price(p4, wave1 * mult, dir)),
                method: TargetMethod::FibExtension,
                weight: w,
                label: Some(label.into()),
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Harmonic PRZ retracement (XABCD)
// ---------------------------------------------------------------------------
//
// Pattern-specific PRZ projection: each harmonic has different target levels
// based on its XABCD structure. We use both:
//   1. AD-leg retracements (pattern-specific ratios)
//   2. XA-leg extensions (for patterns like Butterfly/Crab that extend beyond X)
//   3. PRZ zone (range between closest targets = high-probability reversal area)

pub struct HarmonicRetracementMethod;

/// Pattern-specific PRZ target ratios (AD retracement + XA extension).
/// Format: (ad_retracement_ratio, weight, label)
fn harmonic_target_levels(subkind: &str) -> Vec<(f64, f32, &'static str)> {
    // Strip direction prefix (e.g. "bull_gartley" → "gartley")
    let name = subkind
        .strip_prefix("bull_").or_else(|| subkind.strip_prefix("bear_"))
        .unwrap_or(subkind);

    match name {
        "gartley" => vec![
            // Gartley: D at 0.786 XA → targets retrace toward A
            (0.382, 0.70, "gartley T1 (0.382 AD)"),
            (0.618, 0.90, "gartley T2 (0.618 AD)"),
            (0.786, 0.65, "gartley T3 (0.786 AD)"),
        ],
        "bat" => vec![
            // Bat: D at 0.886 XA → deeper reversal, conservative targets
            (0.382, 0.75, "bat T1 (0.382 AD)"),
            (0.618, 0.85, "bat T2 (0.618 AD)"),
            (1.0,   0.50, "bat T3 (1.0 AD)"),
        ],
        "butterfly" => vec![
            // Butterfly: D extends beyond X (1.27 XA) → aggressive targets
            (0.618, 0.80, "butterfly T1 (0.618 AD)"),
            (1.0,   0.90, "butterfly T2 (1.0 AD)"),
            (1.272, 0.60, "butterfly T3 (1.272 AD)"),
        ],
        "crab" => vec![
            // Crab: D at 1.618 XA → most extreme extension, strongest reversal
            (0.618, 0.85, "crab T1 (0.618 AD)"),
            (1.0,   0.90, "crab T2 (1.0 AD)"),
            (1.618, 0.55, "crab T3 (1.618 AD)"),
        ],
        _ => vec![
            // Generic fallback
            (0.382, 0.65, "harm 0.382"),
            (0.618, 0.85, "harm 0.618"),
            (1.0,   0.45, "harm 1.0"),
        ],
    }
}

impl TargetMethodCalc for HarmonicRetracementMethod {
    fn name(&self) -> &'static str {
        "harmonic_retracement"
    }

    fn project(&self, det: &Detection) -> Vec<Target> {
        let dir = match direction_of(&det.kind) {
            Some(d) => d,
            None => return Vec::new(),
        };
        let subkind = match &det.kind {
            PatternKind::Harmonic(s) => s.as_str(),
            _ => return Vec::new(),
        };

        let a_o: Option<f64> = anchor(det, "A").and_then(|p| p.price.to_f64());
        let d_o: Option<f64> = anchor(det, "D").and_then(|p| p.price.to_f64());
        let x_o: Option<f64> = anchor(det, "X").and_then(|p| p.price.to_f64());
        let (a, d_pt): (f64, f64) = match (a_o, d_o) {
            (Some(a), Some(d)) => (a, d),
            _ => return Vec::new(),
        };
        let ad_leg: f64 = (a - d_pt).abs();
        if ad_leg <= 0.0 {
            return Vec::new();
        }

        let levels = harmonic_target_levels(subkind);
        let mut targets: Vec<Target> = levels
            .into_iter()
            .map(|(mult, w, label)| Target {
                price: d(project_price(d_pt, ad_leg * mult, dir)),
                method: TargetMethod::HarmonicPrz,
                weight: w,
                label: Some(label.into()),
            })
            .collect();

        // PRZ zone target: midpoint of the two closest targets as the
        // "sweet spot" where multiple confluences converge.
        if targets.len() >= 2 {
            let t1_price = targets[0].price.to_f64().unwrap_or(0.0);
            let t2_price = targets[1].price.to_f64().unwrap_or(0.0);
            let prz_mid = (t1_price + t2_price) / 2.0;
            targets.push(Target {
                price: d(prz_mid),
                method: TargetMethod::HarmonicPrz,
                weight: 0.95,
                label: Some(format!("{} PRZ zone", subkind.split('_').last().unwrap_or(subkind))),
            });
        }

        // XA extension targets for extended patterns (butterfly, crab)
        if let Some(x_pt) = x_o {
            let xa_leg = (a - x_pt).abs();
            if xa_leg > 0.0 && (subkind.contains("butterfly") || subkind.contains("crab")) {
                // BC projection from D: 0.618 and 1.0 of XA as secondary targets
                let ext_1 = project_price(d_pt, xa_leg * 0.618, dir);
                targets.push(Target {
                    price: d(ext_1),
                    method: TargetMethod::HarmonicPrz,
                    weight: 0.55,
                    label: Some(format!("{} XA 0.618", subkind.split('_').last().unwrap_or(subkind))),
                });
            }
        }

        targets
    }
}

// ---------------------------------------------------------------------------
// Wyckoff range projection
// ---------------------------------------------------------------------------
//
// After a Spring or Upthrust the range height is the textbook target.
// We use the trading-range pivots' min/max as a proxy for the range
// height and project from the false-break low/high.

pub struct WyckoffRangeMethod;

impl TargetMethodCalc for WyckoffRangeMethod {
    fn name(&self) -> &'static str {
        "wyckoff_range"
    }

    fn project(&self, det: &Detection) -> Vec<Target> {
        let dir = match direction_of(&det.kind) {
            Some(d) => d,
            None => return Vec::new(),
        };
        let is_spring = matches!(&det.kind, PatternKind::Wyckoff(name) if name.starts_with("spring"));
        let is_upthrust = matches!(&det.kind, PatternKind::Wyckoff(name) if name.starts_with("upthrust"));
        if !(is_spring || is_upthrust) {
            return Vec::new();
        }
        // Range height = max(price) - min(price) over all anchors except
        // the false-break pivot itself (which is the last one).
        if det.anchors.len() < 2 {
            return Vec::new();
        }
        let (last, body) = det.anchors.split_last().unwrap();
        let mut hi = f64::MIN;
        let mut lo = f64::MAX;
        for piv in body {
            let v = match piv.price.to_f64() {
                Some(v) => v,
                None => return Vec::new(),
            };
            hi = hi.max(v);
            lo = lo.min(v);
        }
        let height = hi - lo;
        let last_price = match last.price.to_f64() {
            Some(v) => v,
            None => return Vec::new(),
        };
        if height <= 0.0 {
            return Vec::new();
        }
        let t1 = project_price(last_price, height * 0.5, dir);
        let t2 = project_price(last_price, height, dir);
        vec![
            Target {
                price: d(t1),
                method: TargetMethod::MeasuredMove,
                weight: 0.7,
                label: Some("wyckoff 0.5x".into()),
            },
            Target {
                price: d(t2),
                method: TargetMethod::MeasuredMove,
                weight: 0.85,
                label: Some("wyckoff 1.0x".into()),
            },
        ]
    }
}

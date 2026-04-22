//! Harmonic pattern catalog.
//!
//! Each entry encodes the four canonical ratio ranges for one harmonic,
//! plus an optional cross-ratio constraint `cd_over_ab` used by the two
//! AB=CD variants to enforce Carney's CD/AB price-equality rule (or the
//! 1.27 / 1.618 multiplier rule for the Alternate variant). Ranges are
//! deliberately a touch wider than the strictest textbook values so
//! realistic noise on live data still matches; HarmonicConfig exposes a
//! global slack on top for further loosening.
//!
//! Adding a new pattern (Shark, Cypher, 5-0, ...) is one entry in
//! `PATTERNS` — no central match arm to edit.

use crate::matcher::RatioRange;

#[derive(Debug, Clone)]
pub struct HarmonicSpec {
    pub name: &'static str,
    pub ab: RatioRange, // AB / XA
    pub bc: RatioRange, // BC / AB
    pub cd: RatioRange, // CD / BC
    pub ad: RatioRange, // AD / XA (can be negative — see 5-0)
    /// Optional cross-ratio `CD / AB` constraint. Encodes Carney's
    /// "CD = AB" rule (classic ABCD → ~1.0) and "CD = AB × 1.27 or
    /// 1.618" rule (Alternate ABCD → ~1.27 / ~1.618) which the
    /// independent `bc` / `cd` ranges cannot enforce on their own
    /// (`r_bc × r_cd ≠ 1` is allowed in the independent checks but
    /// invalidates the AB=CD core invariant).
    ///
    /// `None` for patterns where the cross-ratio isn't part of the
    /// spec (everything except `ab_cd` and `alt_ab_cd`).
    pub cd_over_ab: Option<RatioRange>,
    /// True → pattern completes with D **beyond** X (classic extensions
    /// like Butterfly/Crab) **or** outside the XA envelope in a way that
    /// demands a D-anchored stop rather than an X-anchored one (5-0).
    ///
    /// False → classic retracement pattern where D stays between X and A;
    /// invalidation sits at X.
    ///
    /// This used to be inferred from `ad.hi > 1.0`, but 5-0 has AD near
    /// zero (D collides with A) while still needing a tight D-anchored
    /// stop — so the flag is now explicit per spec.
    pub extension: bool,
}

pub const PATTERNS: &[HarmonicSpec] = &[
    // ── Gartley (Scott Carney, originally H.M. Gartley) ──────────────
    // Ref: https://harmonictrader.com/harmonic-patterns/gartley-pattern/
    // Carney: "0.618 at B point and 0.786 at D point" — strict.
    // CD projection: 1.272 OR 1.618 of BC (AB=CD reciprocal).
    HarmonicSpec {
        name: "gartley",
        ab: RatioRange::new(0.55, 0.65),  // ~0.618 (strict)
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(1.27, 1.618), // Carney: "1.27 or 1.618", NOT 1.13+
        ad: RatioRange::new(0.74, 0.82),  // ~0.786 (strict)
        cd_over_ab: None,
        extension: false,
    },
    // ── Bat (Scott Carney) ─────────────────────────────────────────
    // Ref: https://harmonictrader.com/harmonic-patterns/bat-pattern/
    // Carney: "B less than 0.618, preferably 0.50 or 0.382", "BC
    // projection minimum 1.618, ideally 1.618 or 2.0 (NOT 1.27)",
    // "0.886 XA retracement — defining element in the PRZ".
    HarmonicSpec {
        name: "bat",
        ab: RatioRange::new(0.382, 0.50),
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(1.618, 2.618),
        ad: RatioRange::new(0.84, 0.92),  // ~0.886
        cd_over_ab: None,
        extension: false,
    },
    // ── Butterfly (Bryce Gilmore, formalized by Scott Carney) ────────
    // Ref: https://harmonictrader.com/harmonic-patterns/butterfly-pattern/
    // Carney: "mandatory 0.786 retracement of XA as B", "BC projection
    // typical 1.618, extreme 2.0/2.24/2.618", "1.27 XA projection" for D
    // (extends beyond X).
    HarmonicSpec {
        name: "butterfly",
        ab: RatioRange::new(0.74, 0.82),   // mandatory 0.786
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(1.618, 2.618), // Carney: up to 2.618 extreme
        ad: RatioRange::new(1.20, 1.65),   // 1.27..1.618 XA extension
        cd_over_ab: None,
        extension: true,
    },
    // ── Crab (Scott Carney) ────────────────────────────────────────
    // Ref: https://harmonictrader.com/harmonic-patterns/crab-pattern/
    // Carney: "B retracement of XA less than 0.618", "BC projection
    // extreme (2.24, 2.618, 3.14, 3.618)", "1.618 XA projection —
    // THE defining level of the PRZ, exclusively".
    HarmonicSpec {
        name: "crab",
        ab: RatioRange::new(0.382, 0.618),
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(2.24, 3.618),
        ad: RatioRange::new(1.55, 1.70),  // ~1.618 extension
        cd_over_ab: None,
        extension: true,
    },
    // ── Deep Crab (Scott Carney) ─────────────────────────────────────
    // Ref: https://harmonictrader.com/harmonic-patterns/deep-crab-pattern/
    // Carney: "B must be 0.886 retracement", "extreme (2.24, 2.618, 3.14,
    // 3.618) projection of BC", "1.618 XA projection" for D (exact).
    HarmonicSpec {
        name: "deep_crab",
        ab: RatioRange::new(0.84, 0.92),   // 0.886 (exact)
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(2.24, 3.618),  // Carney: min 2.24 (was 2.0 — too loose)
        ad: RatioRange::new(1.55, 1.70),   // 1.618 (exact)
        cd_over_ab: None,
        extension: true,
    },
    // ── Shark (Scott Carney, 2011) ───────────────────────────────────
    // Ref: https://harmonictrader.com/harmonic-patterns/shark-pattern/
    // 0-X-A-B-C structure mapped to XABCD. Distinctive: "0.886
    // retracement / 1.13 Reciprocal Ratio" is the core PRZ band per
    // Carney. AD > 1.0, BC between 1.13 and 1.618, CD 1.618-2.24.
    HarmonicSpec {
        name: "shark",
        ab: RatioRange::new(0.382, 0.618),
        bc: RatioRange::new(1.13, 1.618),
        cd: RatioRange::new(1.618, 2.24),
        ad: RatioRange::new(0.84, 1.13),  // 0.886–1.13
        cd_over_ab: None,
        extension: true,
    },
    // ── Cypher (Darren Oglesbee) ─────────────────────────────────────
    // BC extends beyond A (1.272–1.414 of XA), CD retraces to 0.786 of XC.
    HarmonicSpec {
        name: "cypher",
        ab: RatioRange::new(0.382, 0.618),
        bc: RatioRange::new(1.13, 1.414),
        cd: RatioRange::new(1.272, 2.0),
        ad: RatioRange::new(0.74, 0.82),  // ~0.786 of XC
        cd_over_ab: None,
        extension: false,
    },
    // ── Alt Bat (Scott Carney) ───────────────────────────────────────
    // Ref: https://harmonictrader.com/harmonic-patterns/alternate-bat-pattern/
    // Carney: "0.382 or less at B (vs classic Bat's 0.50)", "minimum
    // 2.0 BC projection", "1.618 AB=CD", "1.13 XA extension as the
    // defining PRZ element". Distinguishing feature = shallow B.
    HarmonicSpec {
        name: "alt_bat",
        ab: RatioRange::new(0.33, 0.44),  // ~0.382
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(2.0, 3.618),
        ad: RatioRange::new(1.08, 1.18),  // ~1.13
        cd_over_ab: None,
        extension: true,
    },
    // ── 5-0 (Scott Carney, Harmonic Trading Vol. 2) ─────────────────
    // Reference: https://harmonictrader.com/harmonic-patterns/5-0/
    //
    // Structure: 0-X-A-B-C-D (B is an EXTENSION of XA — NOT a retrace).
    //   * r_ab = AB/XA ∈ [1.13, 1.618]  (XA projection that defines B,
    //     Carney: "must not exceed 1.618")
    //   * r_bc = BC/AB ∈ [1.618, 2.24]  (Carney: "strict — the defining
    //     band, exceeding 2.24 negates the pattern")
    //   * r_cd = CD/BC ≈ 0.50           (D at 50% retrace of BC)
    //   * Reciprocal AB=CD rule (Carney's defining measurement):
    //     "D = C − AB projected from C" → CD/AB ≈ 1.0. Enforced via
    //     cd_over_ab below. The r_bc × r_cd cross product already
    //     centers at 1.0 when r_bc = 2.0 and r_cd = 0.50 — the explicit
    //     cross ratio picks off configurations at the range edges that
    //     mathematically violate the AB=CD equality despite passing
    //     the independent leg checks.
    //   * r_ad derivation (with α=r_ab, β=r_bc):
    //         D = A + α·(0.5β − 1),  r_ad = α·(1 − 0.5β)
    //     For α ∈ [1.13, 1.618], β ∈ [1.618, 2.24]:
    //       β=1.618 → r_ad ∈ [+0.22, +0.31]
    //       β=2.00  → r_ad = 0        (D collides with A)
    //       β=2.24  → r_ad ∈ [−0.19, −0.14]
    //     → r_ad observed range ≈ [−0.20, +0.35].
    //
    // Invalidation is D-anchored (tight 2% buffer past D), NOT X-anchored,
    // because D is essentially co-located with A — breaking through D
    // kills the Reciprocal AB=CD + 50%-of-BC PRZ confluence immediately.
    HarmonicSpec {
        name: "five_zero",
        ab: RatioRange::new(1.13, 1.618),
        bc: RatioRange::new(1.618, 2.24),
        cd: RatioRange::new(0.45, 0.55),  // ~0.50 of BC
        ad: RatioRange::new(-0.25, 0.35), // derived analytically (see above)
        // Reciprocal AB=CD — Carney's defining measurement. Range
        // [0.90, 1.10] = ±10% of exact equality. Tight on purpose: a
        // 20%-off CD (like a 0.80 cross ratio) fails Carney's
        // "equivalent length" qualifier even though both `bc` and `cd`
        // pass their independent bounds.
        cd_over_ab: Some(RatioRange::new(0.90, 1.10)),
        extension: true,
    },
    // ── AB=CD (Classic, Scott Carney / Larry Pesavento) ──────────────
    // Ref: https://harmonictrader.com/harmonic-patterns/abcd-pattern/
    //
    // 4-point pattern (A→B→C→D). We embed it in the XABCD container
    // with intentionally loose XA/AD bounds so the earlier pivot (X)
    // acts only as a structural anchor rather than a ratio constraint.
    //
    // Carney's invariants for the classic AB=CD:
    //   * C retracement 0.382–0.886 of AB   (→ r_bc ∈ [0.382, 0.886])
    //   * BC projection 1.13–2.618          (→ r_cd ∈ [1.13, 2.618])
    //   * CD = AB (price equality)          (→ r_cd_over_ab ≈ 1.0)
    //
    // Without the cd_over_ab cross-ratio, the independent r_bc / r_cd
    // checks would accept `(0.382, 1.13)` which gives CD/AB ≈ 0.43 —
    // a valid XABCD geometry but *not* an AB=CD pattern. The cross
    // check [0.85, 1.15] enforces Carney's equality rule with 15%
    // price noise tolerance either side.
    HarmonicSpec {
        name: "ab_cd",
        ab: RatioRange::new(0.20, 3.00),   // X unconstrained (4-point pattern)
        bc: RatioRange::new(0.382, 0.886), // C retracement of AB
        cd: RatioRange::new(1.13, 2.618),  // BC projection; reciprocal of r_bc
        ad: RatioRange::new(0.20, 5.00),   // AD unconstrained
        cd_over_ab: Some(RatioRange::new(0.85, 1.15)), // CD ≈ AB (±15%)
        extension: true,
    },
    // ── Alternate AB=CD (Scott Carney) ───────────────────────────────
    // Ref: https://harmonictrader.com/harmonic-patterns/alternate-abcd-pattern/
    //
    // Carney: "multiply the AB leg by either 1.27 or 1.618 and project
    // that distance from point C. This calculation should converge
    // with a Fibonacci projection (usually a 1.618 or 2.24) of the BC
    // leg." Used when classic AB=CD equality is "blown out".
    //
    // Cross-ratio invariant:
    //   CD = AB × 1.27   → cd_over_ab ≈ 1.27
    //   CD = AB × 1.618  → cd_over_ab ≈ 1.618
    // Union range [1.15, 1.75] with 10–15% noise either side covers
    // both legs without bleeding into classic AB=CD (cd_over_ab ≈ 1.0)
    // or full-extension patterns (1.8+).
    //
    // r_cd = CD/BC derivation:
    //   r_cd = cd_over_ab / r_bc
    //   r_bc ∈ [0.382, 0.886], cd_over_ab ∈ [1.13, 1.75]
    //     lo ≈ 1.13 / 0.886 = 1.275
    //     hi ≈ 1.75 / 0.382 = 4.58
    //   Practical band [1.27, 4.236] — upper bound from Carney's
    //   1.618 / 0.382 = 4.236 edge case.
    HarmonicSpec {
        name: "alt_ab_cd",
        ab: RatioRange::new(0.20, 3.00),
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(1.27, 4.236), // widened from 3.618 to cover 1.618/0.382
        ad: RatioRange::new(0.20, 5.00),
        cd_over_ab: Some(RatioRange::new(1.15, 1.75)), // 1.27 and 1.618 band
        extension: true,
    },
    // ── Three Drives ────────────────────────────────────────────────
    // Mapped to XABCD: three successive drives with equal extensions.
    // Drive 2 = 1.272–1.618 of correction 1, Drive 3 = 1.272–1.618 of
    // correction 2. Corrections retrace 0.618–0.786.
    HarmonicSpec {
        name: "three_drives",
        ab: RatioRange::new(0.55, 0.82),  // correction 1: 0.618–0.786
        bc: RatioRange::new(1.13, 1.618), // drive 2 extension
        cd: RatioRange::new(0.55, 0.82),  // correction 2: 0.618–0.786
        ad: RatioRange::new(1.13, 1.80),  // drive 3 extension total
        cd_over_ab: None,
        extension: true,
    },
];

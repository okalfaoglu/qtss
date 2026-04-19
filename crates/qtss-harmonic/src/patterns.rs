//! Harmonic pattern catalog.
//!
//! Each entry encodes the four canonical ratio ranges for one harmonic.
//! Ranges are deliberately a touch wider than the strictest textbook
//! values so realistic noise on live data still matches; HarmonicConfig
//! exposes a global slack on top for further loosening.
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
    HarmonicSpec {
        name: "gartley",
        ab: RatioRange::new(0.55, 0.65),  // ~0.618
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(1.13, 1.618),
        ad: RatioRange::new(0.74, 0.82),  // ~0.786
        extension: false,
    },
    HarmonicSpec {
        name: "bat",
        ab: RatioRange::new(0.382, 0.50),
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(1.618, 2.618),
        ad: RatioRange::new(0.84, 0.92),  // ~0.886
        extension: false,
    },
    HarmonicSpec {
        name: "butterfly",
        ab: RatioRange::new(0.74, 0.82),  // ~0.786
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(1.618, 2.24),
        ad: RatioRange::new(1.20, 1.65),  // 1.27..1.618 extension
        extension: true,
    },
    HarmonicSpec {
        name: "crab",
        ab: RatioRange::new(0.382, 0.618),
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(2.24, 3.618),
        ad: RatioRange::new(1.55, 1.70),  // ~1.618 extension
        extension: true,
    },
    // ── Deep Crab (Scott Carney) ─────────────────────────────────────
    // Like Crab but AB retraces deeper (0.886 of XA).
    HarmonicSpec {
        name: "deep_crab",
        ab: RatioRange::new(0.84, 0.92),  // ~0.886
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(2.0, 3.618),
        ad: RatioRange::new(1.55, 1.70),  // ~1.618
        extension: true,
    },
    // ── Shark (Scott Carney, 2011) ───────────────────────────────────
    // 0-X-A-B-C structure mapped to XABCD. Distinctive: AD > 1.0,
    // uses 0.886 and 1.13 pivots. Also called "5-0 precursor".
    HarmonicSpec {
        name: "shark",
        ab: RatioRange::new(0.382, 0.618),
        bc: RatioRange::new(1.13, 1.618),
        cd: RatioRange::new(1.618, 2.24),
        ad: RatioRange::new(0.84, 1.13),  // 0.886–1.13
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
        extension: false,
    },
    // ── Alt Bat (Scott Carney) ───────────────────────────────────────
    // Variation of Bat with deeper AB (0.382) and AD at 1.13.
    HarmonicSpec {
        name: "alt_bat",
        ab: RatioRange::new(0.33, 0.44),  // ~0.382
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(2.0, 3.618),
        ad: RatioRange::new(1.08, 1.18),  // ~1.13
        extension: true,
    },
    // ── 5-0 (Scott Carney, Harmonic Trading Vol. 2) ─────────────────
    // Reference: https://harmonictrader.com/harmonic-patterns/5-0/
    //
    // Structure: 0-X-A-B-C-D (B is an EXTENSION of XA — NOT a retrace).
    //   * r_ab = AB/XA ∈ [1.13, 1.618]  (XA projection that defines B,
    //     Carney: "must not exceed 1.618")
    //   * r_bc = BC/AB ∈ [1.618, 2.24]  (strict — the defining band)
    //   * r_cd = CD/BC ≈ 0.50           (D at 50% retrace of BC)
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
        extension: true,
    },
    // ── AB=CD (Classic, Scott Carney / Larry Pesavento) ──────────────
    // 4-point pattern (A→B→C→D). We embed it in the XABCD container
    // with intentionally loose XA/AD bounds so the earlier pivot (X)
    // acts only as a structural anchor rather than a ratio constraint.
    // Invariants enforced:
    //   * BC retraces 0.382–0.886 of AB (r_bc)
    //   * CD ≈ AB (price equality) → CD/BC ≈ 1/r_bc ∈ [1.13, 2.0]
    // Alt AB=CD (below) takes the r_cd > 2.0 branch.
    HarmonicSpec {
        name: "ab_cd",
        ab: RatioRange::new(0.20, 3.00),   // X unconstrained
        bc: RatioRange::new(0.382, 0.886), // BC retracement of AB
        cd: RatioRange::new(1.13, 2.00),   // CD ≈ AB equality region
        ad: RatioRange::new(0.20, 5.00),   // AD unconstrained
        extension: true,
    },
    // ── Alternate AB=CD (Scott Carney, Harmonic Trading Vol. 2) ──────
    // CD extends beyond classic equality: CD = 1.272× or 1.618× of AB.
    // Separation from classic AB=CD is made via the r_cd band:
    // r_cd = CD/BC = (1.272..1.618) / (0.618..0.786) ≈ [2.0, 2.618].
    HarmonicSpec {
        name: "alt_ab_cd",
        ab: RatioRange::new(0.20, 3.00),
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(2.00, 2.618),
        ad: RatioRange::new(0.20, 5.00),
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
        extension: true,
    },
];

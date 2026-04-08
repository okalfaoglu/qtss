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
    pub ad: RatioRange, // AD / XA
}

pub const PATTERNS: &[HarmonicSpec] = &[
    HarmonicSpec {
        name: "gartley",
        ab: RatioRange::new(0.55, 0.65),  // ~0.618
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(1.13, 1.618),
        ad: RatioRange::new(0.74, 0.82),  // ~0.786
    },
    HarmonicSpec {
        name: "bat",
        ab: RatioRange::new(0.382, 0.50),
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(1.618, 2.618),
        ad: RatioRange::new(0.84, 0.92),  // ~0.886
    },
    HarmonicSpec {
        name: "butterfly",
        ab: RatioRange::new(0.74, 0.82),  // ~0.786
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(1.618, 2.24),
        ad: RatioRange::new(1.20, 1.65),  // 1.27..1.618 extension
    },
    HarmonicSpec {
        name: "crab",
        ab: RatioRange::new(0.382, 0.618),
        bc: RatioRange::new(0.382, 0.886),
        cd: RatioRange::new(2.24, 3.618),
        ad: RatioRange::new(1.55, 1.70),  // ~1.618 extension
    },
];

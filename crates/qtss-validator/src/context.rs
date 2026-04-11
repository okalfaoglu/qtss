//! Side inputs supplied to the validator alongside the candidate
//! detection. Channels read whichever fields they care about; missing
//! data simply means a channel returns `None` (no opinion) and drops
//! out of the confidence blend.

use qtss_domain::v2::detection::{Detection, PatternKind};
use qtss_domain::v2::regime::RegimeKind;
use qtss_domain::v2::timeframe::Timeframe;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct ValidationContext {
    /// Detections produced on higher timeframes within the same recent
    /// window — used by the multi-timeframe channel to look for an
    /// agreeing higher-TF structure.
    pub higher_tf_detections: Vec<Detection>,
    /// Historical hit-rate lookup, keyed by `pattern_key()` (see below).
    /// Caller (qtss-storage / qtss-reporting) populates this; the
    /// validator never queries the database directly so this crate stays
    /// pure (CLAUDE.md asset-class agnostic core principle).
    pub hit_rates: HashMap<String, HitRateStat>,
    /// Multi-TF regime confluence (Faz 11). Populated from
    /// `regime_snapshots` by the orchestrator; `None` = not computed yet.
    pub multi_tf_regime: Option<MultiTfRegimeContext>,
}

/// Summary of multi-timeframe regime confluence, injected by the caller.
#[derive(Debug, Clone)]
pub struct MultiTfRegimeContext {
    pub dominant_regime: RegimeKind,
    pub confluence_score: f64,
    pub is_transitioning: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct HitRateStat {
    /// Number of historical occurrences this stat is based on.
    pub samples: u32,
    /// Fraction of those that reached their first target before stop.
    pub hit_rate: f32,
}

/// Stable lookup key combining the pattern family + timeframe so the
/// hit-rate map can be sliced however the reporting layer prefers.
pub fn pattern_key(det: &Detection) -> String {
    let family = match &det.kind {
        PatternKind::Elliott(s) => format!("elliott:{s}"),
        PatternKind::Harmonic(s) => format!("harmonic:{s}"),
        PatternKind::Classical(s) => format!("classical:{s}"),
        PatternKind::Wyckoff(s) => format!("wyckoff:{s}"),
        PatternKind::Range(s) => format!("range:{s}"),
        PatternKind::Custom(s) => format!("custom:{s}"),
    };
    format!("{family}@{:?}", det.timeframe)
}

/// True if `candidate` is on a strictly higher timeframe than `base`.
/// Lookup uses `Timeframe::seconds()` so adding a new variant doesn't
/// require touching this file (CLAUDE.md rule #1).
pub fn is_higher_timeframe(candidate: Timeframe, base: Timeframe) -> bool {
    candidate.seconds() > base.seconds()
}

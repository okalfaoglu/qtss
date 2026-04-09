//! `FormationDetector` — common interface every Elliott formation impls.
//!
//! The aggregator (`ElliottDetectorSet`) holds a `Vec<Box<dyn FormationDetector>>`
//! and dispatches each pivot snapshot to every entry. Adding a new
//! formation = appending one struct to the registry, no central match
//! arm to edit (CLAUDE.md #1).

use qtss_domain::v2::detection::Detection;
use qtss_domain::v2::instrument::Instrument;
use qtss_domain::v2::pivot::PivotTree;
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;

pub trait FormationDetector: Send + Sync {
    /// Stable PatternKind subkind prefix this formation emits — e.g.
    /// `"ending_diagonal"`, `"zigzag"`. Used by metrics and dedup.
    fn name(&self) -> &'static str;

    /// Scan the latest pivots and return zero or more detections. A
    /// single formation can fire multiple variants per pass (e.g. a
    /// zigzag detector that finds both bull and bear cases on different
    /// pivot tails) — return all of them.
    fn detect(
        &self,
        tree: &PivotTree,
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection>;
}

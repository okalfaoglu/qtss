//! `ElliottDetectorSet` — registry of every Elliott formation detector.
//!
//! The orchestrator wires a single `ElliottDetectorSet` per symbol/tf
//! pass and calls `detect_all`. The set holds a `Vec<Box<dyn
//! FormationDetector>>`, so adding a new formation is one `register`
//! call — no central match arm to edit (CLAUDE.md #1).
//!
//! Each formation is gated by its own enable flag in the config struct
//! resolved by the caller, so an operator can disable noisy ones
//! without restarting the worker.

use crate::combination::CombinationDetector;
use crate::config::ElliottConfig;
use crate::detector::ImpulseDetector;
use crate::diagonal::{DiagonalDetector, DiagonalKind};
use crate::error::ElliottResult;
use crate::extended_impulse::ExtendedImpulseDetector;
use crate::flat::FlatDetector;
use crate::formation::FormationDetector;
use crate::forming::FormingImpulseDetector;
use crate::nascent::NascentImpulseDetector;
use crate::triangle::TriangleDetector;
use crate::truncated_fifth::TruncatedFifthDetector;
use crate::zigzag::ZigzagDetector;
use qtss_domain::v2::detection::Detection;
use qtss_domain::v2::instrument::Instrument;
use qtss_domain::v2::pivot::PivotTree;
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;

/// Per-formation enable toggles. Resolved from `system_config` by the
/// orchestrator and passed in. Defaults are conservative — only the
/// well-tested impulse runs unless an operator opts in.
#[derive(Debug, Clone)]
pub struct ElliottFormationToggles {
    pub impulse: bool,
    /// Faz 15 — early 4-pivot impulse detection (nascent wave 3).
    pub nascent_impulse: bool,
    /// Faz 14.A13 — 5-pivot forming impulse (wave 5 in progress).
    pub forming_impulse: bool,
    pub leading_diagonal: bool,
    pub ending_diagonal: bool,
    pub zigzag: bool,
    pub flat: bool,
    pub triangle: bool,
    pub extended_impulse: bool,
    pub truncated_fifth: bool,
    pub combination: bool,
    /// LuxAlgo motive + corrective detector (5-wave + 3-wave patterns).
    pub luxalgo: bool,
}

impl ElliottFormationToggles {
    pub fn defaults() -> Self {
        Self {
            impulse: true,
            // Defaults-on — low-cost, high-value early signal. Operators
            // can disable via `detection.elliott.nascent_impulse.enabled`.
            nascent_impulse: true,
            forming_impulse: true,
            leading_diagonal: true,
            ending_diagonal: true,
            zigzag: true,
            flat: true,
            triangle: true,
            extended_impulse: true,
            truncated_fifth: true,
            combination: true,
            luxalgo: true,
        }
    }
}

/// Adapter that wraps the legacy `ImpulseDetector` (which still returns
/// `Option<Detection>`) into the `FormationDetector` interface.
struct ImpulseAdapter(ImpulseDetector);

impl FormationDetector for ImpulseAdapter {
    fn name(&self) -> &'static str {
        "impulse"
    }
    fn detect(
        &self,
        tree: &PivotTree,
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        self.0.detect_all(tree, instrument, timeframe, regime)
    }
}

pub struct ElliottDetectorSet {
    formations: Vec<Box<dyn FormationDetector>>,
}

impl ElliottDetectorSet {
    /// Build the set from a single base config + per-formation toggles.
    /// Each enabled formation is constructed once; failures are logged
    /// upstream so we just propagate the first error.
    pub fn new(
        base: ElliottConfig,
        toggles: &ElliottFormationToggles,
    ) -> ElliottResult<Self> {
        let mut formations: Vec<Box<dyn FormationDetector>> = Vec::new();

        if toggles.impulse {
            formations.push(Box::new(ImpulseAdapter(ImpulseDetector::new(base.clone())?)));
        }
        if toggles.nascent_impulse {
            formations.push(Box::new(NascentImpulseDetector::new(base.clone())?));
        }
        if toggles.forming_impulse {
            formations.push(Box::new(FormingImpulseDetector::new(base.clone())?));
        }
        if toggles.leading_diagonal {
            formations.push(Box::new(DiagonalDetector::new(
                base.clone(),
                DiagonalKind::Leading,
            )?));
        }
        if toggles.ending_diagonal {
            formations.push(Box::new(DiagonalDetector::new(
                base.clone(),
                DiagonalKind::Ending,
            )?));
        }
        if toggles.zigzag {
            formations.push(Box::new(ZigzagDetector::new(base.clone())?));
        }
        if toggles.flat {
            formations.push(Box::new(FlatDetector::new(base.clone())?));
        }
        if toggles.triangle {
            formations.push(Box::new(TriangleDetector::new(base.clone())?));
        }
        if toggles.extended_impulse {
            formations.push(Box::new(ExtendedImpulseDetector::new(base.clone())?));
        }
        if toggles.truncated_fifth {
            formations.push(Box::new(TruncatedFifthDetector::new(base.clone())?));
        }
        if toggles.combination {
            formations.push(Box::new(CombinationDetector::new(base.clone())?));
        }
        if toggles.luxalgo {
            formations.push(Box::new(crate::luxalgo_detector::LuxAlgoDetector::new(base)));
        }

        Ok(Self { formations })
    }

    /// Run every registered formation against the same pivot snapshot
    /// and return all detections in registration order.
    /// Run every registered formation against the same pivot snapshot
    /// and return all detections in registration order. Each Elliott
    /// detection is automatically tagged with its wave degree label
    /// based on the timeframe.
    pub fn detect_all(
        &self,
        tree: &PivotTree,
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        let mut out = Vec::new();
        for f in &self.formations {
            for d in f.detect(tree, instrument, timeframe, regime) {
                out.push(d.with_degree());
            }
        }
        out
    }

    /// Number of registered formations — useful for diagnostics.
    pub fn len(&self) -> usize {
        self.formations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.formations.is_empty()
    }
}

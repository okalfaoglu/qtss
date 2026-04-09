//! Combination correction (W-X-Y, W-X-Y-X-Z).
//!
//! Combinations chain two or three simpler corrections (zigzag, flat or
//! triangle) via connecting X waves. Detecting them properly requires
//! *cross-scan state*: we have to remember that a prior pivot tail was
//! a valid zigzag/flat, then watch for an X-wave + a new correction
//! that completes the combination.
//!
//! That history-aware detection lives in the orchestrator + repository
//! layer, not in a single pivot scan, so this file deliberately ships
//! as a thin scaffold. The real W-X-Y assembler will:
//!
//!   1. Read recent `family='elliott'` detections from the repository.
//!   2. Find a pair `[(W: zigzag|flat), (Y: zigzag|flat|triangle)]`
//!      separated by exactly one corrective leg (the X wave).
//!   3. Emit `combination_w_x_y_<dir>` (and `..._w_x_y_x_z_<dir>` for
//!      the rarer triple combination).
//!
//! Until that wiring lands the detector returns no detections — but it
//! is registered in the aggregator so the slot exists and the migration
//! can already seed its toggle / threshold keys.

use crate::config::ElliottConfig;
use crate::error::ElliottResult;
use crate::formation::FormationDetector;
use qtss_domain::v2::detection::Detection;
use qtss_domain::v2::instrument::Instrument;
use qtss_domain::v2::pivot::PivotTree;
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;

pub struct CombinationDetector {
    #[allow(dead_code)]
    config: ElliottConfig,
}

impl CombinationDetector {
    pub fn new(config: ElliottConfig) -> ElliottResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }
}

impl FormationDetector for CombinationDetector {
    fn name(&self) -> &'static str {
        "combination"
    }

    fn detect(
        &self,
        _tree: &PivotTree,
        _instrument: &Instrument,
        _timeframe: Timeframe,
        _regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        // Intentional no-op until the cross-scan W-X-Y assembler lands.
        Vec::new()
    }
}

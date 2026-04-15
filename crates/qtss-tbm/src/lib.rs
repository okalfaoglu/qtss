//! Top/Bottom Mining — multi-pillar reversal scoring system.

pub mod config;
pub mod pillar;
pub mod momentum;
pub mod volume;
pub mod structure;
pub mod onchain;
pub mod scorer;
pub mod setup;
pub mod mtf;

pub use config::{TbmConfig, TbmEffortResultTuning, TbmMtfTuning, TbmPillarWeights, TbmSetupTuning};
pub use pillar::{PillarScore, PillarKind};
pub use scorer::{TbmScore, score_tbm};
pub use setup::{TbmSetup, SetupDirection, detect_setups};
pub use mtf::{mtf_confirm, MtfResult, TfScore, Timeframe};

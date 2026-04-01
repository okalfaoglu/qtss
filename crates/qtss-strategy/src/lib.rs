//! Strategy layer: reads `onchain_signal_scores` + optional bars, produces [`qtss_domain::orders::OrderIntent`] via [`qtss_execution::ExecutionGateway`].
//!
//! Worker dry runner: `strategy_runner::spawn_if_enabled` — `docs/QTSS_CURSOR_DEV_GUIDE.md` §4 ADIM 7.

pub mod arb_funding;
pub mod conflict_policy;
pub mod context;
pub mod copy_trade;
mod paper_recording_dry_gateway;
pub mod risk;
pub mod signal_filter;
pub mod whale_momentum;

pub use paper_recording_dry_gateway::{paper_ledger_target_from_db, PaperRecordingDryGateway};

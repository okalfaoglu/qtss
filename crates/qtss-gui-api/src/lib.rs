//! qtss-gui-api — wire DTOs for GUI v2 (plan §9A).
//!
//! Single source of truth for the JSON contract between the Rust API
//! layer and the React shell. Each panel listed in plan §9A.3 gets
//! its own typed module here so the frontend can import a generated
//! TypeScript bundle (via `ts-rs` or `serde-reflection`) without
//! ever touching engine internals.
//!
//! ## Why a dedicated crate
//!
//! - **Layer hygiene (CLAUDE.md #3):** the API crate already pulls in
//!   storage, OAuth, rate-limiting and a dozen v1 routes. GUI v2 wire
//!   types should not have to live next to that — they describe a
//!   *contract*, not a transport.
//! - **Reusable in tooling:** the same structs are useful in
//!   `qtss-reporting` (PDF export of a dashboard snapshot), in
//!   integration tests, and in the backtest visualiser.
//! - **No engine leakage:** GUI types are deliberately *narrower*
//!   than engine types — e.g. the dashboard does not need every
//!   field on `Position`, only what the cards render.

pub mod ai_decisions;
pub mod blotter;
pub mod chart;
pub mod config_editor;
pub mod dashboard;
pub mod montecarlo;
pub mod regime;
pub mod risk;
pub mod scenarios;
pub mod strategy_manager;

pub use dashboard::{
    DashboardSnapshot, EquityPoint, OpenPositionView, PortfolioCard, RiskCard,
};
pub use chart::{
    build_renko, CandleBar, ChartWorkspace, DetectionOverlay, OpenOrderOverlay, RenkoBrick,
};
pub use ai_decisions::{
    payload_preview, AiDecisionEntry, AiDecisionStatus, AiDecisionsView, PAYLOAD_PREVIEW_MAX_LEN,
};
pub use blotter::{merge_blotter, BlotterEntry, BlotterFeed};
pub use config_editor::{group_config_entries, ConfigEditorView, ConfigEntry, ConfigGroup};
pub use montecarlo::{build_montecarlo_fan, FanBand, MonteCarloFan};
pub use regime::{RegimeHud, RegimePoint, RegimeView};
pub use risk::{build_risk_hud, RiskGauge, RiskHud};
pub use scenarios::{build_volatility_tree, ScenarioNode, ScenarioTree, TargetBand};
pub use strategy_manager::{
    StrategyCard, StrategyManagerView, StrategyParam, StrategyStatus,
};

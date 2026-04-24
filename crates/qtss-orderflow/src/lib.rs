//! qtss-orderflow — tape/flow-based event detectors (PR-12C kickoff).
//!
//! Three families this release:
//!
//!   * **LiquidationCluster** — rolling-window liquidation density
//!     (count + notional). Excess cluster = forced-flow inflection,
//!     usually marks the bottom of a dump / top of a rip.
//!   * **BlockTrade** — single liquidation or aggtrade with notional
//!     above a whale threshold (e.g. $500K BTC). Individual print
//!     visible on the tape.
//!   * **CVDDivergence** — Cumulative Volume Delta flat/declining
//!     while price rises (bearish divergence) or vice versa.
//!
//! PR-15 later expands to footprint / absorption / sweep detection
//! (kept out of this MVP to avoid overloading the Confluence engine
//! with noisy signals before the weighting is tuned).
//!
//! Input shapes match what `qtss-onchain`'s Binance WS aggregators
//! persist under `data_snapshots.response_json`:
//!   * liquidations → `{count, events: [{qty, side, price, ts_ms}...]}`
//!   * cvd          → `{count, buckets: [{cvd, delta, trades,
//!                                        buy_qty, sell_qty,
//!                                        bucket_ts_ms}...]}`

mod config;
mod deep;
mod event;
mod events;

pub use config::OrderFlowConfig;
pub use deep::{detect_absorption, detect_footprint_imbalance, detect_sweep};
pub use event::{OrderFlowEvent, OrderFlowEventKind};
pub use events::{detect_block_trades, detect_cvd_divergence, detect_liquidation_cluster};

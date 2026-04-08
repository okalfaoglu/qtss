//! qtss-execution-v2 — translates ApprovedIntent into venue orders.
//!
//! Pipeline (see plan §7B):
//!
//!   ApprovedIntent --(builder)--> OrderBracket
//!                  --(router)---> ExecutionAdapter --> venue
//!
//! ## Layers
//!
//! - **builder** — pure splitter: ApprovedIntent → entry + stop +
//!   take-profit OrderRequests with reduce_only flags wired up.
//! - **adapter** — `ExecutionAdapter` trait every venue/mode implements.
//!   Async; returns `OrderAck` with fills.
//! - **sim** — paper-fill adapter for `dry` and `backtest` modes.
//!   Slippage and taker fee live in `SimConfig` (CLAUDE.md rule #2).
//! - **router** — picks an adapter by `ExecutionMode`. Adapters live in
//!   a `HashMap` keyed by mode so adding a new venue is one register
//!   call (CLAUDE.md rule #1).
//!
//! Live broker adapters (Binance spot/futures, Bybit, …) plug in via
//! the same `ExecutionAdapter` trait in their own crates.

mod adapter;
mod builder;
mod error;
mod router;
mod sim;

#[cfg(test)]
mod tests;

pub use adapter::{ExecutionAdapter, Fill, OrderAck, OrderHandle, OrderStatus};
pub use builder::{split_intent, OrderBracket};
pub use error::{ExecutionError, ExecutionResult};
pub use router::{ExecutionRouter, RoutedAcks};
pub use sim::{SimAdapter, SimConfig};

//! qtss-runtime — `live` / `dry` / `backtest` execution context.
//!
//! Plan §10 Faz 4B requires that **mode is a runtime context, not a
//! feature flag**: every worker, strategy, risk gate and execution
//! adapter knows at construction time whether it is running with real
//! money, paper money on live data, or paper money on historical data.
//! That single piece of information drives a handful of cross-cutting
//! decisions:
//!
//! | Question                          | live  | dry   | backtest |
//! |-----------------------------------|-------|-------|----------|
//! | Hits real broker REST?            | yes   | no    | no       |
//! | Subscribes to live market WS?     | yes   | yes   | no       |
//! | Writes to live tables?            | yes   | no    | no       |
//! | Writes to `dry_*` mirror tables?  | no    | yes   | no       |
//! | Writes to backtest tables?        | no    | no    | yes      |
//! | Default kill-switch on boot?      | off   | off   | off      |
//! | Allowed to call AI providers?     | yes   | yes   | no(¹)    |
//! | Allowed to send notifications?    | yes   | yes   | no       |
//!
//! (¹) Backtests use the recorded fixture advisor; never the live
//!     network LLM. The runtime enforces this with a typed policy.
//!
//! ## Design (CLAUDE.md)
//!
//! - **Single dispatch (#1):** every per-mode decision lives behind a
//!   trait method on [`RuntimeMode`]. The rest of the codebase asks
//!   `ctx.policy().writes_live_tables()` instead of doing
//!   `match mode { ... }` at every site.
//! - **No hardcoded values (#2):** [`RuntimeContext`] does not know
//!   about timeouts, slippage or table names. It carries an opaque
//!   `RuntimeId`, a `RunMode`, and a [`StorageNamespace`] populated
//!   from `qtss_config` by the bootstrap layer.
//! - **No layer leakage (#3):** this crate depends only on
//!   `qtss-domain` and `qtss-execution-v2` (for the `ExecutionMode`
//!   alias). It explicitly does *not* know about strategies, risk,
//!   storage backends, or notification transports — those crates
//!   *consume* the context.

mod context;
mod error;
mod policy;
mod storage_ns;

pub use context::{RuntimeContext, RuntimeId};
pub use error::{RuntimeError, RuntimeResult};
pub use policy::{RuntimeMode, RuntimePolicy};
pub use storage_ns::StorageNamespace;

pub use qtss_domain::execution::ExecutionMode as RunMode;

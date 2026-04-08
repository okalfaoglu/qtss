//! qtss-fees — venue-agnostic commission model.
//!
//! Centralises every fee calculation in QTSS so execution adapters,
//! the simulator, the backtest engine and the portfolio engine all
//! agree on what a trade actually costs.
//!
//! ## Design (CLAUDE.md rules #1 and #2)
//!
//! - **No hardcoded numbers.** A [`FeeBook`] is built from config rows
//!   (loaded out-of-crate by `qtss-config`) and looked up by venue +
//!   symbol. The crate ships *no* default rates.
//! - **No scattered match arms.** Liquidity → schedule field is a
//!   trivial dispatch on the [`Liquidity`] enum; venue/symbol lookup
//!   is a `HashMap`, so adding a venue is one `register` call.
//! - **Trait boundary.** Adapters depend on [`FeeModel`], not on the
//!   concrete book — the simulator and the live Binance adapter share
//!   the same trait, the backtest engine can swap in a historical
//!   schedule without touching execution code.

mod book;
mod error;
mod model;
mod schedule;

pub use book::FeeBook;
pub use error::{FeeError, FeeResult};
pub use model::{FeeModel, FeeQuote, TradeContext};
pub use schedule::{FeeSchedule, Liquidity};

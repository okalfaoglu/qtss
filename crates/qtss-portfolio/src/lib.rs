//! qtss-portfolio — per-instrument position tracking + aggregate
//! equity / day-pnl / peak-equity book-keeping. Consumed by the risk
//! engine via the `AccountState` snapshot.

mod engine;
mod error;
mod position;

pub use engine::{PortfolioConfig, PortfolioEngine};
pub use error::{PortfolioError, PortfolioResult};
pub use position::Position;

//! qtss-config — typed, scoped, audited configuration registry.
//!
//! See `docs/QTSS_V2_ARCHITECTURE_PLAN.md` §7C for the design and
//! `migrations/0014_qtss_v2_config.sql` for the schema.
//!
//! ## Usage
//! ```ignore
//! use qtss_config::{ConfigStore, PgConfigStore, ResolveCtx, Scope};
//!
//! let store = PgConfigStore::new(pool);
//! let ctx = ResolveCtx::default().with_venue("binance");
//! let max_dd: f64 = store.get("risk.account.max_drawdown_pct", &ctx).await?;
//! ```
//!
//! ## Design rules (see CLAUDE.md)
//! * No hardcoded constants — every parameter resolved through this crate.
//! * No scattered if/else — scope resolution uses an ordered list of scopes.
//! * Detector / Strategy / Adapter layers depend on this crate, not on `.env`.

#![forbid(unsafe_code)]

mod error;
mod scope;
mod store;
mod types;

#[cfg(test)]
mod tests;

pub use error::{ConfigError, ConfigResult};
pub use scope::{ResolveCtx, Scope, ScopeType};
pub use store::{ConfigStore, MemoryConfigStore, PgConfigStore};
pub use types::{ConfigSchemaRow, ConfigValueRow, SetOptions, ValueType};

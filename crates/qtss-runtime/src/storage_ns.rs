//! Storage namespace selection.
//!
//! Different modes write into different table sets. The runtime
//! carries a `StorageNamespace` so query builders can prefix table
//! names without each call site re-deriving the rule. The actual
//! table name strings are *not* hardcoded here — bootstrap reads
//! them from `qtss_config` and constructs the namespace.

use crate::RunMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageNamespace {
    /// Schema or prefix for live production tables (e.g. `public`).
    pub live_schema: String,
    /// Schema or prefix for paper/dry mirror tables (e.g. `dry`).
    pub dry_schema: String,
    /// Schema or prefix for backtest tables (e.g. `bt_<run_id>`).
    pub backtest_schema: String,
}

impl StorageNamespace {
    /// Returns the schema name in effect for the given mode. The
    /// caller decides whether to qualify a table name with it or to
    /// pass it to `SET search_path`.
    pub fn for_mode(&self, mode: RunMode) -> &str {
        match mode {
            RunMode::Live => &self.live_schema,
            RunMode::Dry => &self.dry_schema,
            RunMode::Backtest => &self.backtest_schema,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns() -> StorageNamespace {
        StorageNamespace {
            live_schema: "public".into(),
            dry_schema: "dry".into(),
            backtest_schema: "bt_run42".into(),
        }
    }

    #[test]
    fn each_mode_picks_its_own_schema() {
        let n = ns();
        assert_eq!(n.for_mode(RunMode::Live), "public");
        assert_eq!(n.for_mode(RunMode::Dry), "dry");
        assert_eq!(n.for_mode(RunMode::Backtest), "bt_run42");
    }
}

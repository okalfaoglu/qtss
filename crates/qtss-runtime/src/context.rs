//! `RuntimeContext` — the value every worker, strategy and adapter
//! receives at construction.

use crate::policy::{RuntimeMode, RuntimePolicy};
use crate::storage_ns::StorageNamespace;
use crate::{RunMode, RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};

/// Stable identifier for one runtime instance. For `Live` and `Dry`
/// this is typically the hostname; for `Backtest` it embeds the run
/// id so concurrent backtests stay isolated in storage and audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeId(pub String);

#[derive(Debug, Clone)]
pub struct RuntimeContext {
    id: RuntimeId,
    mode: RunMode,
    storage: StorageNamespace,
}

impl RuntimeContext {
    pub fn new(id: RuntimeId, mode: RunMode, storage: StorageNamespace) -> Self {
        Self { id, mode, storage }
    }

    pub fn id(&self) -> &RuntimeId {
        &self.id
    }

    pub fn mode(&self) -> RunMode {
        self.mode
    }

    pub fn storage(&self) -> &StorageNamespace {
        &self.storage
    }

    pub fn schema(&self) -> &str {
        self.storage.for_mode(self.mode)
    }

    pub fn policy(&self) -> RuntimePolicy {
        self.mode.policy()
    }

    /// Convenience: gate `op` against the policy in one call. Used by
    /// storage / notify / execution layers as the canonical "may I?".
    pub fn require(&self, op: &'static str, allowed_check: fn(&RuntimePolicy) -> bool) -> RuntimeResult<()> {
        let policy = self.policy();
        if allowed_check(&policy) {
            Ok(())
        } else {
            Err(RuntimeError::NotAllowed { op, mode: self.mode })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(mode: RunMode) -> RuntimeContext {
        RuntimeContext::new(
            RuntimeId("test".into()),
            mode,
            StorageNamespace {
                live_schema: "public".into(),
                dry_schema: "dry".into(),
                backtest_schema: "bt".into(),
            },
        )
    }

    #[test]
    fn schema_follows_mode() {
        assert_eq!(ctx(RunMode::Live).schema(), "public");
        assert_eq!(ctx(RunMode::Dry).schema(), "dry");
        assert_eq!(ctx(RunMode::Backtest).schema(), "bt");
    }

    #[test]
    fn require_blocks_real_broker_in_dry() {
        let r = ctx(RunMode::Dry).require("place_order", |p| p.real_broker_calls);
        assert!(r.is_err());
    }

    #[test]
    fn require_allows_real_broker_in_live() {
        let r = ctx(RunMode::Live).require("place_order", |p| p.real_broker_calls);
        assert!(r.is_ok());
    }
}

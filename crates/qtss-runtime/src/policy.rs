//! Per-mode policy dispatch.
//!
//! Each `RunMode` maps to one [`RuntimePolicy`] struct via
//! [`RuntimeMode::policy`]. Callers ask the policy questions instead
//! of matching on the mode at every call site (CLAUDE.md rule #1).

use crate::error::{RuntimeError, RuntimeResult};
use crate::RunMode;

/// What a given run mode is allowed to do. All fields are deliberate
/// booleans rather than a bitset so the table reads at a glance and
/// adding a new dimension is one field, not a flag rename.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimePolicy {
    /// Place orders against the real broker REST API.
    pub real_broker_calls: bool,
    /// Subscribe to live market-data WebSockets.
    pub live_marketdata_subscription: bool,
    /// Persist into the production tables.
    pub writes_live_tables: bool,
    /// Persist into the dry/paper mirror tables.
    pub writes_dry_tables: bool,
    /// Persist into the backtest tables.
    pub writes_backtest_tables: bool,
    /// Allowed to call live AI/LLM providers.
    pub allows_live_ai: bool,
    /// Allowed to dispatch notifications (Telegram, webhooks, …).
    pub allows_notifications: bool,
}

impl RuntimePolicy {
    /// Guard helper used by storage / notify / execution layers. Returns
    /// `Err(NotAllowed)` instead of panicking so the caller can audit
    /// every blocked attempt instead of crashing.
    pub fn require(&self, op: &'static str, allowed: bool, mode: RunMode) -> RuntimeResult<()> {
        if allowed {
            Ok(())
        } else {
            Err(RuntimeError::NotAllowed { op, mode })
        }
    }
}

/// Trait that maps a [`RunMode`] to its [`RuntimePolicy`]. Default impl
/// covers the three known modes; future modes (e.g. `Shadow`) plug in
/// by overriding `policy_for` on a custom resolver.
pub trait RuntimeMode {
    fn policy(&self) -> RuntimePolicy;
}

impl RuntimeMode for RunMode {
    fn policy(&self) -> RuntimePolicy {
        match self {
            RunMode::Live => RuntimePolicy {
                real_broker_calls: true,
                live_marketdata_subscription: true,
                writes_live_tables: true,
                writes_dry_tables: false,
                writes_backtest_tables: false,
                allows_live_ai: true,
                allows_notifications: true,
            },
            RunMode::Dry => RuntimePolicy {
                real_broker_calls: false,
                live_marketdata_subscription: true,
                writes_live_tables: false,
                writes_dry_tables: true,
                writes_backtest_tables: false,
                allows_live_ai: true,
                allows_notifications: true,
            },
            RunMode::Backtest => RuntimePolicy {
                real_broker_calls: false,
                live_marketdata_subscription: false,
                writes_live_tables: false,
                writes_dry_tables: false,
                writes_backtest_tables: true,
                allows_live_ai: false,
                allows_notifications: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_can_touch_real_broker_others_cannot() {
        assert!(RunMode::Live.policy().real_broker_calls);
        assert!(!RunMode::Dry.policy().real_broker_calls);
        assert!(!RunMode::Backtest.policy().real_broker_calls);
    }

    #[test]
    fn backtest_blocks_live_ai_and_notifications() {
        let p = RunMode::Backtest.policy();
        assert!(!p.allows_live_ai);
        assert!(!p.allows_notifications);
        assert!(!p.live_marketdata_subscription);
    }

    #[test]
    fn dry_writes_only_to_mirror_tables() {
        let p = RunMode::Dry.policy();
        assert!(p.writes_dry_tables);
        assert!(!p.writes_live_tables);
        assert!(!p.writes_backtest_tables);
    }

    #[test]
    fn require_helper_returns_typed_error() {
        let err = RunMode::Backtest
            .policy()
            .require("send_notification", false, RunMode::Backtest)
            .unwrap_err();
        assert!(matches!(err, RuntimeError::NotAllowed { .. }));
    }
}

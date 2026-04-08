//! Scope hierarchy and resolution context.
//!
//! Resolution order (highest precedence first):
//!     instrument > strategy > venue > asset_class > global
//!
//! `ResolveCtx` carries optional dimensions from the call site; the store
//! converts it into a list of `Scope`s in priority order and queries them
//! sequentially. The first match wins.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Type discriminator for a scope row in `config_scope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeType {
    Global,
    AssetClass,
    Venue,
    Strategy,
    Instrument,
    User,
}

impl ScopeType {
    pub fn as_str(self) -> &'static str {
        match self {
            ScopeType::Global => "global",
            ScopeType::AssetClass => "asset_class",
            ScopeType::Venue => "venue",
            ScopeType::Strategy => "strategy",
            ScopeType::Instrument => "instrument",
            ScopeType::User => "user",
        }
    }
}

impl fmt::Display for ScopeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A concrete scope (type + key). `key` is empty string for global.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Scope {
    pub scope_type: ScopeType,
    pub key: String,
}

impl Scope {
    pub fn global() -> Self {
        Self {
            scope_type: ScopeType::Global,
            key: String::new(),
        }
    }

    pub fn new(scope_type: ScopeType, key: impl Into<String>) -> Self {
        Self {
            scope_type,
            key: key.into(),
        }
    }
}

/// Resolution context: which dimensions are known at call site.
///
/// All fields are optional. Builders compose them as the request flows
/// through layers (e.g. an order intake handler knows venue + instrument;
/// a strategy evaluator also knows the strategy id).
#[derive(Debug, Clone, Default)]
pub struct ResolveCtx {
    pub instrument: Option<String>,
    pub strategy: Option<String>,
    pub venue: Option<String>,
    pub asset_class: Option<String>,
    pub user: Option<String>,
}

impl ResolveCtx {
    pub fn with_instrument(mut self, v: impl Into<String>) -> Self {
        self.instrument = Some(v.into());
        self
    }

    pub fn with_strategy(mut self, v: impl Into<String>) -> Self {
        self.strategy = Some(v.into());
        self
    }

    pub fn with_venue(mut self, v: impl Into<String>) -> Self {
        self.venue = Some(v.into());
        self
    }

    pub fn with_asset_class(mut self, v: impl Into<String>) -> Self {
        self.asset_class = Some(v.into());
        self
    }

    pub fn with_user(mut self, v: impl Into<String>) -> Self {
        self.user = Some(v.into());
        self
    }

    /// Build the priority-ordered list of scopes to probe.
    /// Highest precedence first; resolution stops at first match.
    pub fn priority_chain(&self) -> Vec<Scope> {
        let candidates: [(ScopeType, &Option<String>); 5] = [
            (ScopeType::Instrument, &self.instrument),
            (ScopeType::Strategy, &self.strategy),
            (ScopeType::Venue, &self.venue),
            (ScopeType::AssetClass, &self.asset_class),
            (ScopeType::User, &self.user),
        ];

        let mut chain: Vec<Scope> = candidates
            .iter()
            .filter_map(|(ty, val)| val.as_ref().map(|v| Scope::new(*ty, v.clone())))
            .collect();

        // Global is always the last fallback.
        chain.push(Scope::global());
        chain
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;

    #[test]
    fn priority_chain_includes_global_last() {
        let ctx = ResolveCtx::default().with_venue("binance");
        let chain = ctx.priority_chain();
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].scope_type, ScopeType::Venue);
        assert_eq!(chain[0].key, "binance");
        assert_eq!(chain[1], Scope::global());
    }

    #[test]
    fn full_chain_priority_order() {
        let ctx = ResolveCtx::default()
            .with_instrument("BTCUSDT")
            .with_strategy("trend_v1")
            .with_venue("binance")
            .with_asset_class("crypto_spot")
            .with_user("u1");

        let chain = ctx.priority_chain();
        let types: Vec<ScopeType> = chain.iter().map(|s| s.scope_type).collect();
        assert_eq!(
            types,
            vec![
                ScopeType::Instrument,
                ScopeType::Strategy,
                ScopeType::Venue,
                ScopeType::AssetClass,
                ScopeType::User,
                ScopeType::Global,
            ]
        );
    }

    #[test]
    fn empty_ctx_resolves_to_global_only() {
        let chain = ResolveCtx::default().priority_chain();
        assert_eq!(chain, vec![Scope::global()]);
    }
}

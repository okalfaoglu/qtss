//! Faz 9.8.4 — Execution manager.
//!
//! Owns a dispatch table of [`ExecutionGateway`] per [`ExecutionMode`]
//! and lets the worker place the **same** `OrderIntent` on *every*
//! active mode in parallel (typical deployment runs `Dry` + `Live`
//! side by side so paper tracks the live book 1:1). CLAUDE.md #1 —
//! the dispatch is a HashMap rather than inline match chains; adding
//! a new mode is one `register` call.
//!
//! The manager is intentionally thin: it does not size orders, does
//! not decide risk — it just fans the intent out to registered
//! gateways and collects per-mode outcomes. Selector + allocator +
//! risk engine sit upstream.

use async_trait::async_trait;
use qtss_domain::execution::ExecutionMode;
use qtss_domain::orders::OrderIntent;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::gateway::{ExecutionError, ExecutionGateway};

/// Per-mode outcome of a `place()` call. Kept separate from the
/// trait surface so callers can log partial failures (dry succeeded,
/// live rejected) without short-circuiting.
#[derive(Debug)]
pub struct PlaceOutcome {
    pub mode: ExecutionMode,
    pub result: Result<Uuid, ExecutionError>,
}

impl PlaceOutcome {
    pub fn is_ok(&self) -> bool {
        self.result.is_ok()
    }
}

/// Dispatch table + fan-out. Cloneable by design so the worker
/// can hand it to spawned tasks.
#[derive(Clone, Default)]
pub struct ExecutionManager {
    gateways: HashMap<ExecutionMode, Arc<dyn ExecutionGateway>>,
}

impl ExecutionManager {
    pub fn new() -> Self {
        Self {
            gateways: HashMap::new(),
        }
    }

    pub fn register(&mut self, mode: ExecutionMode, gw: Arc<dyn ExecutionGateway>) {
        self.gateways.insert(mode, gw);
    }

    pub fn modes(&self) -> Vec<ExecutionMode> {
        self.gateways.keys().copied().collect()
    }

    pub fn gateway(&self, mode: ExecutionMode) -> Option<Arc<dyn ExecutionGateway>> {
        self.gateways.get(&mode).cloned()
    }

    pub fn len(&self) -> usize {
        self.gateways.len()
    }

    pub fn is_empty(&self) -> bool {
        self.gateways.is_empty()
    }

    /// Place the intent on a single specified mode. Errors when the
    /// mode has no gateway registered (caller bug — should have been
    /// caught at worker startup).
    pub async fn place_on(
        &self,
        mode: ExecutionMode,
        intent: OrderIntent,
    ) -> Result<Uuid, ExecutionError> {
        let gw = self.gateways.get(&mode).ok_or_else(|| {
            ExecutionError::Other(format!("no gateway registered for mode {mode:?}"))
        })?;
        gw.place(intent).await
    }

    /// Fan out the intent to every registered mode sequentially,
    /// collecting per-mode outcomes. Sequential (not parallel) so
    /// live-gateway rate limits stay deterministic; call sites that
    /// truly need concurrency spawn tasks themselves.
    pub async fn place_on_all(&self, intent: OrderIntent) -> Vec<PlaceOutcome> {
        let mut out = Vec::with_capacity(self.gateways.len());
        for (mode, gw) in &self.gateways {
            // OrderIntent isn't Clone in the domain crate; we need a
            // clone per call so each gateway sees an owned value. We
            // copy through OrderIntentClone helper below — but since
            // the domain already derives Clone, use that.
            let res = gw.place(intent.clone()).await;
            out.push(PlaceOutcome { mode: *mode, result: res });
        }
        out
    }

    pub async fn cancel_on(
        &self,
        mode: ExecutionMode,
        client_order_id: Uuid,
    ) -> Result<(), ExecutionError> {
        let gw = self.gateways.get(&mode).ok_or_else(|| {
            ExecutionError::Other(format!("no gateway registered for mode {mode:?}"))
        })?;
        gw.cancel(client_order_id).await
    }
}

/// `Send + Sync` trait-object wrapper so the manager itself can be
/// shared across tasks (e.g. stored in an `Arc<ExecutionManager>`
/// inside the worker handle).
#[async_trait]
pub trait ExecutionContext: Send + Sync {
    async fn place(&self, mode: ExecutionMode, intent: OrderIntent)
        -> Result<Uuid, ExecutionError>;
    async fn place_all(&self, intent: OrderIntent) -> Vec<PlaceOutcome>;
}

#[async_trait]
impl ExecutionContext for ExecutionManager {
    async fn place(
        &self,
        mode: ExecutionMode,
        intent: OrderIntent,
    ) -> Result<Uuid, ExecutionError> {
        self.place_on(mode, intent).await
    }
    async fn place_all(&self, intent: OrderIntent) -> Vec<PlaceOutcome> {
        self.place_on_all(intent).await
    }
}

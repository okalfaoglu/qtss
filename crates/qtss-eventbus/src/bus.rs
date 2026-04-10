//! In-process publish/subscribe bus built on `tokio::sync::broadcast`.
//!
//! Each topic gets its own broadcast channel allocated lazily on first
//! publish or subscribe. Subscribers receive every event published after
//! they subscribed. Slow subscribers that fall behind by more than the
//! channel capacity get a `Lagged` error on their next `recv()` call —
//! callers decide whether to skip ahead or restart.

use crate::envelope::Event;
use crate::error::{EventBusError, EventBusResult};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use tracing::warn;

/// Default per-topic channel capacity. Overridable per topic via
/// `InProcessBus::with_capacity_for`.
pub const DEFAULT_CAPACITY: usize = 1024;

#[async_trait]
pub trait EventBus: Send + Sync {
    /// Publish a typed payload on `topic`. Payload is serialized once
    /// to JSON so all subscribers see the same wire form regardless of
    /// concrete type — this matches the PG bridge which only carries JSON.
    async fn publish<T: Serialize + Send + Sync>(
        &self,
        topic: &str,
        payload: &T,
    ) -> EventBusResult<usize>;

    /// Publish a pre-serialized envelope. Used by the PG bridge to avoid
    /// re-serializing payloads it received as JSON.
    async fn publish_raw(&self, event: Event<serde_json::Value>) -> EventBusResult<usize>;

    /// Subscribe to a topic. Returns an [`EventStream`] that yields
    /// typed events of `T`. Events that fail to deserialize into `T`
    /// are skipped with a `tracing::warn` log — typed subscribers are
    /// expected to be tolerant of foreign payloads on shared topics.
    fn subscribe<T: DeserializeOwned + Send + 'static>(&self, topic: &str) -> EventStream<T>;
}

// ---------------------------------------------------------------------------
// EventStream — typed wrapper around broadcast::Receiver
// ---------------------------------------------------------------------------

pub struct EventStream<T> {
    topic: String,
    rx: broadcast::Receiver<Event<serde_json::Value>>,
    _marker: std::marker::PhantomData<T>,
}

impl<T: DeserializeOwned> EventStream<T> {
    /// Receive the next typed event. Returns:
    /// * `Ok(Some(event))` — successful delivery
    /// * `Ok(None)` — payload could not be deserialized into `T`
    ///                (logged at warn; caller can simply call recv again)
    /// * `Err(Lagged)` — subscriber fell behind; the next call resumes
    ///                   from the most recent unread event
    /// * `Err(Closed)` — sender side dropped, no more events will arrive
    pub async fn recv(&mut self) -> EventBusResult<Option<Event<T>>> {
        match self.rx.recv().await {
            Ok(raw) => match serde_json::from_value::<T>(raw.payload.clone()) {
                Ok(payload) => Ok(Some(raw.map(|_| payload))),
                Err(e) => {
                    warn!(topic = %self.topic, error = %e, "skipping foreign payload");
                    Ok(None)
                }
            },
            Err(broadcast::error::RecvError::Lagged(skipped)) => Err(EventBusError::Lagged {
                topic: self.topic.clone(),
                skipped,
            }),
            Err(broadcast::error::RecvError::Closed) => {
                Err(EventBusError::Closed(self.topic.clone()))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// InProcessBus
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct InProcessBus {
    inner: Arc<Inner>,
}

struct Inner {
    default_capacity: usize,
    capacity_overrides: RwLock<HashMap<String, usize>>,
    senders: RwLock<HashMap<String, broadcast::Sender<Event<serde_json::Value>>>>,
}

impl Default for InProcessBus {
    fn default() -> Self {
        Self::new()
    }
}

impl InProcessBus {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(default_capacity: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                default_capacity,
                capacity_overrides: RwLock::new(HashMap::new()),
                senders: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Override capacity for a specific topic. Must be called before any
    /// publish/subscribe on that topic to take effect.
    pub fn with_capacity_for(&self, topic: &str, capacity: usize) {
        self.inner
            .capacity_overrides
            .write()
            .expect("capacity overrides lock poisoned")
            .insert(topic.to_string(), capacity);
    }

    fn capacity_for(&self, topic: &str) -> usize {
        self.inner
            .capacity_overrides
            .read()
            .expect("capacity overrides lock poisoned")
            .get(topic)
            .copied()
            .unwrap_or(self.inner.default_capacity)
    }

    /// Get-or-create the broadcast sender for `topic`. Single function
    /// keeps the lazy-init logic in one place — callers (publish,
    /// subscribe, bridge) all go through here.
    fn sender(&self, topic: &str) -> broadcast::Sender<Event<serde_json::Value>> {
        // Fast path: read lock.
        if let Some(sender) = self
            .inner
            .senders
            .read()
            .expect("senders lock poisoned")
            .get(topic)
            .cloned()
        {
            return sender;
        }
        // Slow path: write lock + double-check.
        let mut guard = self.inner.senders.write().expect("senders lock poisoned");
        guard
            .entry(topic.to_string())
            .or_insert_with(|| {
                let cap = self.capacity_for(topic);
                broadcast::channel(cap).0
            })
            .clone()
    }

    /// Raw broadcast receiver for `topic`. The SSE bridge in `qtss-api`
    /// uses this to forward verbatim JSON envelopes to browsers without
    /// going through `EventStream`'s typed deserialization (the bridge
    /// must remain payload-agnostic).
    pub fn raw_receiver(
        &self,
        topic: &str,
    ) -> broadcast::Receiver<Event<serde_json::Value>> {
        self.sender(topic).subscribe()
    }

    /// Number of currently active subscribers on `topic`. Useful for
    /// metrics and tests.
    pub fn subscriber_count(&self, topic: &str) -> usize {
        self.inner
            .senders
            .read()
            .expect("senders lock poisoned")
            .get(topic)
            .map(|s| s.receiver_count())
            .unwrap_or(0)
    }
}

#[async_trait]
impl EventBus for InProcessBus {
    async fn publish<T: Serialize + Send + Sync>(
        &self,
        topic: &str,
        payload: &T,
    ) -> EventBusResult<usize> {
        let json = serde_json::to_value(payload).map_err(EventBusError::Serde)?;
        let event = Event::new(topic, json);
        self.publish_raw(event).await
    }

    async fn publish_raw(&self, event: Event<serde_json::Value>) -> EventBusResult<usize> {
        let topic = event.topic.clone();
        let sender = self.sender(&topic);
        // broadcast::send returns the number of receivers; Err only if
        // there are zero receivers, which we treat as a soft no-op.
        match sender.send(event) {
            Ok(n) => Ok(n),
            Err(_) => Ok(0),
        }
    }

    fn subscribe<T: DeserializeOwned + Send + 'static>(&self, topic: &str) -> EventStream<T> {
        let sender = self.sender(topic);
        EventStream {
            topic: topic.to_string(),
            rx: sender.subscribe(),
            _marker: std::marker::PhantomData,
        }
    }
}

//! qtss-eventbus — typed publish/subscribe for the v2 architecture.
//!
//! See `docs/QTSS_V2_ARCHITECTURE_PLAN.md` §3.1 (event topics).
//!
//! ## Two layers
//! * **In-process** ([`InProcessBus`]): a `tokio::sync::broadcast` channel
//!   per topic. Used by every worker for low-latency fan-out (bar.closed,
//!   pattern.detected, intent.created, ...).
//! * **PG bridge** ([`PgNotifyBridge`]): subscribes to a Postgres
//!   `LISTEN` channel and re-publishes incoming notifications onto the
//!   in-process bus under the same topic name. Used to bridge cross-process
//!   notifications such as `config_changed` (emitted by migration 0014's
//!   trigger) into the local cache invalidation pipeline.
//!
//! ## Why one trait, not many
//! Following CLAUDE.md rule #1, every event flows through the same
//! [`EventBus`] interface. Adding a new topic = adding an entry to
//! [`topics`]; no new trait, no per-topic if/else branching.

#![forbid(unsafe_code)]

mod bus;
mod envelope;
mod error;
mod pg_bridge;
pub mod topics;

#[cfg(test)]
mod tests;

pub use bus::{EventBus, EventStream, InProcessBus};
pub use envelope::Event;
pub use error::{EventBusError, EventBusResult};
pub use pg_bridge::{PgBridgeHandle, PgNotifyBridge};

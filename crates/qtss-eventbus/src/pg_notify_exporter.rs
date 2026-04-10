//! Mirror in-process events to Postgres `NOTIFY` so other processes
//! (notably `qtss-api`'s SSE bridge) can pick them up.
//!
//! Each topic in the list gets one subscriber task that drains the
//! local bus and runs `pg_notify('<topic>', '<json>')` for every event.
//! This is the *outbound* counterpart to [`crate::PgNotifyBridge`]
//! (which is *inbound*: PG NOTIFY → in-process bus).
//!
//! ## Why one task per topic, not one big subscribe
//! Each topic on `InProcessBus` has its own broadcast channel; a single
//! task can only receive from one. Spawning per-topic keeps the per-task
//! state trivial (one rx + one connection handle from the pool) and
//! makes a slow forwarder on one topic unable to back-pressure others.
//!
//! ## Payload size
//! Postgres `NOTIFY` truncates at ~8000 bytes. Anything larger is
//! dropped with a warning rather than corrupted — large payloads should
//! land in a table and be referenced by id, not blasted through NOTIFY.

use crate::bus::{EventBus, InProcessBus};
use crate::envelope::Event;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// Postgres `NOTIFY` payload limit, with headroom for envelope JSON
/// overhead added by Postgres itself.
const MAX_NOTIFY_PAYLOAD: usize = 7_900;

pub struct PgNotifyExporter;

impl PgNotifyExporter {
    /// Spawn one forwarder task per topic. Returns the join handles so
    /// the caller can keep them alive (the loop runs until the bus is
    /// dropped or the task is aborted).
    pub fn start(
        bus: Arc<InProcessBus>,
        pool: PgPool,
        topics: &[&'static str],
    ) -> Vec<JoinHandle<()>> {
        topics
            .iter()
            .map(|t| spawn_topic(bus.clone(), pool.clone(), *t))
            .collect()
    }
}

fn spawn_topic(bus: Arc<InProcessBus>, pool: PgPool, topic: &'static str) -> JoinHandle<()> {
    let mut stream = bus.subscribe::<serde_json::Value>(topic);
    info!(topic = %topic, "pg notify exporter attached");
    tokio::spawn(async move {
        loop {
            match stream.recv().await {
                Ok(Some(event)) => {
                    if let Err(e) = forward(&pool, topic, &event).await {
                        warn!(topic = %topic, error = %e, "pg notify forward failed");
                    }
                }
                Ok(None) => {
                    // Foreign payload on a shared topic — already logged
                    // by EventStream::recv.
                }
                Err(e) => {
                    debug!(topic = %topic, error = %e, "exporter stream ended");
                    break;
                }
            }
        }
    })
}

async fn forward(
    pool: &PgPool,
    topic: &str,
    event: &Event<serde_json::Value>,
) -> Result<(), sqlx::Error> {
    let payload = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
    if payload.len() > MAX_NOTIFY_PAYLOAD {
        warn!(
            topic = %topic,
            bytes = payload.len(),
            "skipping oversized event (> NOTIFY limit)"
        );
        return Ok(());
    }
    sqlx::query("SELECT pg_notify($1, $2)")
        .bind(topic)
        .bind(payload)
        .execute(pool)
        .await?;
    Ok(())
}

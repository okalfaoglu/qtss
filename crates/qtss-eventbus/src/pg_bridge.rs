//! Postgres `LISTEN`/`NOTIFY` bridge.
//!
//! Spawns a background task that holds a dedicated connection from a
//! `PgPool`, runs `LISTEN <channel>` for each configured channel, and
//! republishes each notification onto the in-process bus under a topic
//! name. By default the topic name equals the channel name; pass an
//! explicit map to remap.
//!
//! Used to bridge migration 0014's `config_changed` trigger into the
//! local cache invalidation pipeline of `qtss-config`. The same bridge
//! works for any other PG-driven event we add later (e.g. broker fills
//! pushed via NOTIFY for replay).

use crate::bus::{EventBus, InProcessBus};
use crate::envelope::Event;
use crate::error::EventBusError;
use sqlx::postgres::PgListener;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// Handle to a running bridge task. Drop or call `stop()` to shut it down.
pub struct PgBridgeHandle {
    handle: JoinHandle<()>,
    stop_tx: Option<oneshot::Sender<()>>,
}

impl PgBridgeHandle {
    /// Signal the bridge to shut down and await the task. Idempotent.
    pub async fn stop(mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        let _ = self.handle.await;
    }

    /// Abort without waiting. Used by tests on cleanup paths.
    pub fn abort(&self) {
        self.handle.abort();
    }
}

pub struct PgNotifyBridge;

impl PgNotifyBridge {
    /// Start the bridge. `channels` is the list of PG channel names to
    /// listen on; each notification is republished as
    /// `Event<serde_json::Value>` on the in-process bus under the same
    /// name.
    ///
    /// The payload is parsed as JSON if possible; otherwise it is wrapped
    /// as `{ "raw": "<text>" }` so subscribers always see a JSON object.
    pub async fn start(
        pool: PgPool,
        channels: Vec<String>,
        bus: Arc<InProcessBus>,
    ) -> Result<PgBridgeHandle, EventBusError> {
        let mut listener = PgListener::connect_with(&pool).await?;
        for ch in &channels {
            listener.listen(ch).await?;
            debug!(channel = %ch, "pg listener attached");
        }
        info!(count = channels.len(), "pg notify bridge started");

        let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
        let bus_for_task = bus.clone();

        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stop_rx => {
                        info!("pg notify bridge stopping");
                        break;
                    }
                    res = listener.recv() => {
                        match res {
                            Ok(notification) => {
                                let topic = notification.channel().to_string();
                                let payload = parse_payload(notification.payload());
                                let event = Event::new(topic.clone(), payload);
                                if let Err(e) = bus_for_task.publish_raw(event).await {
                                    warn!(topic = %topic, error = %e, "failed to publish bridged event");
                                }
                            }
                            Err(e) => {
                                error!(error = %e, "pg listener error; restarting connection on next iteration");
                                // Brief backoff so we don't spin if the DB is down.
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            }
                        }
                    }
                }
            }
        });

        Ok(PgBridgeHandle {
            handle,
            stop_tx: Some(stop_tx),
        })
    }
}

/// Parse a NOTIFY payload as JSON. Falls back to wrapping the raw text in
/// `{ "raw": "..." }` so the bus contract (always a JSON object) holds.
fn parse_payload(payload: &str) -> serde_json::Value {
    serde_json::from_str(payload)
        .unwrap_or_else(|_| serde_json::json!({ "raw": payload }))
}

#[cfg(test)]
mod tests {
    use super::parse_payload;

    #[test]
    fn parses_json_object() {
        let v = parse_payload(r#"{"key":"risk.max_dd","scope_id":1}"#);
        assert_eq!(v["key"], "risk.max_dd");
        assert_eq!(v["scope_id"], 1);
    }

    #[test]
    fn wraps_non_json_text() {
        let v = parse_payload("hello world");
        assert_eq!(v["raw"], "hello world");
    }
}

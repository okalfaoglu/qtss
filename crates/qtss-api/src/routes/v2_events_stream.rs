//! `GET /v2/events/stream?topics=a,b,c` — Server-Sent Events bridge.
//!
//! Subscribes to one or more topics on the API's local event bus
//! (which is fed from Postgres `NOTIFY` by the worker via
//! [`qtss_eventbus::PgNotifyExporter`]) and forwards each event to the
//! browser as an SSE message. Topic name = SSE `event:` field; the JSON
//! envelope = SSE `data:` field.
//!
//! Why one endpoint, not many: dashboards typically want a handful of
//! topics on a single connection (pattern.detected + intent.created +
//! position.* etc.). A single endpoint with a `topics` query parameter
//! avoids hard-coding a route per topic and keeps the browser EventSource
//! count to one.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::routing::get;
use axum::Router;
use futures::stream::{self, Stream, StreamExt};
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;
use tracing::warn;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct StreamQuery {
    /// Comma-separated topic names. Only topics on
    /// [`qtss_eventbus::topics::SSE_EXPORTED_TOPICS`] are accepted; the
    /// rest are silently dropped so a stale client can't ask the bridge
    /// to listen on something the worker isn't mirroring to PG NOTIFY.
    pub topics: Option<String>,
}

pub fn v2_events_stream_router() -> Router<SharedState> {
    Router::new().route("/v2/events/stream", get(stream_handler))
}

async fn stream_handler(
    State(state): State<SharedState>,
    Query(q): Query<StreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>, ApiError> {
    let allowed: std::collections::HashSet<&'static str> =
        qtss_eventbus::topics::SSE_EXPORTED_TOPICS.iter().copied().collect();

    let requested: Vec<&'static str> = q
        .topics
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .filter_map(|t| allowed.iter().find(|a| **a == t).copied())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| qtss_eventbus::topics::SSE_EXPORTED_TOPICS.to_vec());

    if requested.is_empty() {
        return Err(ApiError::bad_request(
            "no valid topics requested; see qtss_eventbus::topics::SSE_EXPORTED_TOPICS",
        ));
    }

    // Subscribe to each topic and merge into a single stream. We pull
    // raw broadcast receivers (not the typed EventStream) so the SSE
    // payload is the verbatim JSON envelope the worker published —
    // dashboards can decode whatever shape the producer chose.
    let mut merged = stream::SelectAll::new();
    for topic in &requested {
        let rx = state.event_bus.raw_receiver(topic);
        let topic_name: &'static str = topic;
        let typed = BroadcastStream::new(rx).filter_map(move |res| async move {
            match res {
                Ok(evt) => {
                    let data = serde_json::to_string(&evt).unwrap_or_else(|_| "{}".into());
                    Some(Ok::<_, Infallible>(SseEvent::default().event(topic_name).data(data)))
                }
                Err(e) => {
                    warn!(topic = %topic_name, error = %e, "sse subscriber lagged");
                    None
                }
            }
        });
        merged.push(Box::pin(typed) as std::pin::Pin<Box<dyn Stream<Item = _> + Send>>);
    }

    Ok(Sse::new(merged).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    ))
}

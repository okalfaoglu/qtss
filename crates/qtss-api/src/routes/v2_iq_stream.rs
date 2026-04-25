//! `GET /v2/iq-stream/{exchange}/{symbol}/{tf}` — Server-Sent Events
//! channel for the IQ Chart (FAZ 25 PR-25F).
//!
//! Subscribes to PostgreSQL `LISTEN qtss_iq_changed` and forwards
//! every payload that matches the requested (exchange, symbol, tf)
//! tuple to the connected browser. The frontend uses each push to
//! invalidate its react-query cache so the chart updates within a
//! second of a new motive / abc / structure / setup landing in the
//! database — replaces the 20s polling tick with sub-second latency.
//!
//! Payload shape (writer side fires this for every relevant change):
//! ```json
//! { "kind": "iq_structure" | "elliott_early" | "iq_setup",
//!   "exchange": "binance", "segment": "futures",
//!   "symbol": "BTCUSDT", "timeframe": "4h" }
//! ```

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, Sse, KeepAlive};
use axum::routing::get;
use axum::Router;
use futures::stream::Stream;
use serde::Deserialize;
use serde_json::Value;
use sqlx::postgres::PgListener;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, warn};

use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct Q {
    pub segment: Option<String>,
}

pub fn v2_iq_stream_router() -> Router<SharedState> {
    Router::new().route("/v2/iq-stream/{exchange}/{symbol}/{tf}", get(stream))
}

async fn stream(
    State(state): State<SharedState>,
    Path((exchange, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<Q>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let pool = state.pool.clone();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);

    // Spawn the listener task. It exits naturally when the channel
    // sender drops (i.e. the client disconnects).
    tokio::spawn(async move {
        // Each connection gets its own dedicated PgListener — keeps
        // back-pressure isolated and prevents one slow consumer from
        // stalling the others.
        let mut listener = match PgListener::connect_with(&pool).await {
            Ok(l) => l,
            Err(e) => {
                warn!(%e, "iq-stream: PgListener connect failed");
                let _ = tx
                    .send(Ok(Event::default().event("error").data("listener_failed")))
                    .await;
                return;
            }
        };
        if let Err(e) = listener
            .listen_all(["qtss_iq_changed", "qtss_market_bars_gap_filled"])
            .await
        {
            warn!(%e, "iq-stream: LISTEN failed");
            return;
        }

        // Hello frame so the client can confirm the connection is
        // live before any data event arrives.
        let _ = tx
            .send(Ok(Event::default().event("hello").data(format!(
                r#"{{"exchange":"{}","segment":"{}","symbol":"{}","tf":"{}"}}"#,
                exchange, segment, symbol, tf
            ))))
            .await;

        loop {
            match listener.recv().await {
                Ok(notif) => {
                    let channel = notif.channel();
                    let payload = notif.payload();
                    let parsed: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
                    // Filter — only forward when the payload matches
                    // this connection's symbol+tf (and segment when
                    // present). Reduces wire traffic; one db row
                    // change wakes only the right tabs.
                    let matches = matches_filter(&parsed, &exchange, &segment, &symbol, &tf);
                    if !matches {
                        continue;
                    }
                    let evt = Event::default().event(channel).data(payload);
                    if tx.send(Ok(evt)).await.is_err() {
                        debug!("iq-stream: client disconnected");
                        break;
                    }
                }
                Err(e) => {
                    warn!(%e, "iq-stream: listener error");
                    break;
                }
            }
        }
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

fn matches_filter(
    payload: &Value,
    exchange: &str,
    segment: &str,
    symbol: &str,
    tf: &str,
) -> bool {
    let str_eq = |k: &str, v: &str| -> bool {
        payload.get(k).and_then(|x| x.as_str()) == Some(v)
    };
    let str_or_missing = |k: &str, v: &str| -> bool {
        match payload.get(k).and_then(|x| x.as_str()) {
            Some(s) => s == v,
            None => true, // payload omitted the key — broadcast to all
        }
    };
    str_eq("symbol", symbol)
        && str_or_missing("exchange", exchange)
        && str_or_missing("segment", segment)
        && (payload.get("timeframe").is_none()
            || payload.get("timeframe").and_then(|x| x.as_str()) == Some(tf))
}

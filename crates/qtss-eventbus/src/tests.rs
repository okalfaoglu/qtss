//! Unit tests for the in-process bus. PG bridge has its own inline tests
//! for the payload parser; full PG integration is exercised by the
//! workspace integration harness in a later PR.

use crate::bus::{EventBus, InProcessBus};
use crate::topics;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct BarClosedPayload {
    symbol: String,
    close: f64,
}

#[tokio::test]
async fn publish_then_receive_round_trip() {
    let bus = InProcessBus::new();
    let mut sub = bus.subscribe::<BarClosedPayload>(topics::BAR_CLOSED);

    let payload = BarClosedPayload {
        symbol: "BTCUSDT".into(),
        close: 100.0,
    };
    let delivered = bus.publish(topics::BAR_CLOSED, &payload).await.unwrap();
    assert_eq!(delivered, 1);

    let evt = sub.recv().await.unwrap().expect("payload should deserialize");
    assert_eq!(evt.payload, payload);
    assert_eq!(evt.topic, topics::BAR_CLOSED);
}

#[tokio::test]
async fn publish_with_no_subscribers_is_soft_noop() {
    let bus = InProcessBus::new();
    let payload = BarClosedPayload {
        symbol: "ETHUSDT".into(),
        close: 50.0,
    };
    // Subscribing then dropping leaves zero receivers; sender exists.
    drop(bus.subscribe::<BarClosedPayload>(topics::BAR_CLOSED));
    let delivered = bus.publish(topics::BAR_CLOSED, &payload).await.unwrap();
    assert_eq!(delivered, 0);
}

#[tokio::test]
async fn fan_out_to_multiple_subscribers() {
    let bus = InProcessBus::new();
    let mut a = bus.subscribe::<BarClosedPayload>(topics::BAR_CLOSED);
    let mut b = bus.subscribe::<BarClosedPayload>(topics::BAR_CLOSED);
    let mut c = bus.subscribe::<BarClosedPayload>(topics::BAR_CLOSED);

    let payload = BarClosedPayload {
        symbol: "SOLUSDT".into(),
        close: 25.0,
    };
    let delivered = bus.publish(topics::BAR_CLOSED, &payload).await.unwrap();
    assert_eq!(delivered, 3);

    for sub in [&mut a, &mut b, &mut c] {
        let evt = sub.recv().await.unwrap().expect("delivered");
        assert_eq!(evt.payload, payload);
    }
}

#[tokio::test]
async fn topics_are_isolated() {
    let bus = InProcessBus::new();
    let mut bars = bus.subscribe::<BarClosedPayload>(topics::BAR_CLOSED);
    let _ticks = bus.subscribe::<BarClosedPayload>(topics::TICK_TRADE);

    let payload = BarClosedPayload {
        symbol: "BNBUSDT".into(),
        close: 300.0,
    };
    bus.publish(topics::TICK_TRADE, &payload).await.unwrap();

    // bars subscriber must not see the tick payload — different topic.
    let try_recv = tokio::time::timeout(std::time::Duration::from_millis(50), bars.recv()).await;
    assert!(try_recv.is_err(), "bars subscriber must time out");
}

#[tokio::test]
async fn foreign_payload_is_skipped_not_fatal() {
    #[derive(Serialize)]
    struct OtherShape {
        unrelated_field: i32,
    }

    let bus = InProcessBus::new();
    let mut sub = bus.subscribe::<BarClosedPayload>(topics::BAR_CLOSED);

    bus.publish(
        topics::BAR_CLOSED,
        &OtherShape { unrelated_field: 7 },
    )
    .await
    .unwrap();

    // recv returns Ok(None) for the foreign payload.
    let result = sub.recv().await.unwrap();
    assert!(result.is_none(), "foreign payload should be skipped");
}

#[tokio::test]
async fn arc_bus_is_shareable() {
    let bus = Arc::new(InProcessBus::new());
    let bus2 = bus.clone();

    let mut sub = bus.subscribe::<BarClosedPayload>(topics::BAR_CLOSED);
    let payload = BarClosedPayload {
        symbol: "ARB".into(),
        close: 1.0,
    };
    bus2.publish(topics::BAR_CLOSED, &payload).await.unwrap();

    let evt = sub.recv().await.unwrap().expect("delivered");
    assert_eq!(evt.payload, payload);
}

#[tokio::test]
async fn subscriber_count_reflects_active_subs() {
    let bus = InProcessBus::new();
    assert_eq!(bus.subscriber_count(topics::BAR_CLOSED), 0);

    let _a = bus.subscribe::<BarClosedPayload>(topics::BAR_CLOSED);
    let b = bus.subscribe::<BarClosedPayload>(topics::BAR_CLOSED);
    assert_eq!(bus.subscriber_count(topics::BAR_CLOSED), 2);

    drop(b);
    assert_eq!(bus.subscriber_count(topics::BAR_CLOSED), 1);
}

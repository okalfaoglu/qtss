use crate::chain::verify_chain;
use crate::sink::{AuditSink, MemoryAuditSink};
use crate::types::NewAuditEvent;
use serde_json::json;

#[tokio::test]
async fn empty_chain_verifies() {
    let sink = MemoryAuditSink::new();
    let rows = sink.read_all().await.unwrap();
    verify_chain(&rows).unwrap();
}

#[tokio::test]
async fn appended_chain_links_correctly() {
    let sink = MemoryAuditSink::new();
    sink.append(NewAuditEvent::new(
        "alice",
        "config.set",
        "risk.max_dd",
        json!({ "old": 0.05, "new": 0.07 }),
    ))
    .await
    .unwrap();
    sink.append(NewAuditEvent::new(
        "bob",
        "intent.approve",
        "intent-1",
        json!({ "venue": "binance_spot" }),
    ))
    .await
    .unwrap();
    sink.append(NewAuditEvent::new(
        "system",
        "killswitch.trip",
        "global",
        json!({ "reason": "max_dd_breach" }),
    ))
    .await
    .unwrap();

    let rows = sink.read_all().await.unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows[0].prev_hash.is_none(), "genesis row has no prev");
    assert_eq!(rows[1].prev_hash.as_ref().unwrap(), &rows[0].row_hash);
    assert_eq!(rows[2].prev_hash.as_ref().unwrap(), &rows[1].row_hash);
    verify_chain(&rows).unwrap();
}

#[tokio::test]
async fn tampered_payload_breaks_chain() {
    let sink = MemoryAuditSink::new();
    sink.append(NewAuditEvent::new(
        "alice",
        "config.set",
        "risk.max_dd",
        json!({ "new": 0.07 }),
    ))
    .await
    .unwrap();
    sink.append(NewAuditEvent::new(
        "alice",
        "config.set",
        "risk.max_pos",
        json!({ "new": 5 }),
    ))
    .await
    .unwrap();

    let mut rows = sink.read_all().await.unwrap();
    // Tamper with the payload of the first row, leaving the hash intact.
    rows[0].payload = json!({ "new": 9.99 });

    let err = verify_chain(&rows).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("hash mismatch") || msg.contains("row hash mismatch"));
}

#[tokio::test]
async fn deleted_middle_row_breaks_chain() {
    let sink = MemoryAuditSink::new();
    for i in 0..3 {
        sink.append(NewAuditEvent::new(
            "alice",
            "config.set",
            format!("k{i}"),
            json!({ "v": i }),
        ))
        .await
        .unwrap();
    }
    let mut rows = sink.read_all().await.unwrap();
    rows.remove(1);
    let err = verify_chain(&rows).unwrap_err();
    assert!(err.to_string().contains("chain broken"));
}

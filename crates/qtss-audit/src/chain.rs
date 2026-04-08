//! Hash chain primitives. Kept dependency-light so the same code can run
//! in both the writer (sink) and an offline verifier.

use crate::error::{AuditError, AuditResult};
use crate::types::{AuditRecord, NewAuditEvent};
use sha2::{Digest, Sha256};

/// Canonical JSON form of an event for hashing. We sort keys to keep the
/// digest stable across serializer versions and language clients.
pub fn canonical_payload(evt: &NewAuditEvent) -> AuditResult<Vec<u8>> {
    // serde_json::to_value gives us a Value we can canonicalize manually.
    let mut buf = Vec::with_capacity(256);
    buf.extend_from_slice(evt.actor.as_bytes());
    buf.push(0);
    buf.extend_from_slice(evt.action.as_bytes());
    buf.push(0);
    buf.extend_from_slice(evt.subject.as_bytes());
    buf.push(0);
    if let Some(corr) = evt.correlation_id {
        buf.extend_from_slice(corr.as_bytes());
    }
    buf.push(0);
    let canonical = canonicalize_value(&evt.payload);
    buf.extend_from_slice(canonical.as_bytes());
    Ok(buf)
}

/// Stable string form of a JSON value: object keys sorted lexicographically,
/// arrays preserved in order. Matches what an offline auditor would compute.
fn canonicalize_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", k, canonicalize_value(&map[k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        serde_json::Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(canonicalize_value).collect();
            format!("[{}]", parts.join(","))
        }
        other => other.to_string(),
    }
}

/// Compute the row hash given the previous row's hash (or empty bytes for
/// the genesis row) and the canonical payload.
pub fn hash_row(prev_hash: Option<&[u8]>, canonical: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.unwrap_or(&[]));
    hasher.update(canonical);
    hasher.finalize().to_vec()
}

/// Verify a chain of records in insertion order. Returns Ok on success or
/// the first violation found. Used by the offline auditor and by tests.
pub fn verify_chain(records: &[AuditRecord]) -> AuditResult<()> {
    let mut expected_prev: Option<Vec<u8>> = None;
    for rec in records {
        // 1. prev_hash must match the previous row's row_hash.
        let actual_prev = rec.prev_hash.as_deref();
        let expected_slice = expected_prev.as_deref();
        if actual_prev != expected_slice {
            return Err(AuditError::ChainBroken {
                id: rec.id,
                expected: expected_slice.map(hex::encode).unwrap_or_default(),
                found: actual_prev.map(hex::encode).unwrap_or_default(),
            });
        }
        // 2. row_hash must equal sha256(prev_hash || canonical_payload).
        let evt = NewAuditEvent {
            actor: rec.actor.clone(),
            action: rec.action.clone(),
            subject: rec.subject.clone(),
            payload: rec.payload.clone(),
            correlation_id: rec.correlation_id,
        };
        let canonical = canonical_payload(&evt)?;
        let recomputed = hash_row(actual_prev, &canonical);
        if recomputed != rec.row_hash {
            return Err(AuditError::HashMismatch {
                id: rec.id,
                stored: hex::encode(&rec.row_hash),
                recomputed: hex::encode(&recomputed),
            });
        }
        expected_prev = Some(rec.row_hash.clone());
    }
    Ok(())
}

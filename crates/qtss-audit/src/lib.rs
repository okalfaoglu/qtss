//! qtss-audit — append-only, hash-chained audit log.
//!
//! Every state-changing action in QTSS v2 (config edit, intent approval,
//! kill switch, secret rotation, ...) writes one row here. Rows are linked
//! by `row_hash = sha256(prev_hash || canonical_payload)` so any tampering
//! breaks the chain. The DB enforces append-only via triggers (see
//! migration 0015); this crate enforces *correct linkage* on the write
//! path and provides a verifier for the read path.

mod chain;
mod error;
mod sink;
mod types;

#[cfg(test)]
mod tests;

pub use chain::{canonical_payload, hash_row, verify_chain};
pub use error::{AuditError, AuditResult};
pub use sink::{AuditSink, MemoryAuditSink, PgAuditSink};
pub use types::{AuditRecord, NewAuditEvent};

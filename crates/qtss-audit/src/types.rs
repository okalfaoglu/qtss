use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Caller-supplied event before it gets hashed and persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAuditEvent {
    pub actor: String,
    pub action: String,
    pub subject: String,
    pub payload: serde_json::Value,
    pub correlation_id: Option<Uuid>,
}

impl NewAuditEvent {
    pub fn new(
        actor: impl Into<String>,
        action: impl Into<String>,
        subject: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            actor: actor.into(),
            action: action.into(),
            subject: subject.into(),
            payload,
            correlation_id: None,
        }
    }

    pub fn with_correlation(mut self, id: Uuid) -> Self {
        self.correlation_id = Some(id);
        self
    }
}

/// A persisted audit row, as returned by the sink after insertion or
/// during chain verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub actor: String,
    pub action: String,
    pub subject: String,
    pub payload: serde_json::Value,
    pub correlation_id: Option<Uuid>,
    pub prev_hash: Option<Vec<u8>>,
    pub row_hash: Vec<u8>,
}

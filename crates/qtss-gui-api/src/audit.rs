//! `/v2/audit` wire types -- Faz 5 Adim (k).
//!
//! The Audit Log Viewer card is a thin projection of the HTTP audit
//! trail (`audit_log`) into a wire shape the React table can render
//! without parsing UUIDs or raw `details` blobs. Same trick as the
//! AI Decisions card: a single-line `details_preview` keeps the table
//! cheap and the operator clicks through for the full document.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ai_decisions::payload_preview;

/// One row in the Audit Log Viewer table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub at: DateTime<Utc>,
    pub request_id: Option<String>,
    pub user_id: Option<String>,
    pub org_id: Option<String>,
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub roles: Vec<String>,
    /// `details->>'kind'` if present, else None. Mirrors the storage
    /// `list_recent(details_kind=...)` filter so the React side can
    /// render a tag column without re-parsing the JSON blob.
    pub kind: Option<String>,
    /// Trimmed single-line preview of the `details` JSON. Empty when
    /// the row carries no details.
    pub details_preview: Option<String>,
}

/// Whole `/v2/audit` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditView {
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<AuditEntry>,
}

/// Pull the `details->>'kind'` discriminant out of the row's JSON, if
/// any. Used by the route to populate the `kind` tag column without
/// re-querying.
pub fn extract_kind(details: Option<&serde_json::Value>) -> Option<String> {
    details
        .and_then(|v| v.get("kind"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Project an optional details JSON into the trimmed preview string.
pub fn details_preview(details: Option<&serde_json::Value>) -> Option<String> {
    details.map(payload_preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_kind_returns_string_field() {
        let v = json!({ "kind": "config_upsert", "module": "api" });
        assert_eq!(extract_kind(Some(&v)).as_deref(), Some("config_upsert"));
    }

    #[test]
    fn extract_kind_handles_missing_or_non_string() {
        assert!(extract_kind(None).is_none());
        let v = json!({ "kind": 42 });
        assert!(extract_kind(Some(&v)).is_none());
    }

    #[test]
    fn details_preview_passes_through_or_none() {
        assert!(details_preview(None).is_none());
        let v = json!({ "kind": "x" });
        let p = details_preview(Some(&v)).unwrap();
        assert!(p.contains("\"kind\""));
    }

    #[test]
    fn json_round_trip() {
        let view = AuditView {
            generated_at: Utc::now(),
            entries: vec![AuditEntry {
                id: "00000000-0000-0000-0000-000000000001".into(),
                at: Utc::now(),
                request_id: Some("req-1".into()),
                user_id: None,
                org_id: None,
                method: "GET".into(),
                path: "/v2/dashboard".into(),
                status_code: 200,
                roles: vec!["dashboard".into()],
                kind: Some("read".into()),
                details_preview: Some("{}".into()),
            }],
        };
        let j = serde_json::to_string(&view).unwrap();
        let back: AuditView = serde_json::from_str(&j).unwrap();
        assert_eq!(back.entries.len(), 1);
        assert_eq!(back.entries[0].path, "/v2/dashboard");
    }
}

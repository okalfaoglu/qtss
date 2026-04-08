//! `/v2/ai-decisions` wire types -- Faz 5 Adim (j).
//!
//! The AI Decisions card shows the human-in-the-loop queue: every
//! `ai_approval_requests` row the org has, normalised to a wire shape
//! the React table can render without parsing UUIDs or raw payloads.
//!
//! The DTOs are deliberately narrower than the storage row -- the
//! payload JSON is summarised to a short preview string so the table
//! stays cheap; the full payload is fetched on demand via the existing
//! detail route when the operator opens a row.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Lifecycle status. Mirrors the storage `status` column with a
/// closed enum so the React side can switch on it directly instead
/// of comparing strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiDecisionStatus {
    Pending,
    Approved,
    Rejected,
    Other,
}

impl AiDecisionStatus {
    pub fn parse(raw: &str) -> Self {
        match raw {
            "pending" => Self::Pending,
            "approved" => Self::Approved,
            "rejected" => Self::Rejected,
            _ => Self::Other,
        }
    }
}

/// One row in the AI Decisions table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiDecisionEntry {
    pub id: String,
    pub kind: String,
    pub status: AiDecisionStatus,
    pub model_hint: Option<String>,
    /// Short, human-readable summary of the request payload. Capped at
    /// `payload_preview_max_len` so the wire stays tight.
    pub payload_preview: String,
    pub admin_note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
}

/// Whole `/v2/ai-decisions` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiDecisionsView {
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<AiDecisionEntry>,
}

/// Hard cap on the preview string. Anything longer is trimmed with an
/// ellipsis -- the operator clicks through to the detail route for
/// the full document.
pub const PAYLOAD_PREVIEW_MAX_LEN: usize = 240;

/// Trim a JSON value into a short single-line preview.
pub fn payload_preview(value: &serde_json::Value) -> String {
    let raw = value.to_string();
    let mut s: String = raw.chars().filter(|c| *c != '\n').collect();
    if s.len() > PAYLOAD_PREVIEW_MAX_LEN {
        let cut = s
            .char_indices()
            .nth(PAYLOAD_PREVIEW_MAX_LEN - 1)
            .map(|(i, _)| i)
            .unwrap_or(PAYLOAD_PREVIEW_MAX_LEN - 1);
        s.truncate(cut);
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_parse_round_trip() {
        assert_eq!(AiDecisionStatus::parse("pending"), AiDecisionStatus::Pending);
        assert_eq!(AiDecisionStatus::parse("approved"), AiDecisionStatus::Approved);
        assert_eq!(AiDecisionStatus::parse("rejected"), AiDecisionStatus::Rejected);
        assert_eq!(AiDecisionStatus::parse("weird"), AiDecisionStatus::Other);
    }

    #[test]
    fn payload_preview_trims_long_strings() {
        let big = json!({ "blob": "x".repeat(1_000) });
        let p = payload_preview(&big);
        assert!(p.chars().count() <= PAYLOAD_PREVIEW_MAX_LEN);
        assert!(p.ends_with('…'));
    }

    #[test]
    fn payload_preview_strips_newlines() {
        let v = json!({ "k": "line1\nline2" });
        let p = payload_preview(&v);
        assert!(!p.contains('\n'));
    }

    #[test]
    fn json_round_trip() {
        let view = AiDecisionsView {
            generated_at: Utc::now(),
            entries: vec![AiDecisionEntry {
                id: "00000000-0000-0000-0000-000000000001".into(),
                kind: "strategy_param_change".into(),
                status: AiDecisionStatus::Pending,
                model_hint: Some("opus-4.6".into()),
                payload_preview: "{\"min_confidence\":0.75}".into(),
                admin_note: None,
                created_at: Utc::now(),
                decided_at: None,
            }],
        };
        let j = serde_json::to_string(&view).unwrap();
        let back: AiDecisionsView = serde_json::from_str(&j).unwrap();
        assert_eq!(back.entries.len(), 1);
        assert_eq!(back.entries[0].status, AiDecisionStatus::Pending);
    }
}

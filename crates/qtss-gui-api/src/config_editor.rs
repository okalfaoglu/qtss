//! `/v2/config` wire types -- Faz 5 Adim (i).
//!
//! The Config Editor card lists every `system_config` row grouped by
//! module so the React form can render one collapsible section per
//! module instead of a flat 200-row table. The wire DTO is a strict
//! projection of `SystemConfigRow` -- it carries the same fields with
//! one extra `masked` flag mirrored from the repository's
//! `is_secret` masking so the frontend never has to re-derive what
//! "_masked" means.
//!
//! Mutations stay on the existing `/admin/system-config` admin route
//! (admin role). This module is read-only on purpose: the dashboard
//! roles see the catalogue, the admin role edits it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// One row in the config editor table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigEntry {
    pub module: String,
    pub config_key: String,
    pub value: JsonValue,
    pub schema_version: i32,
    pub description: Option<String>,
    pub is_secret: bool,
    /// Mirrors the repository's `_masked` projection so the React side
    /// never has to inspect the value to decide whether to render a
    /// "secret" badge instead of a JSON tree.
    pub masked: bool,
    pub updated_at: DateTime<Utc>,
}

impl ConfigEntry {
    /// Returns true if `value` was already redacted by the storage
    /// layer's secret-masking projection.
    pub fn detect_masked(value: &JsonValue, is_secret: bool) -> bool {
        is_secret
            && value
                .as_object()
                .and_then(|o| o.get("_masked"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
    }
}

/// One module section in the editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigGroup {
    pub module: String,
    pub entries: Vec<ConfigEntry>,
}

/// Whole `/v2/config` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigEditorView {
    pub generated_at: DateTime<Utc>,
    pub groups: Vec<ConfigGroup>,
}

/// Pure builder. Takes a flat list of `ConfigEntry` (already projected
/// from the storage rows) and folds them into modules, sorted by
/// module name then by config_key inside each group.
pub fn group_config_entries(mut entries: Vec<ConfigEntry>) -> Vec<ConfigGroup> {
    entries.sort_by(|a, b| {
        a.module
            .cmp(&b.module)
            .then_with(|| a.config_key.cmp(&b.config_key))
    });
    let mut groups: Vec<ConfigGroup> = Vec::new();
    for e in entries {
        if let Some(last) = groups.last_mut() {
            if last.module == e.module {
                last.entries.push(e);
                continue;
            }
        }
        groups.push(ConfigGroup {
            module: e.module.clone(),
            entries: vec![e],
        });
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(module: &str, key: &str, secret: bool, value: JsonValue) -> ConfigEntry {
        let masked = ConfigEntry::detect_masked(&value, secret);
        ConfigEntry {
            module: module.into(),
            config_key: key.into(),
            value,
            schema_version: 1,
            description: None,
            is_secret: secret,
            masked,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn detect_masked_picks_up_redacted_value() {
        let v = json!({ "_masked": true });
        assert!(ConfigEntry::detect_masked(&v, true));
    }

    #[test]
    fn detect_masked_false_for_plain_secret_value() {
        // The repo only masks on read paths; an in-memory row built
        // from a write call still carries the real value.
        let v = json!({ "value": "deadbeef" });
        assert!(!ConfigEntry::detect_masked(&v, true));
    }

    #[test]
    fn detect_masked_false_when_not_secret() {
        let v = json!({ "_masked": true });
        assert!(!ConfigEntry::detect_masked(&v, false));
    }

    #[test]
    fn group_sorts_modules_and_keys() {
        let entries = vec![
            entry("risk", "max_drawdown", false, json!(0.05)),
            entry("api", "v2_chart_renko_brick_pct", false, json!("0.005")),
            entry("api", "jwt_audience", false, json!("qtss-api")),
            entry("risk", "killswitch_drawdown", false, json!(0.08)),
        ];
        let groups = group_config_entries(entries);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].module, "api");
        assert_eq!(groups[0].entries[0].config_key, "jwt_audience");
        assert_eq!(groups[0].entries[1].config_key, "v2_chart_renko_brick_pct");
        assert_eq!(groups[1].module, "risk");
        assert_eq!(groups[1].entries[0].config_key, "killswitch_drawdown");
    }

    #[test]
    fn group_handles_empty() {
        assert!(group_config_entries(vec![]).is_empty());
    }

    #[test]
    fn json_round_trip() {
        let view = ConfigEditorView {
            generated_at: Utc::now(),
            groups: group_config_entries(vec![entry(
                "api",
                "jwt_audience",
                false,
                json!("qtss-api"),
            )]),
        };
        let j = serde_json::to_string(&view).unwrap();
        let back: ConfigEditorView = serde_json::from_str(&j).unwrap();
        assert_eq!(back.groups.len(), 1);
        assert_eq!(back.groups[0].entries[0].config_key, "jwt_audience");
    }
}

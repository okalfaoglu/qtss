use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Allowed value types for `config_schema.value_type`. Must stay in sync
/// with the CHECK constraint in migration 0014.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Int,
    Float,
    Decimal,
    String,
    Bool,
    Enum,
    Object,
    Array,
    Duration,
}

impl ValueType {
    pub fn as_str(self) -> &'static str {
        match self {
            ValueType::Int => "int",
            ValueType::Float => "float",
            ValueType::Decimal => "decimal",
            ValueType::String => "string",
            ValueType::Bool => "bool",
            ValueType::Enum => "enum",
            ValueType::Object => "object",
            ValueType::Array => "array",
            ValueType::Duration => "duration",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSchemaRow {
    pub key: String,
    pub category: String,
    pub subcategory: Option<String>,
    pub value_type: String,
    pub json_schema: serde_json::Value,
    pub default_value: serde_json::Value,
    pub unit: Option<String>,
    pub description: String,
    pub ui_widget: Option<String>,
    pub requires_restart: bool,
    pub is_secret_ref: bool,
    pub sensitivity: String,
    pub deprecated_at: Option<DateTime<Utc>>,
    pub introduced_in: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigValueRow {
    pub id: i64,
    pub key: String,
    pub scope_id: i64,
    pub value: serde_json::Value,
    pub version: i32,
    pub enabled: bool,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Option<Uuid>,
}

/// Optional flags for a `set` operation.
#[derive(Debug, Clone, Default)]
pub struct SetOptions {
    /// Optimistic-lock guard. If `Some`, the update is rejected unless
    /// the row's current `version` matches.
    pub expected_version: Option<i32>,
    /// Schedule activation. `None` = effective immediately.
    pub valid_from: Option<DateTime<Utc>>,
    /// Auto-expire. `None` = never.
    pub valid_until: Option<DateTime<Utc>>,
    /// Group multi-key edits in audit log.
    pub correlation: Option<Uuid>,
}

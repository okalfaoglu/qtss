//! Sürümlü `audit_log.details` JSON şeması (RBAC / denetim derinliği, §9.1).

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// `details.schema_version` — uyumsuz sürümde istemciler ayrıştırmayı atlayabilir.
pub const AUDIT_DETAILS_SCHEMA_VERSION: u32 = 1;

pub mod kind {
    pub const USER_PERMISSIONS_REPLACE: &str = "user_permissions_replace";
    /// Genel HTTP mutasyon satırı (`audit_http_middleware`); kolonlarla aynı bilgi `details` zarfında.
    pub const HTTP_MUTATION: &str = "http_mutation";
}

#[derive(Debug, Serialize)]
pub struct HttpMutationDetailsV1 {
    pub schema_version: u32,
    pub kind: &'static str,
    pub http_method: String,
    pub path: String,
    pub status_code: u16,
}

impl HttpMutationDetailsV1 {
    pub fn new(http_method: &str, path: &str, status_code: u16) -> Self {
        Self {
            schema_version: AUDIT_DETAILS_SCHEMA_VERSION,
            kind: kind::HTTP_MUTATION,
            http_method: http_method.to_string(),
            path: path.to_string(),
            status_code,
        }
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("audit details JSON")
    }
}

#[derive(Debug, Serialize)]
pub struct UserPermissionsReplaceDetailsV1 {
    pub schema_version: u32,
    pub kind: &'static str,
    pub target_user_id: Uuid,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

impl UserPermissionsReplaceDetailsV1 {
    pub fn new(target_user_id: Uuid, before: Vec<String>, after: Vec<String>) -> Self {
        Self {
            schema_version: AUDIT_DETAILS_SCHEMA_VERSION,
            kind: kind::USER_PERMISSIONS_REPLACE,
            target_user_id,
            before,
            after,
        }
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("audit details JSON")
    }
}

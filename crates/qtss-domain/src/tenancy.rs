use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Şimdilik tek kurum; ileride çoklu kiracı için doğrudan genişletilebilir.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrganizationId(pub Uuid);

impl OrganizationId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }
}

/// İstek bağlamı: API ve worker’lar taşır.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantContext {
    pub org_id: OrganizationId,
}

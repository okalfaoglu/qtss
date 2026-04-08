//! Role -> permission mapping. Static so adding/removing a permission
//! is a code review, not a SQL UPDATE on a live system.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoleName {
    Admin,
    Trader,
    Viewer,
}

impl RoleName {
    pub fn as_str(&self) -> &'static str {
        match self {
            RoleName::Admin => "admin",
            RoleName::Trader => "trader",
            RoleName::Viewer => "viewer",
        }
    }

    pub fn from_db(name: &str) -> Option<Self> {
        match name {
            "admin" => Some(RoleName::Admin),
            "trader" => Some(RoleName::Trader),
            "viewer" => Some(RoleName::Viewer),
            _ => None,
        }
    }
}

/// Coarse-grained permissions. Each downstream crate (config editor,
/// intent approval, ...) checks for the specific variant it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    ConfigRead,
    ConfigWrite,
    IntentSubmit,
    IntentApprove,
    KillSwitchTrip,
    UserManage,
    SecretsManage,
    ViewDashboards,
}

/// Permission set for a given role. Used by middleware that wraps
/// authenticated handlers — the handler asks `permissions_for(role)`
/// once and short-circuits with `PermissionDenied` if the required
/// variant is missing.
pub fn permissions_for(role: RoleName) -> &'static [Permission] {
    match role {
        RoleName::Admin => &[
            Permission::ConfigRead,
            Permission::ConfigWrite,
            Permission::IntentSubmit,
            Permission::IntentApprove,
            Permission::KillSwitchTrip,
            Permission::UserManage,
            Permission::SecretsManage,
            Permission::ViewDashboards,
        ],
        RoleName::Trader => &[
            Permission::ConfigRead,
            Permission::IntentSubmit,
            Permission::IntentApprove,
            Permission::KillSwitchTrip,
            Permission::ViewDashboards,
        ],
        RoleName::Viewer => &[Permission::ConfigRead, Permission::ViewDashboards],
    }
}

//! `/v2/users` wire types -- Faz 5 Adim (l).
//!
//! The User & Roles card lists every member of the org with their
//! role keys and granted permissions. The DTO drops `password_hash`
//! (obviously) and any audit columns the table doesn't actually need
//! to render -- the React side gets exactly the columns the form will
//! show.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One row in the User & Roles table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserCard {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub created_at: DateTime<Utc>,
    /// Role keys joined from the `user_roles` / `roles` tables.
    pub roles: Vec<String>,
    /// Fine-grained permissions from `user_permissions`.
    pub permissions: Vec<String>,
}

/// Whole `/v2/users` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsersView {
    pub generated_at: DateTime<Utc>,
    pub users: Vec<UserCard>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let view = UsersView {
            generated_at: Utc::now(),
            users: vec![UserCard {
                id: "00000000-0000-0000-0000-000000000001".into(),
                email: "ops@example.com".into(),
                display_name: Some("Ops".into()),
                is_admin: false,
                created_at: Utc::now(),
                roles: vec!["dashboard".into(), "ops".into()],
                permissions: vec!["qtss:orders.write".into()],
            }],
        };
        let j = serde_json::to_string(&view).unwrap();
        let back: UsersView = serde_json::from_str(&j).unwrap();
        assert_eq!(back.users.len(), 1);
        assert_eq!(back.users[0].roles.len(), 2);
        assert!(!j.contains("password_hash"));
    }
}

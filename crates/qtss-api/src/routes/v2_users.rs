#![allow(dead_code)]
//! `GET /v2/users` -- Faz 5 Adim (l).
//!
//! Read-only User & Roles viewer for the org. Admin role gates the
//! endpoint -- non-admins should not see other accounts. Mutations
//! stay on the existing /admin/users-permissions routes.

use std::collections::HashMap;

use axum::extract::{Extension, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use qtss_gui_api::{UserCard, UsersView};

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct UsersQuery {
    pub limit: Option<i64>,
}

pub fn v2_users_router() -> Router<SharedState> {
    Router::new().route("/v2/users", get(get_users))
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    display_name: Option<String>,
    is_admin: bool,
    created_at: DateTime<Utc>,
}

async fn get_users(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<UsersQuery>,
) -> Result<Json<UsersView>, ApiError> {
    let org_id = Uuid::parse_str(claims.org_id.trim())
        .map_err(|_| ApiError::bad_request("invalid token org_id"))?;
    let limit = q
        .limit
        .unwrap_or_else(|| env_int("QTSS_V2_USERS_LIMIT", 200))
        .clamp(1, 1_000);

    // 1) Users in this org.
    let users: Vec<UserRow> = sqlx::query_as::<_, UserRow>(
        r#"SELECT id, email, display_name, is_admin, created_at
           FROM users
           WHERE org_id = $1
           ORDER BY email ASC
           LIMIT $2"#,
    )
    .bind(org_id)
    .bind(limit)
    .fetch_all(&st.pool)
    .await
    .map_err(qtss_storage::error::StorageError::from)?;

    // 2) Roles per user (role.key joined through user_roles).
    let role_rows: Vec<(Uuid, String)> = sqlx::query_as::<_, (Uuid, String)>(
        r#"SELECT ur.user_id, r.key
           FROM user_roles ur
           JOIN roles r ON r.id = ur.role_id
           JOIN users u ON u.id = ur.user_id
           WHERE u.org_id = $1
           ORDER BY ur.user_id, r.key"#,
    )
    .bind(org_id)
    .fetch_all(&st.pool)
    .await
    .map_err(qtss_storage::error::StorageError::from)?;
    let mut roles_by_user: HashMap<Uuid, Vec<String>> = HashMap::new();
    for (uid, key) in role_rows {
        roles_by_user.entry(uid).or_default().push(key);
    }

    // 3) Permissions per user.
    let perm_rows: Vec<(Uuid, String)> = sqlx::query_as::<_, (Uuid, String)>(
        r#"SELECT up.user_id, up.permission
           FROM user_permissions up
           JOIN users u ON u.id = up.user_id
           WHERE u.org_id = $1
           ORDER BY up.user_id, up.permission"#,
    )
    .bind(org_id)
    .fetch_all(&st.pool)
    .await
    .map_err(qtss_storage::error::StorageError::from)?;
    let mut perms_by_user: HashMap<Uuid, Vec<String>> = HashMap::new();
    for (uid, p) in perm_rows {
        perms_by_user.entry(uid).or_default().push(p);
    }

    let cards: Vec<UserCard> = users
        .into_iter()
        .map(|u| UserCard {
            id: u.id.to_string(),
            email: u.email,
            display_name: u.display_name,
            is_admin: u.is_admin,
            created_at: u.created_at,
            roles: roles_by_user.remove(&u.id).unwrap_or_default(),
            permissions: perms_by_user.remove(&u.id).unwrap_or_default(),
        })
        .collect();

    Ok(Json(UsersView {
        generated_at: Utc::now(),
        users: cards,
    }))
}

fn env_int(key: &str, default: i64) -> i64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parses_limit() {
        let q: UsersQuery = serde_urlencoded::from_str("limit=50").unwrap();
        assert_eq!(q.limit, Some(50));
    }
}

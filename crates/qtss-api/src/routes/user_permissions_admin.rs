//! Kullanıcı başına `user_permissions` (admin, aynı `org_id`).

use std::collections::BTreeSet;

use axum::extract::{Extension, Path, State};
use axum::http::HeaderMap;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use qtss_storage::{insert_http_audit, AuditHttpRow};

use crate::audit_event::UserPermissionsReplaceDetailsV1;
use crate::audit_http::http_audit_enabled;
use crate::oauth::rbac::is_known_qtss_permission;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct ReplacePermissionsBody {
    pub permissions: Vec<String>,
}

pub fn user_permissions_admin_router() -> Router<SharedState> {
    Router::new().route(
        "/users/{user_id}/permissions",
        get(get_user_permissions).put(put_user_permissions),
    )
}

async fn ensure_same_org(
    claims: &AccessClaims,
    st: &SharedState,
    target_user_id: Uuid,
) -> Result<(), String> {
    let caller_org = Uuid::parse_str(claims.org_id.trim())
        .map_err(|_| "geçersiz token org_id".to_string())?;
    let Some(target_org) = st
        .user_permissions
        .org_id_for_user(target_user_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Err("kullanıcı bulunamadı".to_string());
    };
    if caller_org != target_org {
        return Err("hedef kullanıcı aynı kuruma ait değil".to_string());
    }
    Ok(())
}

fn validate_permissions(perms: &[String]) -> Result<(), String> {
    for p in perms {
        if !is_known_qtss_permission(p) {
            return Err(format!("geçersiz permission: {p}"));
        }
    }
    Ok(())
}

async fn get_user_permissions(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<Vec<String>>, String> {
    ensure_same_org(&claims, &st, user_id).await?;
    let rows = st
        .user_permissions
        .list_for_user(user_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(rows))
}

async fn put_user_permissions(
    headers: HeaderMap,
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(user_id): Path<Uuid>,
    Json(body): Json<ReplacePermissionsBody>,
) -> Result<Json<Vec<String>>, String> {
    ensure_same_org(&claims, &st, user_id).await?;
    validate_permissions(&body.permissions)?;
    let unique: Vec<String> = body
        .permissions
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let before = st
        .user_permissions
        .list_for_user(user_id)
        .await
        .map_err(|e| e.to_string())?;
    st.user_permissions
        .replace_for_user(user_id, &unique)
        .await
        .map_err(|e| e.to_string())?;

    if http_audit_enabled() {
        let request_id = headers
            .get("x-request-id")
            .and_then(|h| h.to_str().ok())
            .map(String::from);
        let actor_user_id = Uuid::parse_str(claims.sub.trim()).ok();
        let org_id = Uuid::parse_str(claims.org_id.trim()).ok();
        let path = format!("/api/v1/users/{user_id}/permissions");
        let details = UserPermissionsReplaceDetailsV1::new(user_id, before, unique.clone()).to_value();
        let pool = st.pool.clone();
        let roles = claims.roles.clone();
        tokio::spawn(async move {
            let row = AuditHttpRow {
                request_id,
                user_id: actor_user_id,
                org_id,
                method: "PUT".into(),
                path,
                status_code: 200,
                roles,
                details: Some(details),
            };
            if let Err(e) = insert_http_audit(&pool, row).await {
                tracing::warn!(error = %e, "audit_log user_permissions yazılamadı");
            }
        });
    }

    Ok(Json(unique))
}

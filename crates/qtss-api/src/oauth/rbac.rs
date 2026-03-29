//! Rol tabanlı erişim: JWT `roles` (`roles.key`) ve ilk aşama `permissions` (rol → yetenek haritası).

use std::collections::BTreeSet;

use axum::extract::{Extension, Request};
use axum::middleware::Next;
use axum::response::Response;

use super::error::Forbidden;
use super::AccessClaims;

/// Dashboard salt okunur ve üstü.
pub const QTSS_PERM_READ: &str = "qtss:read";
/// Katalog senkronu, emir/piyasa yazma ve benzeri operasyonel uçlar.
pub const QTSS_PERM_OPS: &str = "qtss:ops";
/// Yapılandırma, mutabakat tetikleme ve diğer yönetim uçları.
pub const QTSS_PERM_ADMIN: &str = "qtss:admin";
/// Salt okunur denetim günlüğü (`GET /api/v1/audit/recent`); `user_permissions` veya JWT claim ile verilebilir.
pub const QTSS_PERM_AUDIT_READ: &str = "qtss:audit:read";

/// Maps DB role keys to coarse JWT `permissions`. Unknown roles yield no permissions (same as before for custom keys).
pub fn permissions_for_roles(roles: &[String]) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for r in roles {
        match r.as_str() {
            "admin" => {
                set.insert(QTSS_PERM_READ.to_string());
                set.insert(QTSS_PERM_OPS.to_string());
                set.insert(QTSS_PERM_ADMIN.to_string());
                set.insert(QTSS_PERM_AUDIT_READ.to_string());
            }
            "trader" => {
                set.insert(QTSS_PERM_READ.to_string());
                set.insert(QTSS_PERM_OPS.to_string());
            }
            "analyst" | "viewer" => {
                set.insert(QTSS_PERM_READ.to_string());
            }
            _ => {}
        }
    }
    set.into_iter().collect()
}

pub fn has_permission(claims: &AccessClaims, permission: &str) -> bool {
    claims.permissions.iter().any(|p| p == permission)
}

/// Legacy access tokens without `permissions`: derive from `roles` once per request.
pub fn normalize_claims(mut claims: AccessClaims) -> AccessClaims {
    if claims.permissions.is_empty() {
        claims.permissions = permissions_for_roles(&claims.roles);
    }
    claims
}

pub fn is_known_qtss_permission(p: &str) -> bool {
    matches!(
        p,
        QTSS_PERM_READ | QTSS_PERM_OPS | QTSS_PERM_ADMIN | QTSS_PERM_AUDIT_READ
    )
}

/// JWT’deki (rol/claim) izinlere `user_permissions` satırlarını ekler (bilinmeyen dizgiler yok sayılır).
pub fn merge_jwt_with_db_permissions(mut claims: AccessClaims, db_perms: &[String]) -> AccessClaims {
    let mut set: BTreeSet<String> = claims.permissions.iter().cloned().collect();
    for p in db_perms {
        if is_known_qtss_permission(p) {
            set.insert(p.clone());
        }
    }
    claims.permissions = set.into_iter().collect();
    claims
}

fn allows_dashboard(claims: &AccessClaims) -> bool {
    has_permission(claims, QTSS_PERM_READ)
        || has_permission(claims, QTSS_PERM_OPS)
        || has_permission(claims, QTSS_PERM_ADMIN)
}

fn allows_ops(claims: &AccessClaims) -> bool {
    has_permission(claims, QTSS_PERM_OPS) || has_permission(claims, QTSS_PERM_ADMIN)
}

fn allows_admin(claims: &AccessClaims) -> bool {
    has_permission(claims, QTSS_PERM_ADMIN)
}

fn allows_audit_read(claims: &AccessClaims) -> bool {
    has_permission(claims, QTSS_PERM_ADMIN) || has_permission(claims, QTSS_PERM_AUDIT_READ)
}

/// `qtss:admin` veya `qtss:audit:read` (salt okunur audit listesi).
pub async fn require_audit_read(
    Extension(claims): Extension<AccessClaims>,
    req: Request,
    next: Next,
) -> Result<Response, Forbidden> {
    if !allows_audit_read(&claims) {
        return Err(Forbidden::new(
            "denetim günlüğü için qtss:admin veya qtss:audit:read gerekli",
        ));
    }
    Ok(next.run(req).await)
}

/// Yalnızca `qtss:admin` (veya eşdeğer rol kaynaklı üretilmiş izinler).
pub async fn require_admin(
    Extension(claims): Extension<AccessClaims>,
    req: Request,
    next: Next,
) -> Result<Response, Forbidden> {
    if !allows_admin(&claims) {
        return Err(Forbidden::new("config yönetimi için admin rolü gerekli"));
    }
    Ok(next.run(req).await)
}

/// Dashboard ve salt okunur piyasa verisi: viewer ve üzeri.
pub async fn require_dashboard_roles(
    Extension(claims): Extension<AccessClaims>,
    req: Request,
    next: Next,
) -> Result<Response, Forbidden> {
    if !allows_dashboard(&claims) {
        return Err(Forbidden::new(
            "dashboard veya piyasa verisi için uygun rol gerekli",
        ));
    }
    Ok(next.run(req).await)
}

/// Katalog senkronu ve operasyonel işlemler.
pub async fn require_ops_roles(
    Extension(claims): Extension<AccessClaims>,
    req: Request,
    next: Next,
) -> Result<Response, Forbidden> {
    if !allows_ops(&claims) {
        return Err(Forbidden::new("bu işlem için admin veya trader rolü gerekli"));
    }
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::AccessClaims;

    #[test]
    fn permissions_admin_includes_all() {
        let roles = vec!["admin".to_string()];
        let p = permissions_for_roles(&roles);
        assert!(p.contains(&QTSS_PERM_READ.to_string()));
        assert!(p.contains(&QTSS_PERM_OPS.to_string()));
        assert!(p.contains(&QTSS_PERM_ADMIN.to_string()));
        assert!(p.contains(&QTSS_PERM_AUDIT_READ.to_string()));
    }

    #[test]
    fn permissions_trader_is_read_and_ops() {
        let roles = vec!["trader".to_string()];
        let p = permissions_for_roles(&roles);
        assert!(p.contains(&QTSS_PERM_READ.to_string()));
        assert!(p.contains(&QTSS_PERM_OPS.to_string()));
        assert!(!p.contains(&QTSS_PERM_ADMIN.to_string()));
    }

    #[test]
    fn permissions_viewer_read_only() {
        let roles = vec!["viewer".to_string()];
        let p = permissions_for_roles(&roles);
        assert_eq!(p, vec![QTSS_PERM_READ.to_string()]);
    }

    #[test]
    fn merge_db_adds_ops_to_viewer() {
        let claims = normalize_claims(AccessClaims {
            sub: "00000000-0000-0000-0000-000000000001".into(),
            org_id: "00000000-0000-0000-0000-000000000002".into(),
            roles: vec!["viewer".into()],
            permissions: vec![],
            azp: "x".into(),
            exp: 0,
            iat: 0,
            aud: "a".into(),
            iss: "i".into(),
        });
        let merged = merge_jwt_with_db_permissions(
            claims,
            &[QTSS_PERM_OPS.to_string(), "unknown:bad".into()],
        );
        assert!(merged.permissions.contains(&QTSS_PERM_READ.to_string()));
        assert!(merged.permissions.contains(&QTSS_PERM_OPS.to_string()));
        assert_eq!(merged.permissions.len(), 2);
    }

    #[test]
    fn known_permission_includes_audit_read() {
        assert!(is_known_qtss_permission(QTSS_PERM_AUDIT_READ));
    }
}

//! Rol tabanlı erişim: JWT içindeki `roles` alanı (`roles.key` ile eşleşir).

use axum::extract::{Extension, Request};
use axum::middleware::Next;
use axum::response::Response;

use super::error::Forbidden;
use super::AccessClaims;

fn has_any_role(claims: &AccessClaims, allowed: &[&str]) -> bool {
    claims
        .roles
        .iter()
        .any(|r| allowed.iter().any(|a| a == &r.as_str()))
}

/// Yalnızca `admin`.
pub async fn require_admin(
    Extension(claims): Extension<AccessClaims>,
    req: Request,
    next: Next,
) -> Result<Response, Forbidden> {
    if !has_any_role(&claims, &["admin"]) {
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
    if !has_any_role(&claims, &["admin", "trader", "analyst", "viewer"]) {
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
    if !has_any_role(&claims, &["admin", "trader"]) {
        return Err(Forbidden::new("bu işlem için admin veya trader rolü gerekli"));
    }
    Ok(next.run(req).await)
}

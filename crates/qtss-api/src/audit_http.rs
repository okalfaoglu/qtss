//! Mutasyon istekleri için `audit_log` satırı.
//!
//! Yalnızca ortam değişkeni tam olarak `1` iken yazılır; tanımsız veya `0` / boş / başka değer = kapalı (opt-in).

use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;
use uuid::Uuid;

use qtss_storage::{insert_http_audit, AuditHttpRow};

use crate::oauth::AccessClaims;
use crate::state::SharedState;

pub fn http_audit_enabled() -> bool {
    matches!(std::env::var("QTSS_AUDIT_HTTP").ok().as_deref(), Some("1"))
}

/// `PUT .../users/{id}/permissions` için ayrıntılı satır handler yazar; çift kayıt önlenir.
fn skip_generic_http_audit(method: &Method, path: &str) -> bool {
    method == Method::PUT
        && path.starts_with("/api/v1/users/")
        && path.ends_with("/permissions")
}

pub async fn audit_http_middleware(
    State(st): State<SharedState>,
    req: Request,
    next: Next,
) -> Response {
    if !http_audit_enabled() {
        return next.run(req).await;
    }

    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .map(String::from);
    let claims = req.extensions().get::<AccessClaims>().cloned();

    let resp = next.run(req).await;
    let status = resp.status().as_u16();

    if !matches!(
        method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        return resp;
    }
    if !path.starts_with("/api/v1/") {
        return resp;
    }
    if skip_generic_http_audit(&method, &path) {
        return resp;
    }

    if let Some(c) = claims {
        let user_id = Uuid::parse_str(&c.sub).ok();
        let org_id = Uuid::parse_str(&c.org_id).ok();
        let roles = c.roles.clone();
        let pool = st.pool.clone();
        let row = AuditHttpRow {
            request_id,
            user_id,
            org_id,
            method: method.to_string(),
            path,
            status_code: status,
            roles,
            details: None,
        };
        tokio::spawn(async move {
            if let Err(e) = insert_http_audit(&pool, row).await {
                tracing::warn!(error = %e, "audit_log yazılamadı");
            }
        });
    }

    resp
}

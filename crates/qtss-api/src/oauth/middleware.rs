use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;
use uuid::Uuid;

use crate::oauth::error::UnauthorizedBearer;
use crate::oauth::rbac::{merge_jwt_with_db_permissions, normalize_claims};
use crate::state::SharedState;

fn merge_db_permissions_from_table() -> bool {
    match std::env::var("QTSS_JWT_MERGE_DB_PERMISSIONS") {
        Err(_) => true,
        Ok(s) => {
            let t = s.trim();
            !(t == "0"
                || t.eq_ignore_ascii_case("false")
                || t.eq_ignore_ascii_case("no")
                || t.eq_ignore_ascii_case("off"))
        }
    }
}

pub async fn require_jwt(
    State(state): State<SharedState>,
    mut req: Request,
    next: Next,
) -> Result<Response, UnauthorizedBearer> {
    let jwt = state.jwt.as_ref().ok_or(UnauthorizedBearer)?;
    let hdr = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(UnauthorizedBearer)?;
    let token = hdr
        .strip_prefix("Bearer ")
        .ok_or(UnauthorizedBearer)?
        .trim();
    if token.is_empty() {
        return Err(UnauthorizedBearer);
    }
    let claims = jwt.verify(token).map_err(|_| UnauthorizedBearer)?;
    let mut claims = normalize_claims(claims);
    if merge_db_permissions_from_table() {
        if let Ok(uid) = Uuid::parse_str(claims.sub.trim()) {
            match state.user_permissions.list_for_user(uid).await {
                Ok(db) => {
                    claims = merge_jwt_with_db_permissions(claims, &db);
                }
                Err(e) => {
                    tracing::warn!(
                        %e,
                        "require_jwt: user_permissions list failed; continuing with JWT-only"
                    );
                }
            }
        }
    }
    // `Extension<AccessClaims>` çıkarıcısı `extensions` içinde doğrudan `AccessClaims` arar.
    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
}

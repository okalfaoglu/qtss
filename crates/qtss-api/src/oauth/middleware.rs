use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;

use crate::oauth::error::UnauthorizedBearer;
use crate::oauth::rbac::normalize_claims;
use crate::state::SharedState;

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
    let claims = normalize_claims(claims);
    // `Extension<AccessClaims>` çıkarıcısı `extensions` içinde doğrudan `AccessClaims` arar.
    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
}

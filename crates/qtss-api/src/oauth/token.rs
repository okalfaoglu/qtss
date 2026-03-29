use axum::extract::State;
use axum::http::{header::CONTENT_TYPE, HeaderMap};
use axum::Json;
use bytes::Bytes;
use chrono::Utc;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use uuid::Uuid;

use crate::oauth::error::{
    invalid_client, invalid_grant, invalid_request, server_error, unsupported_grant_type, OAuthErr,
};
use crate::oauth::jwt::JwtIssuer;
use crate::oauth::rbac::permissions_for_roles;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct TokenRequest {
    pub grant_type: String,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Debug, FromRow)]
struct OauthClientRow {
    id: Uuid,
    client_secret_hash: String,
    allowed_grant_types: Vec<String>,
    service_user_id: Option<Uuid>,
}

#[derive(Debug, FromRow)]
struct UserAuthRow {
    id: Uuid,
    org_id: Uuid,
    password_hash: String,
}

#[derive(Debug, FromRow)]
struct OrgIdRow {
    org_id: Uuid,
}

#[derive(Debug, FromRow)]
struct RefreshRow {
    user_id: Uuid,
    expires_at: chrono::DateTime<chrono::Utc>,
    revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn parse_token_request(headers: &HeaderMap, body: &Bytes) -> Result<TokenRequest, String> {
    let ct = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if ct.contains("application/json") {
        serde_json::from_slice(body).map_err(|e| e.to_string())
    } else {
        serde_urlencoded::from_bytes(body).map_err(|e| e.to_string())
    }
}

pub async fn oauth_token(
    State(st): State<SharedState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<TokenResponse>, OAuthErr> {
    let req = parse_token_request(&headers, &body).map_err(|e| invalid_request(format!("gövde: {e}")))?;

    let jwt = st.jwt.as_ref().ok_or_else(|| server_error("JWT yapılandırılmamış".to_string()))?;

    let client_id = req.client_id.as_deref().ok_or_else(|| invalid_request("client_id gerekli"))?;
    let client_secret = req
        .client_secret
        .as_deref()
        .ok_or_else(|| invalid_request("client_secret gerekli"))?;

    let grant = req.grant_type.as_str();
    match grant {
        "password" => token_password(&st, jwt, client_id, client_secret, &req).await,
        "client_credentials" => token_client_credentials(&st, jwt, client_id, client_secret).await,
        "refresh_token" => token_refresh(&st, jwt, client_id, client_secret, &req).await,
        _ => Err(unsupported_grant_type()),
    }
}

async fn token_password(
    st: &SharedState,
    jwt: &JwtIssuer,
    client_id: &str,
    client_secret: &str,
    req: &TokenRequest,
) -> Result<Json<TokenResponse>, OAuthErr> {
    let row = fetch_oauth_client(st, client_id).await?;
    if !row.allowed_grant_types.iter().any(|g| g == "password") {
        return Err(invalid_grant("password grant bu istemci için kapalı"));
    }
    verify_client_secret(&row.client_secret_hash, client_secret).map_err(|_| {
        invalid_client("istemci kimlik doğrulaması başarısız")
    })?;

    let email = req
        .username
        .as_deref()
        .ok_or_else(|| invalid_request("username (e-posta) gerekli"))?;
    let password = req
        .password
        .as_deref()
        .ok_or_else(|| invalid_request("password gerekli"))?;

    let user: UserAuthRow = sqlx::query_as(
        "SELECT id, org_id, password_hash FROM users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(&st.pool)
    .await
    .map_err(|e| invalid_grant(format!("db: {e}")))?
    .ok_or_else(|| invalid_grant("kullanıcı bulunamadı"))?;

    verify_password_hash(&user.password_hash, password).map_err(|_| {
        invalid_grant("geçersiz kimlik bilgisi")
    })?;

    let roles = load_roles(st, user.id).await.map_err(invalid_grant)?;

    finish_token_issue(
        st,
        jwt,
        row.id,
        user.id,
        user.org_id,
        roles,
        client_id,
        req.scope.clone(),
    )
    .await
}

async fn token_client_credentials(
    st: &SharedState,
    jwt: &JwtIssuer,
    client_id: &str,
    client_secret: &str,
) -> Result<Json<TokenResponse>, OAuthErr> {
    let row = fetch_oauth_client(st, client_id).await?;
    if !row
        .allowed_grant_types
        .iter()
        .any(|g| g == "client_credentials")
    {
        return Err(invalid_grant(
            "client_credentials bu istemci için kapalı",
        ));
    }
    verify_client_secret(&row.client_secret_hash, client_secret).map_err(|_| {
        invalid_client("istemci kimlik doğrulaması başarısız")
    })?;

    let suid = row
        .service_user_id
        .ok_or_else(|| invalid_grant("service_user_id tanımlı değil"))?;

    let org: OrgIdRow = sqlx::query_as("SELECT org_id FROM users WHERE id = $1")
        .bind(suid)
        .fetch_optional(&st.pool)
        .await
        .map_err(|e| invalid_grant(format!("db: {e}")))?
        .ok_or_else(|| invalid_grant("servis kullanıcısı yok"))?;

    let roles = load_roles(st, suid).await.map_err(invalid_grant)?;

    finish_token_issue(
        st,
        jwt,
        row.id,
        suid,
        org.org_id,
        roles,
        client_id,
        None,
    )
    .await
}

async fn token_refresh(
    st: &SharedState,
    jwt: &JwtIssuer,
    client_id: &str,
    client_secret: &str,
    req: &TokenRequest,
) -> Result<Json<TokenResponse>, OAuthErr> {
    let row = fetch_oauth_client(st, client_id).await?;
    if !row
        .allowed_grant_types
        .iter()
        .any(|g| g == "refresh_token")
    {
        return Err(invalid_grant(
            "refresh_token bu istemci için kapalı",
        ));
    }
    verify_client_secret(&row.client_secret_hash, client_secret).map_err(|_| {
        invalid_client("istemci kimlik doğrulaması başarısız")
    })?;

    let raw = req
        .refresh_token
        .as_deref()
        .ok_or_else(|| invalid_request("refresh_token gerekli"))?;
    let hash = hash_token(raw);

    let rt: RefreshRow = sqlx::query_as(
        r#"SELECT user_id, expires_at, revoked_at FROM refresh_tokens
           WHERE token_hash = $1 AND client_uuid = $2"#,
    )
    .bind(&hash)
    .bind(row.id)
    .fetch_optional(&st.pool)
    .await
    .map_err(|e| invalid_grant(format!("db: {e}")))?
    .ok_or_else(|| invalid_grant("yenileme belirteci geçersiz"))?;

    if rt.revoked_at.is_some() {
        return Err(invalid_grant("belirteç iptal edilmiş"));
    }
    if rt.expires_at < Utc::now() {
        return Err(invalid_grant("belirteç süresi dolmuş"));
    }

    sqlx::query("UPDATE refresh_tokens SET revoked_at = now() WHERE token_hash = $1")
        .bind(&hash)
        .execute(&st.pool)
        .await
        .map_err(|e| invalid_grant(format!("db: {e}")))?;

    let org: OrgIdRow = sqlx::query_as("SELECT org_id FROM users WHERE id = $1")
        .bind(rt.user_id)
        .fetch_optional(&st.pool)
        .await
        .map_err(|e| invalid_grant(format!("db: {e}")))?
        .ok_or_else(|| invalid_grant("kullanıcı yok"))?;

    let roles = load_roles(st, rt.user_id).await.map_err(invalid_grant)?;

    finish_token_issue(
        st,
        jwt,
        row.id,
        rt.user_id,
        org.org_id,
        roles,
        client_id,
        req.scope.clone(),
    )
    .await
}

async fn fetch_oauth_client(st: &SharedState, client_id: &str) -> Result<OauthClientRow, OAuthErr> {
    sqlx::query_as::<_, OauthClientRow>(
        r#"SELECT id, client_secret_hash, allowed_grant_types, service_user_id
           FROM oauth_clients WHERE client_id = $1"#,
    )
    .bind(client_id)
    .fetch_optional(&st.pool)
    .await
    .map_err(|e| invalid_client(format!("db: {e}")))?
    .ok_or_else(|| invalid_client("bilinmeyen client_id"))
}

#[derive(Debug, FromRow)]
struct RoleKey {
    key: String,
}

async fn load_roles(st: &SharedState, user_id: Uuid) -> Result<Vec<String>, String> {
    let rows: Vec<RoleKey> = sqlx::query_as(
        r#"SELECT r.key FROM roles r
           INNER JOIN user_roles ur ON ur.role_id = r.id
           WHERE ur.user_id = $1"#,
    )
    .bind(user_id)
    .fetch_all(&st.pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|r| r.key).collect())
}

#[allow(clippy::too_many_arguments)]
async fn finish_token_issue(
    st: &SharedState,
    jwt: &JwtIssuer,
    client_uuid: Uuid,
    user_id: Uuid,
    org_id: Uuid,
    roles: Vec<String>,
    client_id: &str,
    scope: Option<String>,
) -> Result<Json<TokenResponse>, OAuthErr> {
    let permissions = permissions_for_roles(&roles);
    let access = jwt
        .issue_access_token(user_id, org_id, roles, permissions, client_id)
        .map_err(|e| server_error(e.to_string()))?;

    let refresh_ttl = st.refresh_ttl_secs;
    let raw_refresh = random_token();
    let refresh_hash = hash_token(&raw_refresh);
    let exp = Utc::now() + chrono::Duration::seconds(refresh_ttl);

    sqlx::query(
        r#"INSERT INTO refresh_tokens (client_uuid, user_id, token_hash, expires_at)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(client_uuid)
    .bind(user_id)
    .bind(&refresh_hash)
    .bind(exp)
    .execute(&st.pool)
    .await
    .map_err(|e| invalid_grant(format!("refresh kayıt: {e}")))?;

    Ok(Json(TokenResponse {
        access_token: access,
        token_type: "Bearer",
        expires_in: jwt.access_ttl_secs,
        refresh_token: Some(raw_refresh),
        scope,
    }))
}

fn hash_token(raw: &str) -> String {
    let mut h = Sha256::new();
    h.update(raw.as_bytes());
    format!("{:x}", h.finalize())
}

fn random_token() -> String {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}

fn verify_client_secret(phc: &str, plain: &str) -> Result<(), ()> {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    let parsed = PasswordHash::new(phc).map_err(|_| ())?;
    argon2::Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .map_err(|_| ())
}

fn verify_password_hash(phc: &str, plain: &str) -> Result<(), ()> {
    verify_client_secret(phc, plain)
}

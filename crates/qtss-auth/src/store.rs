//! Persistence layer for users, role assignments, and sessions. The PG
//! store is the production target; the in-memory store backs unit tests
//! that exercise login + permission checks without a DB round-trip.

use crate::error::{AuthError, AuthResult};
use crate::password::{hash_password, verify_password};
use crate::roles::{permissions_for, Permission, RoleName};
use crate::session::{Session, SessionToken};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub id: i64,
    pub username: String,
    pub email: Option<String>,
    pub is_active: bool,
    pub roles: Vec<RoleName>,
}

impl UserRecord {
    pub fn has_permission(&self, perm: Permission) -> bool {
        self.roles
            .iter()
            .any(|r| permissions_for(*r).contains(&perm))
    }
}

#[async_trait]
pub trait AuthStore: Send + Sync {
    async fn create_user(
        &self,
        username: &str,
        email: Option<&str>,
        password: &str,
        roles: &[RoleName],
    ) -> AuthResult<UserRecord>;

    async fn login(
        &self,
        username: &str,
        password: &str,
        ttl: Duration,
    ) -> AuthResult<(UserRecord, Session)>;

    async fn validate_session(&self, token: SessionToken) -> AuthResult<UserRecord>;

    async fn revoke_session(&self, token: SessionToken) -> AuthResult<()>;
}

// ---------------------------------------------------------------------------
// PgAuthStore
// ---------------------------------------------------------------------------

pub struct PgAuthStore {
    pool: PgPool,
}

impl PgAuthStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn load_roles(&self, user_id: i64) -> AuthResult<Vec<RoleName>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT r.name FROM qtss_roles r
             JOIN qtss_user_roles ur ON ur.role_id = r.id
             WHERE ur.user_id = $1",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().filter_map(|r| RoleName::from_db(&r.0)).collect())
    }
}

#[async_trait]
impl AuthStore for PgAuthStore {
    async fn create_user(
        &self,
        username: &str,
        email: Option<&str>,
        password: &str,
        roles: &[RoleName],
    ) -> AuthResult<UserRecord> {
        let hash = hash_password(password)?;
        let mut tx = self.pool.begin().await?;
        let user_row: (i64,) = sqlx::query_as(
            "INSERT INTO qtss_users (username, email, password_hash) VALUES ($1, $2, $3)
             RETURNING id",
        )
        .bind(username)
        .bind(email)
        .bind(&hash)
        .fetch_one(&mut *tx)
        .await?;
        for role in roles {
            sqlx::query(
                "INSERT INTO qtss_user_roles (user_id, role_id)
                 SELECT $1, id FROM qtss_roles WHERE name = $2",
            )
            .bind(user_row.0)
            .bind(role.as_str())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(UserRecord {
            id: user_row.0,
            username: username.to_string(),
            email: email.map(|s| s.to_string()),
            is_active: true,
            roles: roles.to_vec(),
        })
    }

    async fn login(
        &self,
        username: &str,
        password: &str,
        ttl: Duration,
    ) -> AuthResult<(UserRecord, Session)> {
        let row: Option<(i64, String, Option<String>, bool, String)> = sqlx::query_as(
            "SELECT id, username, email, is_active, password_hash FROM qtss_users WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;

        let (id, username, email, is_active, password_hash) =
            row.ok_or_else(|| AuthError::UserNotFound(username.to_string()))?;
        if !is_active {
            return Err(AuthError::UserDisabled(username));
        }
        verify_password(password, &password_hash)?;

        let roles = self.load_roles(id).await?;
        let user = UserRecord {
            id,
            username,
            email,
            is_active,
            roles,
        };

        let token = SessionToken::new();
        let now = Utc::now();
        let expires_at = now + ttl;
        sqlx::query(
            "INSERT INTO qtss_sessions (id, user_id, issued_at, expires_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(token.0)
        .bind(user.id)
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;

        sqlx::query("UPDATE qtss_users SET last_login_at = $1 WHERE id = $2")
            .bind(now)
            .bind(user.id)
            .execute(&self.pool)
            .await?;

        let session = Session {
            token,
            user_id: user.id,
            issued_at: now,
            expires_at,
            revoked_at: None,
        };
        Ok((user, session))
    }

    async fn validate_session(&self, token: SessionToken) -> AuthResult<UserRecord> {
        let row: Option<(i64, DateTime<Utc>, Option<DateTime<Utc>>)> = sqlx::query_as(
            "SELECT user_id, expires_at, revoked_at FROM qtss_sessions WHERE id = $1",
        )
        .bind(token.0)
        .fetch_optional(&self.pool)
        .await?;

        let (user_id, expires_at, revoked_at) = row.ok_or(AuthError::SessionInvalid)?;
        let now = Utc::now();
        if revoked_at.is_some() || now >= expires_at {
            return Err(AuthError::SessionInvalid);
        }

        let urow: Option<(i64, String, Option<String>, bool)> = sqlx::query_as(
            "SELECT id, username, email, is_active FROM qtss_users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        let (id, username, email, is_active) = urow.ok_or(AuthError::SessionInvalid)?;
        if !is_active {
            return Err(AuthError::UserDisabled(username));
        }
        let roles = self.load_roles(id).await?;
        Ok(UserRecord {
            id,
            username,
            email,
            is_active,
            roles,
        })
    }

    async fn revoke_session(&self, token: SessionToken) -> AuthResult<()> {
        sqlx::query("UPDATE qtss_sessions SET revoked_at = NOW() WHERE id = $1 AND revoked_at IS NULL")
            .bind(token.0)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MemoryAuthStore — test-only
// ---------------------------------------------------------------------------

struct MemUser {
    record: UserRecord,
    password_hash: String,
}

#[derive(Default)]
pub struct MemoryAuthStore {
    users: Mutex<HashMap<String, MemUser>>, // by username
    sessions: Mutex<HashMap<uuid::Uuid, Session>>,
    next_id: Mutex<i64>,
}

impl MemoryAuthStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn alloc_id(&self) -> i64 {
        let mut g = self.next_id.lock().expect("id lock poisoned");
        *g += 1;
        *g
    }
}

#[async_trait]
impl AuthStore for MemoryAuthStore {
    async fn create_user(
        &self,
        username: &str,
        email: Option<&str>,
        password: &str,
        roles: &[RoleName],
    ) -> AuthResult<UserRecord> {
        let hash = hash_password(password)?;
        let id = self.alloc_id();
        let record = UserRecord {
            id,
            username: username.to_string(),
            email: email.map(|s| s.to_string()),
            is_active: true,
            roles: roles.to_vec(),
        };
        self.users.lock().expect("users lock poisoned").insert(
            username.to_string(),
            MemUser {
                record: record.clone(),
                password_hash: hash,
            },
        );
        Ok(record)
    }

    async fn login(
        &self,
        username: &str,
        password: &str,
        ttl: Duration,
    ) -> AuthResult<(UserRecord, Session)> {
        let (record, hash) = {
            let guard = self.users.lock().expect("users lock poisoned");
            let mu = guard
                .get(username)
                .ok_or_else(|| AuthError::UserNotFound(username.to_string()))?;
            if !mu.record.is_active {
                return Err(AuthError::UserDisabled(username.to_string()));
            }
            (mu.record.clone(), mu.password_hash.clone())
        };
        verify_password(password, &hash)?;

        let token = SessionToken::new();
        let now = Utc::now();
        let session = Session {
            token,
            user_id: record.id,
            issued_at: now,
            expires_at: now + ttl,
            revoked_at: None,
        };
        self.sessions
            .lock()
            .expect("sessions lock poisoned")
            .insert(token.0, session.clone());
        Ok((record, session))
    }

    async fn validate_session(&self, token: SessionToken) -> AuthResult<UserRecord> {
        let session = {
            let guard = self.sessions.lock().expect("sessions lock poisoned");
            guard.get(&token.0).cloned().ok_or(AuthError::SessionInvalid)?
        };
        if !session.is_active(Utc::now()) {
            return Err(AuthError::SessionInvalid);
        }
        let guard = self.users.lock().expect("users lock poisoned");
        let mu = guard
            .values()
            .find(|m| m.record.id == session.user_id)
            .ok_or(AuthError::SessionInvalid)?;
        Ok(mu.record.clone())
    }

    async fn revoke_session(&self, token: SessionToken) -> AuthResult<()> {
        let mut guard = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(s) = guard.get_mut(&token.0) {
            s.revoked_at = Some(Utc::now());
        }
        Ok(())
    }
}

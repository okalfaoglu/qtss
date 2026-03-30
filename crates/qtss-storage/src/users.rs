//! `users` table helpers (e.g. `preferred_locale` from migration 0045).

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

pub struct UserRepository {
    pool: PgPool,
}

impl UserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get_preferred_locale(
        &self,
        user_id: Uuid,
    ) -> Result<Option<String>, StorageError> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as(r#"SELECT preferred_locale FROM users WHERE id = $1"#)
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.and_then(|(l,)| l))
    }

    pub async fn set_preferred_locale(
        &self,
        user_id: Uuid,
        locale: Option<&str>,
    ) -> Result<(), StorageError> {
        sqlx::query(r#"UPDATE users SET preferred_locale = $2 WHERE id = $1"#)
            .bind(user_id)
            .bind(locale)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

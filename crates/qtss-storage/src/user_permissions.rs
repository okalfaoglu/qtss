//! `user_permissions` — kullanıcıya özel `qtss:*` satırları.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

pub struct UserPermissionRepository {
    pool: PgPool,
}

impl UserPermissionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list_for_user(&self, user_id: Uuid) -> Result<Vec<String>, StorageError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"SELECT permission FROM user_permissions WHERE user_id = $1 ORDER BY permission"#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(p,)| p).collect())
    }

    pub async fn org_id_for_user(&self, user_id: Uuid) -> Result<Option<Uuid>, StorageError> {
        let row: Option<(Uuid,)> =
            sqlx::query_as(r#"SELECT org_id FROM users WHERE id = $1"#)
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(o,)| o))
    }

    /// Mevcut satırları siler, verilen izinleri yazar (yalnızca çağıranın doğruladığı dizgi).
    pub async fn replace_for_user(
        &self,
        user_id: Uuid,
        permissions: &[String],
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(r#"DELETE FROM user_permissions WHERE user_id = $1"#)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        for p in permissions {
            sqlx::query(
                r#"INSERT INTO user_permissions (user_id, permission) VALUES ($1, $2)"#,
            )
            .bind(user_id)
            .bind(p)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

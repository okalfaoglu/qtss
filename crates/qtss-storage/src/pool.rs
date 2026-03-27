use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::error::StorageError;

pub async fn create_pool(database_url: &str, max_connections: u32) -> Result<PgPool, StorageError> {
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &PgPool) -> Result<(), StorageError> {
    sqlx::migrate!("../../migrations").run(pool).await?;
    Ok(())
}

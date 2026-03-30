//! Integration: apply SQLx migrations against `DATABASE_URL` (FAZ 11.10 / §2.2 madde 9).
//!
//! CI sets `DATABASE_URL`; locally, export a Postgres URL or the test exits early without failure.
//! Expects **0045+0046+0047** worker `system_config` seeds (count ≥ 7).

use qtss_storage::{create_pool, run_migrations};

#[tokio::test]
async fn migrations_apply_and_worker_system_config_seeded() {
    let url = match std::env::var("DATABASE_URL") {
        Ok(u) if !u.trim().is_empty() => u,
        _ => {
            eprintln!("migrations_apply: skip (DATABASE_URL unset)");
            return;
        }
    };

    let pool = create_pool(&url, 3).await.expect("create_pool");
    run_migrations(&pool).await.expect("run_migrations");

    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM system_config WHERE module = $1",
    )
    .bind("worker")
    .fetch_one(&pool)
    .await
    .expect("count worker system_config");

    assert!(
        n >= 7,
        "expected at least 7 worker system_config seeds (0045+0046+0047), got {n}"
    );
}

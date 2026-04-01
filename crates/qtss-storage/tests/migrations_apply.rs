//! Postgres integration: full `migrations/*.sql` chain applies (`QTSS_MASTER_DEV_GUIDE` §7 CI).
//!
//! Requires `DATABASE_URL`. CI `postgres-migrations` job provides it.

use qtss_storage::{create_pool, run_migrations};
use sqlx::postgres::PgConnectOptions;
use std::str::FromStr;

#[tokio::test]
async fn migrations_apply_including_ai_tables() {
    qtss_common::load_dotenv();
    let url = match std::env::var("DATABASE_URL") {
        Ok(u) if !u.trim().is_empty() => u,
        _ => {
            eprintln!("migrations_apply: skip (DATABASE_URL unset)");
            return;
        }
    };

    if PgConnectOptions::from_str(url.trim()).is_err() {
        eprintln!("migrations_apply: skip (DATABASE_URL invalid)");
        return;
    }

    let pool = create_pool(url.trim(), 3).await.expect("create_pool");
    run_migrations(&pool).await.expect("run_migrations");

    let ai_ok: bool =
        sqlx::query_scalar::<_, bool>("SELECT to_regclass('public.ai_decisions') IS NOT NULL")
            .fetch_one(&pool)
            .await
            .expect("to_regclass ai_decisions");
    assert!(ai_ok, "ai_decisions missing after migrations");

    let apr_ok: bool =
        sqlx::query_scalar::<_, bool>("SELECT to_regclass('public.ai_approval_requests') IS NOT NULL")
            .fetch_one(&pool)
            .await
            .expect("to_regclass ai_approval_requests");
    assert!(apr_ok, "ai_approval_requests missing after migrations");

    let has_col: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_schema = 'public'
              AND table_name = 'ai_decisions'
              AND column_name = 'approval_request_id'
        )"#,
    )
    .fetch_one(&pool)
    .await
    .expect("approval_request_id column check");
    assert!(has_col, "ai_decisions.approval_request_id expected");
}

//! Integration: `decision_exists_for_hash` TTL window against live Postgres (`QTSS_MASTER_DEV_GUIDE` §7).
//!
//! CI (`postgres-migrations` job) sets `DATABASE_URL`. Locally, skip if unset.

use qtss_ai::storage::{decision_exists_for_hash, insert_ai_decision};
use qtss_storage::{create_pool, run_migrations};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn decision_exists_for_hash_respects_created_at_ttl() {
    let url = match std::env::var("DATABASE_URL") {
        Ok(u) if !u.trim().is_empty() => u,
        _ => {
            eprintln!("decision_exists_for_hash_it: skip (DATABASE_URL unset)");
            return;
        }
    };

    let pool = create_pool(&url, 3).await.expect("create_pool");
    run_migrations(&pool).await.expect("run_migrations");

    let prompt_hash = format!("it_prompt_hash_{}", Uuid::new_v4());
    let ttl_minutes = 60i64;

    assert!(
        !decision_exists_for_hash(&pool, &prompt_hash, ttl_minutes)
            .await
            .expect("decision_exists_for_hash empty"),
        "expected no row before insert",
    );

    let row_id = insert_ai_decision(
        &pool,
        "tactical",
        Some("BTC"),
        None,
        Some(&prompt_hash),
        &json!({ "it": true }),
        None,
        None,
        None,
        3600,
        &json!({}),
    )
    .await
    .expect("insert_ai_decision");

    assert!(
        decision_exists_for_hash(&pool, &prompt_hash, ttl_minutes)
            .await
            .expect("decision_exists_for_hash after insert"),
        "expected row inside TTL window",
    );

    sqlx::query(
        r#"UPDATE ai_decisions
           SET created_at = created_at - ($2 * interval '1 minute') - interval '1 second'
           WHERE id = $1"#,
    )
    .bind(row_id)
    .bind(ttl_minutes)
    .execute(&pool)
    .await
    .expect("backdate created_at past TTL");

    assert!(
        !decision_exists_for_hash(&pool, &prompt_hash, ttl_minutes)
            .await
            .expect("decision_exists_for_hash after backdate"),
        "expected no row after created_at moved before TTL window",
    );
}

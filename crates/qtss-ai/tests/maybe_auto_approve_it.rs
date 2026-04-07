//! Integration: `maybe_auto_approve` updates `ai_decisions` / child rows when gate passes (`QTSS_MASTER_DEV_GUIDE` §7).
//!
//! CI (`postgres-migrations` job) sets `DATABASE_URL`. Locally, skip if unset.

use chrono::{Duration, Utc};
use qtss_ai::approval::{maybe_auto_approve, AiDecisionNotifySnapshot};
use qtss_ai::config::AiEngineConfig;
use qtss_ai::storage::{insert_ai_decision, insert_tactical_decision};
use qtss_storage::{create_pool, run_migrations, sync_sqlx_migration_checksums_from_disk};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn maybe_auto_approve_marks_rows_approved_when_eligible() {
    qtss_common::load_dotenv();
    let url = match std::env::var("DATABASE_URL") {
        Ok(u) if !u.trim().is_empty() => u,
        _ => {
            eprintln!("maybe_auto_approve_it: skip (DATABASE_URL unset)");
            return;
        }
    };

    let pool = create_pool(&url, 3).await.expect("create_pool");
    sync_sqlx_migration_checksums_from_disk(&pool)
        .await
        .expect("sync_sqlx_migration_checksums_from_disk");
    run_migrations(&pool).await.expect("run_migrations");

    let mut cfg = AiEngineConfig::default_disabled();
    cfg.auto_approve_enabled = true;
    cfg.auto_approve_threshold = 0.85;

    let decision_id = insert_ai_decision(
        &pool,
        "tactical",
        Some("BTC"),
        None,
        Some(&format!("hash_{}", Uuid::new_v4())),
        &json!({ "it": true }),
        None,
        None,
        Some(0.92),
        3600,
        None,
        &json!({}),
    )
    .await
    .expect("insert_ai_decision");

    let valid_until = Utc::now() + Duration::hours(1);
    insert_tactical_decision(
        &pool,
        decision_id,
        "BTC",
        &json!({
            "direction": "buy",
            "position_size_multiplier": 1.0,
            "stop_loss_pct": 0.02,
            "take_profit_pct": 0.04
        }),
        valid_until,
    )
    .await
    .expect("insert_tactical_decision");

    maybe_auto_approve(
        &pool,
        decision_id,
        0.92,
        &cfg,
        None,
        Some("BTC"),
        Some("buy"),
        Some("it"),
        &AiDecisionNotifySnapshot::default(),
    )
    .await
    .expect("maybe_auto_approve");

    let parent: String = sqlx::query_scalar("SELECT status FROM ai_decisions WHERE id = $1")
        .bind(decision_id)
        .fetch_one(&pool)
        .await
        .expect("select ai_decisions.status");
    assert_eq!(parent, "approved");

    let child: String = sqlx::query_scalar(
        "SELECT status FROM ai_tactical_decisions WHERE decision_id = $1 LIMIT 1",
    )
    .bind(decision_id)
    .fetch_one(&pool)
    .await
    .expect("select ai_tactical_decisions.status");
    assert_eq!(child, "approved");
}

#[tokio::test]
async fn maybe_auto_approve_leaves_pending_when_below_threshold() {
    qtss_common::load_dotenv();
    let url = match std::env::var("DATABASE_URL") {
        Ok(u) if !u.trim().is_empty() => u,
        _ => {
            eprintln!("maybe_auto_approve_it: skip (DATABASE_URL unset)");
            return;
        }
    };

    let pool = create_pool(&url, 3).await.expect("create_pool");
    sync_sqlx_migration_checksums_from_disk(&pool)
        .await
        .expect("sync_sqlx_migration_checksums_from_disk");
    run_migrations(&pool).await.expect("run_migrations");

    let mut cfg = AiEngineConfig::default_disabled();
    cfg.auto_approve_enabled = true;
    cfg.auto_approve_threshold = 0.85;

    let decision_id = insert_ai_decision(
        &pool,
        "tactical",
        Some("ETH"),
        None,
        Some(&format!("hash_{}", Uuid::new_v4())),
        &json!({}),
        None,
        None,
        Some(0.50),
        3600,
        None,
        &json!({}),
    )
    .await
    .expect("insert_ai_decision");

    maybe_auto_approve(
        &pool,
        decision_id,
        0.50,
        &cfg,
        None,
        Some("ETH"),
        None,
        None,
        &AiDecisionNotifySnapshot::default(),
    )
    .await
    .expect("maybe_auto_approve");

    let parent: String = sqlx::query_scalar("SELECT status FROM ai_decisions WHERE id = $1")
        .bind(decision_id)
        .fetch_one(&pool)
        .await
        .expect("select status");
    assert_eq!(parent, "pending_approval");
}

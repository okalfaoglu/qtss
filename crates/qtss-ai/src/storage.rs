//! Persistence helpers for `ai_*` tables (FAZ 2.4).

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AiError, AiResult};

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct AiTacticalDecisionRow {
    pub id: Uuid,
    pub decision_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub symbol: String,
    pub direction: String,
    pub position_size_multiplier: f64,
    pub entry_price_hint: Option<f64>,
    pub stop_loss_pct: Option<f64>,
    pub take_profit_pct: Option<f64>,
    pub reasoning: Option<String>,
    pub confidence: Option<f64>,
    pub status: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AiPositionDirectiveRow {
    pub id: Uuid,
    pub decision_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub symbol: String,
    pub open_position_ref: Option<Uuid>,
    pub action: String,
    pub new_stop_loss_pct: Option<f64>,
    pub new_take_profit_pct: Option<f64>,
    pub trailing_callback_pct: Option<f64>,
    pub partial_close_pct: Option<f64>,
    pub reasoning: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AiDecisionSummaryRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub layer: String,
    pub symbol: Option<String>,
    pub parsed_decision: Option<Value>,
    pub confidence: Option<f64>,
    pub status: String,
}

#[derive(Debug, Clone, Copy)]
pub enum AiRecordTable {
    TacticalChild,
    PositionDirectiveChild,
    PortfolioDirectiveChild,
}

/// Insert parent `ai_decisions` row (pending approval).
#[allow(clippy::too_many_arguments)]
pub async fn insert_ai_decision(
    pool: &PgPool,
    layer: &str,
    symbol: Option<&str>,
    model_id: Option<&str>,
    prompt_hash: Option<&str>,
    input_snapshot: &Value,
    raw_output: Option<&str>,
    parsed_decision: Option<&Value>,
    confidence: Option<f64>,
    decision_ttl_secs: u64,
    approval_request_id: Option<Uuid>,
    meta_json: &Value,
) -> AiResult<Uuid> {
    let expires_at = Utc::now() + Duration::seconds(decision_ttl_secs as i64);
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO ai_decisions (
            layer, symbol, model_id, prompt_hash, input_snapshot,
            raw_output, parsed_decision, status, confidence, expires_at, meta_json,
            approval_request_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending_approval', $8, $9, $10, $11)
        RETURNING id
        "#,
    )
    .bind(layer)
    .bind(symbol)
    .bind(model_id)
    .bind(prompt_hash)
    .bind(input_snapshot)
    .bind(raw_output)
    .bind(parsed_decision)
    .bind(confidence)
    .bind(expires_at)
    .bind(meta_json)
    .bind(approval_request_id)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Attach an existing `ai_approval_requests` row to a pending LLM decision (same org enforced by caller).
pub async fn set_ai_decision_approval_link(
    pool: &PgPool,
    decision_id: Uuid,
    approval_request_id: Uuid,
) -> AiResult<u64> {
    let res = sqlx::query(
        r#"UPDATE ai_decisions
           SET approval_request_id = $2
           WHERE id = $1 AND status = 'pending_approval'"#,
    )
    .bind(decision_id)
    .bind(approval_request_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// When an AI decision is approved/rejected (API or auto), mirror status onto a linked general approval queue row.
pub async fn sync_linked_approval_request_status(
    pool: &PgPool,
    decision_id: Uuid,
    approval_status: &str,
    admin_note: Option<&str>,
    decided_by_user_id: Option<Uuid>,
) -> AiResult<()> {
    if approval_status != "approved" && approval_status != "rejected" {
        return Ok(());
    }
    // Column is nullable; decode as `Option<Uuid>` or NULL becomes a decode error for plain `Uuid`.
    let aid: Option<Uuid> = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT approval_request_id FROM ai_decisions WHERE id = $1",
    )
    .bind(decision_id)
    .fetch_one(pool)
    .await?;
    let Some(aid) = aid else {
        return Ok(());
    };
    if let Some(uid) = decided_by_user_id {
        sqlx::query(
            r#"UPDATE ai_approval_requests
               SET status = $1,
                   admin_note = COALESCE($2, admin_note),
                   decided_by_user_id = $3,
                   decided_at = now(),
                   updated_at = now()
               WHERE id = $4 AND status = 'pending'"#,
        )
        .bind(approval_status)
        .bind(admin_note)
        .bind(uid)
        .bind(aid)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            r#"UPDATE ai_approval_requests
               SET status = $1,
                   admin_note = COALESCE($2, admin_note),
                   decided_at = now(),
                   updated_at = now()
               WHERE id = $3 AND status = 'pending'"#,
        )
        .bind(approval_status)
        .bind(admin_note)
        .bind(aid)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn insert_ai_decision_error(
    pool: &PgPool,
    layer: &str,
    symbol: Option<&str>,
    input_snapshot: &Value,
    raw_output: &str,
    meta_json: &Value,
) -> AiResult<Uuid> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO ai_decisions (
            layer, symbol, input_snapshot, raw_output, status, meta_json
        )
        VALUES ($1, $2, $3, $4, 'error', $5)
        RETURNING id
        "#,
    )
    .bind(layer)
    .bind(symbol)
    .bind(input_snapshot)
    .bind(raw_output)
    .bind(meta_json)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn insert_tactical_decision(
    pool: &PgPool,
    decision_id: Uuid,
    symbol: &str,
    parsed: &Value,
    valid_until: DateTime<Utc>,
) -> AiResult<Uuid> {
    let direction = parsed
        .get("direction")
        .and_then(|x| x.as_str())
        .ok_or_else(|| AiError::parse("tactical parsed: direction"))?;
    let position_size_multiplier = parsed
        .get("position_size_multiplier")
        .and_then(|x| x.as_f64())
        .unwrap_or(1.0);
    let entry_price_hint = parsed.get("entry_price_hint").and_then(|x| x.as_f64());
    let stop_loss_pct = parsed.get("stop_loss_pct").and_then(|x| x.as_f64());
    let take_profit_pct = parsed.get("take_profit_pct").and_then(|x| x.as_f64());
    let reasoning = parsed
        .get("reasoning")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let confidence = parsed.get("confidence").and_then(|x| x.as_f64());
    let sym_u = symbol.trim().to_uppercase();
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO ai_tactical_decisions (
            decision_id, valid_until, symbol, direction,
            position_size_multiplier, entry_price_hint, stop_loss_pct, take_profit_pct,
            reasoning, confidence, status
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'pending_approval')
        RETURNING id
        "#,
    )
    .bind(decision_id)
    .bind(valid_until)
    .bind(&sym_u)
    .bind(direction)
    .bind(position_size_multiplier)
    .bind(entry_price_hint)
    .bind(stop_loss_pct)
    .bind(take_profit_pct)
    .bind(reasoning)
    .bind(confidence)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn insert_position_directive(
    pool: &PgPool,
    decision_id: Uuid,
    symbol: &str,
    parsed: &Value,
) -> AiResult<Uuid> {
    let action = parsed
        .get("action")
        .and_then(|x| x.as_str())
        .ok_or_else(|| AiError::parse("operational parsed: action"))?;
    let open_position_ref = parsed
        .get("open_position_ref")
        .and_then(|x| x.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());
    let new_stop_loss_pct = parsed.get("new_stop_loss_pct").and_then(|x| x.as_f64());
    let new_take_profit_pct = parsed.get("new_take_profit_pct").and_then(|x| x.as_f64());
    let trailing_callback_pct = parsed.get("trailing_callback_pct").and_then(|x| x.as_f64());
    let partial_close_pct = parsed.get("partial_close_pct").and_then(|x| x.as_f64());
    let reasoning = parsed
        .get("reasoning")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let sym_u = symbol.trim().to_uppercase();
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO ai_position_directives (
            decision_id, symbol, open_position_ref, action,
            new_stop_loss_pct, new_take_profit_pct, trailing_callback_pct, partial_close_pct,
            reasoning, status
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'pending_approval')
        RETURNING id
        "#,
    )
    .bind(decision_id)
    .bind(&sym_u)
    .bind(open_position_ref)
    .bind(action)
    .bind(new_stop_loss_pct)
    .bind(new_take_profit_pct)
    .bind(trailing_callback_pct)
    .bind(partial_close_pct)
    .bind(reasoning)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn insert_portfolio_directive(
    pool: &PgPool,
    decision_id: Uuid,
    parsed: &Value,
    valid_until: Option<DateTime<Utc>>,
) -> AiResult<Uuid> {
    let risk_budget_pct = parsed.get("risk_budget_pct").and_then(|x| x.as_f64());
    let max_open_positions = parsed
        .get("max_open_positions")
        .and_then(|x| x.as_i64())
        .map(|x| x as i32);
    let preferred_regime = parsed
        .get("preferred_regime")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let symbol_scores = parsed
        .get("symbol_scores")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    let macro_note = parsed
        .get("macro_note")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO ai_portfolio_directives (
            decision_id, valid_until, risk_budget_pct, max_open_positions,
            preferred_regime, symbol_scores, macro_note, status
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending_approval')
        RETURNING id
        "#,
    )
    .bind(decision_id)
    .bind(valid_until)
    .bind(risk_budget_pct)
    .bind(max_open_positions)
    .bind(preferred_regime)
    .bind(Json(&symbol_scores))
    .bind(macro_note)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn fetch_latest_approved_tactical(
    pool: &PgPool,
    symbol: &str,
) -> AiResult<Option<AiTacticalDecisionRow>> {
    let sym = symbol.trim().to_uppercase();
    let row = sqlx::query_as::<_, AiTacticalDecisionRow>(
        r#"
        SELECT id, decision_id, created_at, valid_until, symbol, direction,
               position_size_multiplier, entry_price_hint, stop_loss_pct, take_profit_pct,
               reasoning, confidence, status
        FROM ai_tactical_decisions
        WHERE symbol = $1 AND status = 'approved' AND valid_until > now()
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(&sym)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn fetch_latest_approved_directive(
    pool: &PgPool,
    symbol: &str,
) -> AiResult<Option<AiPositionDirectiveRow>> {
    let sym = symbol.trim().to_uppercase();
    let row = sqlx::query_as::<_, AiPositionDirectiveRow>(
        r#"
        SELECT id, decision_id, created_at, symbol, open_position_ref, action,
               new_stop_loss_pct, new_take_profit_pct, trailing_callback_pct, partial_close_pct,
               reasoning, status
        FROM ai_position_directives
        WHERE symbol = $1 AND status = 'approved' AND created_at > now() - interval '10 minutes'
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(&sym)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Marks a child row `applied` and aligns parent `ai_decisions` when applicable.
pub async fn mark_applied(pool: &PgPool, table: AiRecordTable, child_id: Uuid) -> AiResult<()> {
    match table {
        AiRecordTable::TacticalChild => {
            let decision_id: Uuid = sqlx::query_scalar(
                "SELECT decision_id FROM ai_tactical_decisions WHERE id = $1",
            )
            .bind(child_id)
            .fetch_one(pool)
            .await?;
            sqlx::query(
                "UPDATE ai_tactical_decisions SET status = 'applied' WHERE id = $1",
            )
            .bind(child_id)
            .execute(pool)
            .await?;
            sqlx::query(
                r#"UPDATE ai_decisions SET status = 'applied', applied_at = COALESCE(applied_at, now())
                   WHERE id = $1"#,
            )
            .bind(decision_id)
            .execute(pool)
            .await?;
        }
        AiRecordTable::PositionDirectiveChild => {
            let decision_id: Uuid = sqlx::query_scalar(
                "SELECT decision_id FROM ai_position_directives WHERE id = $1",
            )
            .bind(child_id)
            .fetch_one(pool)
            .await?;
            sqlx::query(
                "UPDATE ai_position_directives SET status = 'applied' WHERE id = $1",
            )
            .bind(child_id)
            .execute(pool)
            .await?;
            sqlx::query(
                r#"UPDATE ai_decisions SET status = 'applied', applied_at = COALESCE(applied_at, now())
                   WHERE id = $1"#,
            )
            .bind(decision_id)
            .execute(pool)
            .await?;
        }
        AiRecordTable::PortfolioDirectiveChild => {
            let decision_id: Uuid = sqlx::query_scalar(
                "SELECT decision_id FROM ai_portfolio_directives WHERE id = $1",
            )
            .bind(child_id)
            .fetch_one(pool)
            .await?;
            sqlx::query(
                "UPDATE ai_portfolio_directives SET status = 'applied' WHERE id = $1",
            )
            .bind(child_id)
            .execute(pool)
            .await?;
            sqlx::query(
                r#"UPDATE ai_decisions SET status = 'applied', applied_at = COALESCE(applied_at, now())
                   WHERE id = $1"#,
            )
            .bind(decision_id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// After a failed live order placement, stop re-attempting the same approved tactical row each tick.
/// Parent `ai_decisions` is set to `error` for operator visibility.
pub async fn mark_tactical_execution_failed(pool: &PgPool, tactical_child_id: Uuid) -> AiResult<()> {
    let decision_id: Uuid = sqlx::query_scalar(
        "SELECT decision_id FROM ai_tactical_decisions WHERE id = $1",
    )
    .bind(tactical_child_id)
    .fetch_one(pool)
    .await?;
    sqlx::query(
        "UPDATE ai_tactical_decisions SET status = 'execution_failed' WHERE id = $1",
    )
    .bind(tactical_child_id)
    .execute(pool)
    .await?;
    sqlx::query(
        r#"UPDATE ai_decisions SET status = 'error'
           WHERE id = $1"#,
    )
    .bind(decision_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Expire pending parent rows past `expires_at` and align tactical / operational / portfolio children
/// so they do not stay `pending_approval` or legacy `active` while the parent is `expired`.
pub async fn expire_stale_decisions(pool: &PgPool) -> AiResult<u64> {
    let mut tx = pool.begin().await?;
    let ids: Vec<Uuid> = sqlx::query_scalar(
        r#"UPDATE ai_decisions
           SET status = 'expired'
           WHERE status = 'pending_approval'
             AND expires_at IS NOT NULL
             AND expires_at < now()
           RETURNING id"#,
    )
    .fetch_all(&mut *tx)
    .await?;

    if !ids.is_empty() {
        sqlx::query(
            r#"UPDATE ai_tactical_decisions SET status = 'expired'
               WHERE decision_id = ANY($1) AND status = 'pending_approval'"#,
        )
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"UPDATE ai_position_directives SET status = 'expired'
               WHERE decision_id = ANY($1) AND status = 'pending_approval'"#,
        )
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"UPDATE ai_portfolio_directives SET status = 'expired'
               WHERE decision_id = ANY($1)
                 AND status IN ('pending_approval', 'active', 'approved')"#,
        )
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(ids.len() as u64)
}

/// Duplicate prompt suppression within `ttl_minutes` (Postgres `interval`).
/// TTL behavior is covered by `tests/decision_exists_for_hash_it.rs` in CI (`postgres-migrations` + `DATABASE_URL`).
pub async fn decision_exists_for_hash(pool: &PgPool, prompt_hash: &str, ttl_minutes: i64) -> AiResult<bool> {
    let exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(
            SELECT 1 FROM ai_decisions
            WHERE prompt_hash = $1
              AND created_at > now() - ($2 * interval '1 minute')
        )"#,
    )
    .bind(prompt_hash)
    .bind(ttl_minutes)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// When true, the tactical sweep should not call the LLM or enqueue another Telegram approval for this symbol.
///
/// Blocks while a tactical row is still “open” in DB terms:
/// - `pending_approval` before parent `expires_at`
/// - `approved` and executor-eligible (`valid_until`, child still `approved`)
/// - `applied` (entry recorded) until an `ai_decision_outcomes` row exists (e.g. SL/TP close in `position_manager`)
///
/// Rejected / expired / error / `execution_failed` rows do not block. Hash-only dedupe ([`decision_exists_for_hash`]) is still applied afterward.
pub async fn tactical_symbol_blocked_by_active_decision(pool: &PgPool, symbol: &str) -> AiResult<bool> {
    let sym = symbol.trim().to_uppercase();
    let blocked: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM ai_decisions d
            INNER JOIN ai_tactical_decisions t ON t.decision_id = d.id
            WHERE UPPER(TRIM(d.symbol)) = $1
              AND d.layer = 'tactical'
              AND t.status NOT IN ('rejected', 'expired', 'execution_failed')
              AND d.status NOT IN ('rejected', 'expired', 'error')
              AND NOT EXISTS (SELECT 1 FROM ai_decision_outcomes o WHERE o.decision_id = d.id)
              AND (
                    (d.status = 'pending_approval' AND (d.expires_at IS NULL OR d.expires_at > now()))
                 OR (d.status = 'approved' AND t.status = 'approved' AND t.valid_until > now())
                 OR (d.status = 'applied' AND t.status = 'applied')
              )
        )
        "#,
    )
    .bind(&sym)
    .fetch_one(pool)
    .await?;
    Ok(blocked)
}

/// Latest tactical AI decision for `symbol` within the last `hours` (dedup context).
pub async fn fetch_last_ai_decision_recent(
    pool: &PgPool,
    symbol: &str,
    hours: i64,
) -> AiResult<Option<AiDecisionSummaryRow>> {
    let sym = symbol.trim().to_uppercase();
    let row = sqlx::query_as::<_, AiDecisionSummaryRow>(
        r#"
        SELECT id, created_at, layer, symbol, parsed_decision, confidence, status
        FROM ai_decisions
        WHERE layer = 'tactical'
          AND symbol = $1
          AND created_at > now() - ($2 * interval '1 hour')
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(&sym)
    .bind(hours)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct AiDecisionListRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub layer: String,
    pub symbol: Option<String>,
    pub status: String,
    pub confidence: Option<f64>,
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct AiDecisionDetailRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub layer: String,
    pub symbol: Option<String>,
    pub model_id: Option<String>,
    pub prompt_hash: Option<String>,
    pub input_snapshot: Value,
    pub raw_output: Option<String>,
    pub parsed_decision: Option<Value>,
    pub status: String,
    pub approved_by: Option<String>,
    pub approved_at: Option<DateTime<Utc>>,
    pub applied_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub confidence: Option<f64>,
    pub meta_json: Value,
    pub approval_request_id: Option<Uuid>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct AiPortfolioDirectiveRow {
    pub id: Uuid,
    pub decision_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub risk_budget_pct: Option<f64>,
    pub max_open_positions: Option<i32>,
    pub preferred_regime: Option<String>,
    pub symbol_scores: Value,
    pub macro_note: Option<String>,
    pub status: String,
}

/// Split dashboard filter fields on `|` so `tactical | operational` matches either layer.
fn split_filter_tokens(raw: Option<&str>) -> Vec<String> {
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    s.split('|')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// Filtered list for dashboards (FAZ 7.1).
/// `layer` / `status` may contain several values separated by `|` (e.g. `tactical | operational` → OR).
pub async fn list_ai_decisions(
    pool: &PgPool,
    layer: Option<&str>,
    symbol: Option<&str>,
    status: Option<&str>,
    limit: i64,
) -> AiResult<Vec<AiDecisionListRow>> {
    let lim = limit.clamp(1, 500);
    let layer_vec = split_filter_tokens(layer);
    let status_vec = split_filter_tokens(status);
    let symbol_u = symbol.map(|s| s.trim().to_uppercase()).filter(|s| !s.is_empty());

    let rows = sqlx::query_as::<_, AiDecisionListRow>(
        r#"SELECT id, created_at, layer, symbol, status, confidence, model_id
           FROM ai_decisions
           WHERE (cardinality($1::text[]) = 0 OR layer = ANY($1))
             AND (cardinality($2::text[]) = 0 OR status = ANY($2))
             AND ($3::text IS NULL OR UPPER(TRIM(COALESCE(symbol, ''))) = $3)
           ORDER BY created_at DESC
           LIMIT $4"#,
    )
    .bind(&layer_vec)
    .bind(&status_vec)
    .bind(symbol_u.as_ref())
    .bind(lim)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Deletes all rows with the given status. Only `error` is accepted from the public API (safe housekeeping).
pub async fn delete_ai_decisions_with_status(pool: &PgPool, status: &str) -> AiResult<u64> {
    let st = status.trim();
    if st != "error" {
        return Err(AiError::config(
            "delete_ai_decisions_with_status: only status 'error' is supported",
        ));
    }
    let res = sqlx::query(r#"DELETE FROM ai_decisions WHERE status = $1"#)
        .bind(st)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// Postgres `LISTEN` / `NOTIFY` channel — `qtss-worker` `ai_tactical_executor_loop` interrupts tick sleep when a decision is approved.
pub const AI_TACTICAL_EXECUTOR_WAKE_NOTIFY_CHANNEL: &str = "qtss_ai_tactical_wake";

/// Wake `ai_tactical_executor_loop` immediately (best-effort) after manual or auto-approve.
pub async fn notify_ai_tactical_executor_wake(pool: &PgPool) -> AiResult<()> {
    sqlx::query(r#"SELECT pg_notify($1, $2)"#)
        .bind(AI_TACTICAL_EXECUTOR_WAKE_NOTIFY_CHANNEL)
        .bind("")
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn fetch_ai_decision_detail(pool: &PgPool, id: Uuid) -> AiResult<Option<AiDecisionDetailRow>> {
    let row = sqlx::query_as::<_, AiDecisionDetailRow>(
        r#"SELECT id, created_at, layer, symbol, model_id, prompt_hash, input_snapshot,
                  raw_output, parsed_decision, status, approved_by, approved_at, applied_at,
                  expires_at, confidence, meta_json, approval_request_id
           FROM ai_decisions WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn admin_approve_ai_decision(pool: &PgPool, id: Uuid, approved_by: &str) -> AiResult<u64> {
    let mut tx = pool.begin().await?;
    let n = sqlx::query(
        r#"UPDATE ai_decisions
           SET status = 'approved', approved_at = now(), approved_by = $2
           WHERE id = $1 AND status = 'pending_approval'"#,
    )
    .bind(id)
    .bind(approved_by)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    sqlx::query(
        "UPDATE ai_tactical_decisions SET status = 'approved' WHERE decision_id = $1 AND status = 'pending_approval'",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE ai_position_directives SET status = 'approved' WHERE decision_id = $1 AND status = 'pending_approval'",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"UPDATE ai_portfolio_directives SET status = 'approved'
           WHERE decision_id = $1 AND status IN ('pending_approval', 'active')"#,
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let decider = approved_by
        .strip_prefix("jwt:")
        .and_then(|s| Uuid::parse_str(s.trim()).ok());
    sync_linked_approval_request_status(pool, id, "approved", None, decider).await?;
    if n > 0 {
        if let Err(e) = notify_ai_tactical_executor_wake(pool).await {
            tracing::warn!(%e, "notify_ai_tactical_executor_wake after admin approve");
        }
    }
    Ok(n)
}

pub async fn admin_reject_ai_decision(pool: &PgPool, id: Uuid, approved_by: &str) -> AiResult<u64> {
    let mut tx = pool.begin().await?;
    let n = sqlx::query(
        r#"UPDATE ai_decisions
           SET status = 'rejected', approved_at = now(), approved_by = $2
           WHERE id = $1 AND status = 'pending_approval'"#,
    )
    .bind(id)
    .bind(approved_by)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    sqlx::query(
        "UPDATE ai_tactical_decisions SET status = 'rejected' WHERE decision_id = $1 AND status = 'pending_approval'",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE ai_position_directives SET status = 'rejected' WHERE decision_id = $1 AND status = 'pending_approval'",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"UPDATE ai_portfolio_directives SET status = 'rejected'
           WHERE decision_id = $1
             AND status IN ('pending_approval', 'active', 'approved')"#,
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let decider = approved_by
        .strip_prefix("jwt:")
        .and_then(|s| Uuid::parse_str(s.trim()).ok());
    sync_linked_approval_request_status(pool, id, "rejected", None, decider).await?;
    Ok(n)
}

fn decision_id_from_approval_payload(payload: &Value) -> Option<Uuid> {
    for key in ["decision_id", "ai_decision_id"] {
        if let Some(v) = payload.get(key) {
            if let Some(s) = v.as_str() {
                if let Ok(u) = Uuid::parse_str(s.trim()) {
                    return Some(u);
                }
            }
        }
    }
    payload
        .get("decision")
        .and_then(|d| d.get("id"))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s.trim()).ok())
}

/// After `ai_approval_requests` is resolved (e.g. Telegram or REST), apply the same outcome to linked
/// `ai_decisions` (pending only).
///
/// Resolution order:
/// 1. Rows with `ai_decisions.approval_request_id` matching this request.
/// 2. If none, [`decision_id_from_approval_payload`] on `ai_approval_requests.payload` (API clients that
///    did not call the link endpoint).
///
/// Successful [`admin_approve_ai_decision`] calls emit [`notify_ai_tactical_executor_wake`] when the parent row was pending.
pub async fn mirror_approval_request_outcome_to_linked_ai_decisions(
    pool: &PgPool,
    approval_request_id: Uuid,
    approve: bool,
    decided_by: &str,
) -> AiResult<()> {
    use std::collections::BTreeSet;

    let linked: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM ai_decisions WHERE approval_request_id = $1 AND status = 'pending_approval'",
    )
    .bind(approval_request_id)
    .fetch_all(pool)
    .await?;

    let mut ids: BTreeSet<Uuid> = linked.into_iter().collect();

    if ids.is_empty() {
        let payload: Option<Value> = sqlx::query_scalar(
            "SELECT payload FROM ai_approval_requests WHERE id = $1",
        )
        .bind(approval_request_id)
        .fetch_optional(pool)
        .await?;

        if let Some(p) = payload.as_ref().and_then(decision_id_from_approval_payload) {
            tracing::info!(
                %approval_request_id,
                decision_id = %p,
                "mirror approval_request: using decision_id from payload (no approval_request_id link on ai_decisions)"
            );
            ids.insert(p);
        }
    }

    for id in ids {
        let n = if approve {
            admin_approve_ai_decision(pool, id, decided_by).await?
        } else {
            admin_reject_ai_decision(pool, id, decided_by).await?
        };
        if n == 0 {
            tracing::debug!(
                decision_id = %id,
                "mirror approval_request outcome: ai_decision not updated"
            );
        }
    }

    Ok(())
}

/// Latest active portfolio directive (FAZ 7.1).
/// Latest portfolio directive whose parent decision was approved (or applied after use).
/// Excludes rows tied to `pending_approval` / `rejected` / `expired` parents and legacy `active` children
/// that were written before approval alignment.
pub async fn fetch_active_portfolio_directive(pool: &PgPool) -> AiResult<Option<AiPortfolioDirectiveRow>> {
    let row = sqlx::query_as::<_, AiPortfolioDirectiveRow>(
        r#"SELECT p.id, p.decision_id, p.created_at, p.valid_until, p.risk_budget_pct, p.max_open_positions,
                  p.preferred_regime, p.symbol_scores, p.macro_note, p.status
           FROM ai_portfolio_directives p
           INNER JOIN ai_decisions d ON d.id = p.decision_id
           WHERE p.status IN ('active', 'approved')
             AND d.status IN ('approved', 'applied')
           ORDER BY p.created_at DESC
           LIMIT 1"#,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn insert_ai_decision_outcome(
    pool: &PgPool,
    decision_id: Uuid,
    pnl_pct: Option<f64>,
    pnl_usdt: Option<f64>,
    outcome: &str,
    holding_hours: Option<f64>,
    notes: Option<&str>,
) -> AiResult<Uuid> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"INSERT INTO ai_decision_outcomes (
            decision_id, pnl_pct, pnl_usdt, outcome, holding_hours, notes
        ) VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id"#,
    )
    .bind(decision_id)
    .bind(pnl_pct)
    .bind(pnl_usdt)
    .bind(outcome)
    .bind(holding_hours)
    .bind(notes)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Recent decisions with outcomes for multi-turn context (FAZ P2-15).
/// Returns last N decisions for a symbol with direction, confidence, reasoning, status, and outcome.
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct AiDecisionHistoryRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub layer: String,
    pub direction_or_action: Option<String>,
    pub confidence: Option<f64>,
    pub reasoning: Option<String>,
    pub status: String,
    pub outcome: Option<String>,
    pub pnl_pct: Option<f64>,
}

pub async fn fetch_recent_decisions_with_outcomes(
    pool: &PgPool,
    symbol: &str,
    limit: i64,
) -> AiResult<Vec<AiDecisionHistoryRow>> {
    let sym = symbol.trim().to_uppercase();
    let lim = limit.clamp(1, 20);
    let rows = sqlx::query_as::<_, AiDecisionHistoryRow>(
        r#"SELECT d.id, d.created_at, d.layer,
                  COALESCE(
                      d.parsed_decision->>'direction',
                      d.parsed_decision->>'action'
                  ) AS direction_or_action,
                  d.confidence,
                  d.parsed_decision->>'reasoning' AS reasoning,
                  d.status,
                  o.outcome,
                  o.pnl_pct
           FROM ai_decisions d
           LEFT JOIN ai_decision_outcomes o ON o.decision_id = d.id
           WHERE d.symbol = $1
             AND d.status NOT IN ('error', 'expired')
           ORDER BY d.created_at DESC
           LIMIT $2"#,
    )
    .bind(&sym)
    .bind(lim)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Per-symbol outcome stats for tactical/operational feedback (FAZ P2).
pub async fn fetch_symbol_outcome_stats(pool: &PgPool, symbol: &str, n: i64) -> AiResult<Value> {
    let sym = symbol.trim().to_uppercase();
    let lim = n.clamp(1, 100);
    let rows: Vec<(String, Option<f64>, Option<f64>)> = sqlx::query_as(
        r#"SELECT o.outcome, o.pnl_pct, o.pnl_usdt
           FROM ai_decision_outcomes o
           JOIN ai_decisions d ON d.id = o.decision_id
           WHERE d.symbol = $1
           ORDER BY o.recorded_at DESC
           LIMIT $2"#,
    )
    .bind(&sym)
    .bind(lim)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(Value::Null);
    }
    let mut wins = 0_i64;
    let mut losses = 0_i64;
    let mut sum_pnl = 0.0_f64;
    let mut count_pnl = 0_i64;
    for (outcome, pnl_pct, _) in &rows {
        match outcome.as_str() {
            "profit" => wins += 1,
            "loss" => losses += 1,
            _ => {}
        }
        if let Some(p) = pnl_pct {
            sum_pnl += p;
            count_pnl += 1;
        }
    }
    let total = rows.len() as f64;
    Ok(serde_json::json!({
        "symbol": sym,
        "sample_size": rows.len(),
        "win_rate": if total > 0.0 { wins as f64 / total } else { 0.0 },
        "avg_pnl_pct": if count_pnl > 0 { sum_pnl / count_pnl as f64 } else { 0.0 },
        "wins": wins,
        "losses": losses,
    }))
}

/// Aggregate stats over the last `n` outcomes (FAZ 6.3 — strategic context).
pub async fn fetch_recent_outcome_stats(pool: &PgPool, n: i64) -> AiResult<Value> {
    let lim = n.clamp(1, 500);
    let rows: Vec<(String, Option<f64>, Option<f64>)> = sqlx::query_as(
        r#"SELECT o.outcome, o.pnl_pct, o.pnl_usdt
           FROM ai_decision_outcomes o
           ORDER BY o.recorded_at DESC
           LIMIT $1"#,
    )
    .bind(lim)
    .fetch_all(pool)
    .await?;
    let mut wins = 0_i64;
    let mut losses = 0_i64;
    let mut flat = 0_i64;
    let mut sum_pnl = 0.0_f64;
    let mut count_pnl = 0_i64;
    for (outcome, pnl_pct, _) in &rows {
        match outcome.as_str() {
            "profit" => wins += 1,
            "loss" => losses += 1,
            _ => flat += 1,
        }
        if let Some(p) = pnl_pct {
            sum_pnl += p;
            count_pnl += 1;
        }
    }
    let total = rows.len() as f64;
    let win_rate = if total > 0.0 { wins as f64 / total } else { 0.0 };
    let avg_pnl_pct = if count_pnl > 0 {
        sum_pnl / count_pnl as f64
    } else {
        0.0
    };
    Ok(serde_json::json!({
        "sample_size": rows.len(),
        "win_rate_approx": win_rate,
        "wins": wins,
        "losses": losses,
        "other": flat,
        "avg_pnl_pct_sample": avg_pnl_pct,
    }))
}

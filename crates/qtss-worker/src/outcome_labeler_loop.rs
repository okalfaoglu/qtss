//! Faz 9.0.1 — Outcome Labeler.
//!
//! Kapalı `qtss_v2_setups` satırlarını taramayla `qtss_setup_outcomes`
//! tablosuna değişmez etiketle yazar. Idempotent (primary key `setup_id`),
//! retro-labeling ayrı script'e gerek bırakmaz — ilk açılışta mevcut tüm
//! kapalı setup'ları batch halinde etiketler.
//!
//! Etiketleme dispatch tablosu (CLAUDE.md #1):
//!   close_reason → category → label
//!   * target_hit               → tp       → win
//!   * stop_hit                 → sl       → loss
//!   * reverse_signal           → reverse  → invalidated
//!   * p14_opposite_dir_conflict→ conflict → invalidated
//!   * closed_manual            → manual   → neutral|win|loss (pnl'e göre)
//!   * expiry / ttl (future)    → expiry   → timeout
//!   * NULL & closed_win        → tp       → win
//!   * NULL & closed_loss       → sl       → loss
//!   * NULL & state=closed      → unknown  → pnl'den türet
//!
//! Neutral band: `ai.outcome.labeler.neutral_abs_pnl_pct`.

use std::time::Duration;

use qtss_storage::{resolve_system_f64, resolve_worker_enabled_flag, resolve_worker_tick_secs};
use serde_json::json;
use sqlx::{postgres::PgRow, PgPool, Row};
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
struct SetupOutcomeInput {
    setup_id: uuid::Uuid,
    state: String,
    close_reason: Option<String>,
    pnl_pct: Option<f32>,
    risk_pct: Option<f32>,
    max_favorable_r: Option<f32>,
    max_adverse_r: Option<f32>,
    bars_to_close: Option<i32>,
    bars_to_first_tp: Option<i32>,
    closed_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl<'r> sqlx::FromRow<'r, PgRow> for SetupOutcomeInput {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            setup_id: row.try_get("id")?,
            state: row.try_get("state")?,
            close_reason: row.try_get("close_reason")?,
            pnl_pct: row.try_get("pnl_pct")?,
            risk_pct: row.try_get("risk_pct")?,
            max_favorable_r: row.try_get("max_favorable_r")?,
            max_adverse_r: row.try_get("max_adverse_r")?,
            bars_to_close: row.try_get("bars_to_close")?,
            bars_to_first_tp: row.try_get("bars_to_first_tp")?,
            closed_at: row.try_get("closed_at")?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct LabelDecision {
    label: &'static str,
    category: &'static str,
}

/// Dispatch table: close_reason → (category, base label).
fn classify_close_reason(reason: Option<&str>) -> Option<LabelDecision> {
    let r = reason?;
    Some(match r {
        "target_hit" => LabelDecision { label: "win", category: "tp" },
        "stop_hit" => LabelDecision { label: "loss", category: "sl" },
        "reverse_signal" => LabelDecision { label: "invalidated", category: "reverse" },
        "p14_opposite_dir_conflict" => LabelDecision { label: "invalidated", category: "conflict" },
        "closed_manual" => LabelDecision { label: "neutral", category: "manual" },
        "expiry" | "ttl" | "timeout" => LabelDecision { label: "timeout", category: "expiry" },
        _ => LabelDecision { label: "neutral", category: "unknown" },
    })
}

/// Fallback when close_reason is NULL — derive from state + pnl_pct.
fn classify_from_state(state: &str, pnl_pct: Option<f32>, neutral_band: f32) -> LabelDecision {
    match state {
        "closed_win" => LabelDecision { label: "win", category: "tp" },
        "closed_loss" => LabelDecision { label: "loss", category: "sl" },
        _ => {
            let p = pnl_pct.unwrap_or(0.0);
            if p.abs() < neutral_band {
                LabelDecision { label: "neutral", category: "unknown" }
            } else if p > 0.0 {
                LabelDecision { label: "win", category: "unknown" }
            } else {
                LabelDecision { label: "loss", category: "unknown" }
            }
        }
    }
}

/// Final decision: apply neutral band to manual closes + any pnl-based overrides.
fn resolve_label(input: &SetupOutcomeInput, neutral_band: f32) -> LabelDecision {
    if let Some(base) = classify_close_reason(input.close_reason.as_deref()) {
        // Manual closes: pnl mag decides win/loss/neutral.
        if base.category == "manual" {
            let p = input.pnl_pct.unwrap_or(0.0);
            if p.abs() < neutral_band {
                return LabelDecision { label: "neutral", category: "manual" };
            }
            return LabelDecision {
                label: if p > 0.0 { "win" } else { "loss" },
                category: "manual",
            };
        }
        return base;
    }
    classify_from_state(&input.state, input.pnl_pct, neutral_band)
}

fn realized_rr(pnl_pct: Option<f32>, risk_pct: Option<f32>) -> Option<f32> {
    let p = pnl_pct?;
    let r = risk_pct?;
    if r.abs() < 1e-6 {
        return None;
    }
    Some(p / r)
}

async fn sweep_once(pool: &PgPool, neutral_band: f32, batch: i64) -> Result<usize, sqlx::Error> {
    // Her sweep'te outcome'u olmayan kapalı setup'ları çek.
    let rows: Vec<SetupOutcomeInput> = sqlx::query_as(
        r#"
        SELECT s.id, s.state, s.close_reason, s.pnl_pct, s.risk_pct,
               s.max_favorable_r, s.max_adverse_r,
               s.bars_to_close, s.bars_to_first_tp, s.closed_at
          FROM qtss_v2_setups s
          LEFT JOIN qtss_setup_outcomes o ON o.setup_id = s.id
         WHERE o.setup_id IS NULL
           AND (s.state IN ('closed','closed_win','closed_loss','closed_manual')
                OR s.close_reason IS NOT NULL)
         ORDER BY s.closed_at NULLS LAST, s.updated_at DESC
         LIMIT $1
        "#,
    )
    .bind(batch)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(0);
    }

    let mut written = 0usize;
    for r in rows {
        let decision = resolve_label(&r, neutral_band);
        let rr = realized_rr(r.pnl_pct, r.risk_pct);
        let time_to_outcome = r.bars_to_close;
        let meta = json!({
            "source": "outcome_labeler_loop",
            "state": r.state,
            "reason_raw": r.close_reason,
        });
        let res = sqlx::query(
            r#"
            INSERT INTO qtss_setup_outcomes (
                setup_id, label, close_reason, close_reason_category,
                realized_rr, pnl_pct, max_favorable_r, max_adverse_r,
                time_to_outcome_bars, bars_to_first_tp, closed_at, meta_json
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
            ON CONFLICT (setup_id) DO NOTHING
            "#,
        )
        .bind(r.setup_id)
        .bind(decision.label)
        .bind(&r.close_reason)
        .bind(decision.category)
        .bind(rr)
        .bind(r.pnl_pct)
        .bind(r.max_favorable_r)
        .bind(r.max_adverse_r)
        .bind(time_to_outcome)
        .bind(r.bars_to_first_tp)
        .bind(r.closed_at)
        .bind(&meta)
        .execute(pool)
        .await;
        match res {
            Ok(r) if r.rows_affected() > 0 => written += 1,
            Ok(_) => {}
            Err(e) => warn!(%e, setup_id=%r.setup_id, "outcome_labeler insert"),
        }
    }
    Ok(written)
}

pub async fn outcome_labeler_loop(pool: PgPool) {
    info!("outcome_labeler_loop spawned (Faz 9.0.1)");
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "ai",
            "outcome.labeler.enabled",
            "QTSS_OUTCOME_LABELER_ENABLED",
            true,
        )
        .await;
        let tick = resolve_worker_tick_secs(
            &pool,
            "ai",
            "outcome.labeler.tick_secs",
            "QTSS_OUTCOME_LABELER_TICK_SECS",
            120,
            15,
        )
        .await;
        if !enabled {
            tokio::time::sleep(Duration::from_secs(tick)).await;
            continue;
        }
        let neutral_band = resolve_system_f64(
            &pool,
            "ai",
            "outcome.labeler.neutral_abs_pnl_pct",
            "QTSS_OUTCOME_LABELER_NEUTRAL_PCT",
            0.05,
        )
        .await as f32;
        let batch = resolve_worker_tick_secs(
            &pool,
            "ai",
            "outcome.labeler.backfill_batch",
            "QTSS_OUTCOME_LABELER_BATCH",
            500,
            10,
        )
        .await as i64;

        match sweep_once(&pool, neutral_band, batch).await {
            Ok(0) => debug!("outcome_labeler: no new closed setups"),
            Ok(n) => info!(labeled = n, "outcome_labeler sweep"),
            Err(e) => warn!(%e, "outcome_labeler sweep error"),
        }

        tokio::time::sleep(Duration::from_secs(tick)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(reason: Option<&str>, state: &str, pnl: Option<f32>) -> SetupOutcomeInput {
        SetupOutcomeInput {
            setup_id: uuid::Uuid::nil(),
            state: state.to_string(),
            close_reason: reason.map(|s| s.to_string()),
            pnl_pct: pnl,
            risk_pct: Some(1.0),
            max_favorable_r: None,
            max_adverse_r: None,
            bars_to_close: None,
            bars_to_first_tp: None,
            closed_at: None,
        }
    }

    #[test]
    fn tp_and_sl_direct() {
        assert_eq!(resolve_label(&mk(Some("target_hit"), "closed_win", Some(2.0)), 0.05).label, "win");
        assert_eq!(resolve_label(&mk(Some("stop_hit"), "closed_loss", Some(-1.0)), 0.05).label, "loss");
    }

    #[test]
    fn reverse_and_conflict_invalidate() {
        assert_eq!(resolve_label(&mk(Some("reverse_signal"), "closed", None), 0.05).label, "invalidated");
        assert_eq!(resolve_label(&mk(Some("p14_opposite_dir_conflict"), "closed", None), 0.05).label, "invalidated");
    }

    #[test]
    fn manual_split_by_pnl() {
        assert_eq!(resolve_label(&mk(Some("closed_manual"), "closed", Some(0.01)), 0.05).label, "neutral");
        assert_eq!(resolve_label(&mk(Some("closed_manual"), "closed", Some(0.5)), 0.05).label, "win");
        assert_eq!(resolve_label(&mk(Some("closed_manual"), "closed", Some(-0.5)), 0.05).label, "loss");
    }

    #[test]
    fn null_reason_falls_back_to_state() {
        assert_eq!(resolve_label(&mk(None, "closed_win", Some(1.0)), 0.05).label, "win");
        assert_eq!(resolve_label(&mk(None, "closed_loss", Some(-1.0)), 0.05).label, "loss");
        assert_eq!(resolve_label(&mk(None, "closed", Some(0.0)), 0.05).label, "neutral");
    }

    #[test]
    fn rr_computation() {
        assert!((realized_rr(Some(2.0), Some(1.0)).unwrap() - 2.0).abs() < 1e-6);
        assert_eq!(realized_rr(Some(1.0), Some(0.0)), None);
        assert_eq!(realized_rr(None, Some(1.0)), None);
    }
}

//! Faz 9.8.11 — selector worker loop.
//!
//! Bridges the setup engine to the execution layer. Each tick:
//!
//!   1. Read `qtss_setups` rows in state `armed` that have a numeric
//!      entry + SL.
//!   2. Drop the ones that already have a row in `selected_candidates`
//!      for `mode='dry'` (idempotent).
//!   3. For the survivors, compute a minimal TP ladder (single leg at
//!      `target_ref`) and insert a candidate row. The execution bridge
//!      then claims it via `FOR UPDATE SKIP LOCKED`.
//!
//! Why no filter chain here yet: the upstream v2_setup_loop has already
//! walked the allocator + confluence gate + kill-switch. The selector
//! registry (qtss_risk::selector) is surfaced as pure evaluators and
//! will be bolted on in a follow-up once `selected_candidates` carries
//! real traffic — at which point filtering is a local code change
//! with no schema impact.

use std::time::Duration;

use qtss_storage::{
    existing_selected_setup_ids, insert_selected_candidate, list_open_v2_setups,
    resolve_system_u64, resolve_worker_enabled_flag, InsertSelectedCandidate, V2SetupRow,
};
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use tracing::{debug, info, warn};

const MODULE: &str = "risk";
const CFG_ENABLED: &str = "selector.enabled";
const CFG_INTERVAL_MS: &str = "selector.loop_interval_ms";
const CFG_BATCH: &str = "selector.batch_size";
const ENV_ENABLED: &str = "QTSS_SELECTOR_ENABLED";
const ENV_INTERVAL: &str = "QTSS_SELECTOR_INTERVAL_MS";
const ENV_BATCH: &str = "QTSS_SELECTOR_BATCH";

const DEFAULT_INTERVAL_MS: u64 = 5_000;
const DEFAULT_BATCH: u64 = 20;

pub async fn selector_loop(pool: PgPool) {
    info!("selector worker: starting");
    loop {
        let enabled =
            resolve_worker_enabled_flag(&pool, MODULE, CFG_ENABLED, ENV_ENABLED, true).await;
        if !enabled {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        let interval_ms = resolve_system_u64(
            &pool, MODULE, CFG_INTERVAL_MS, ENV_INTERVAL,
            DEFAULT_INTERVAL_MS, 500, 600_000,
        )
        .await;
        let batch = resolve_system_u64(
            &pool, MODULE, CFG_BATCH, ENV_BATCH, DEFAULT_BATCH, 1, 500,
        )
        .await as i64;

        if let Err(e) = run_tick(&pool, batch).await {
            warn!(error=%e, "selector tick failed");
        }
        tokio::time::sleep(Duration::from_millis(interval_ms.max(500))).await;
    }
}

async fn run_tick(pool: &PgPool, batch: i64) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let open = list_open_v2_setups(pool, None, None).await?;
    // Accept both "armed" (entry not yet touched) and "active" (already
    // running) setups: the v2 pipeline currently writes setups straight
    // into "active", so filtering on "armed" would starve us. The
    // selected_candidates UNIQUE (setup_id, mode) keeps the enqueue
    // idempotent regardless of how many times we see the same row.
    let armed: Vec<V2SetupRow> = open
        .into_iter()
        .filter(|s| {
            matches!(s.state.as_str(), "armed" | "active")
                && s.entry_price.is_some()
                && s.entry_sl.is_some()
        })
        .take(batch as usize)
        .collect();
    if armed.is_empty() {
        return Ok(());
    }
    let ids: Vec<_> = armed.iter().map(|s| s.id).collect();
    let existing = existing_selected_setup_ids(pool, &ids, "dry").await?;
    let existing_set: std::collections::HashSet<_> = existing.into_iter().collect();

    let mut inserted = 0usize;
    for s in &armed {
        if existing_set.contains(&s.id) {
            continue;
        }
        let candidate = build_candidate(s);
        match candidate {
            Some(c) => match insert_selected_candidate(pool, &c).await? {
                Some(_id) => inserted += 1,
                None => debug!(setup_id=%s.id, "selector: idempotent skip"),
            },
            None => debug!(setup_id=%s.id, "selector: skip (unable to build candidate)"),
        }
    }
    if inserted > 0 {
        info!(inserted, "selector: candidates enqueued");
    }
    Ok(())
}

fn build_candidate(s: &V2SetupRow) -> Option<InsertSelectedCandidate> {
    let entry = decimal_from_f32(s.entry_price?)?;
    let sl = decimal_from_f32(s.entry_sl?)?;
    let tp_ladder = match s.target_ref {
        Some(t) => {
            let tp = decimal_from_f32(t)?;
            json!([{ "price": tp.to_string(), "qty": "1.0", "filled_qty": "0" }])
        }
        None => json!([]),
    };
    let risk_pct = s
        .risk_pct
        .and_then(decimal_from_f32)
        .unwrap_or_else(|| Decimal::new(1, 2)); // 0.01 default
    let direction: &'static str = match s.direction.as_str() {
        "long" => "long",
        "short" => "short",
        _ => return None, // neutral never placed
    };
    let meta = json!({
        "profile": s.profile,
        "venue_class": s.venue_class,
        "ai_score": s.ai_score,
        "detection_id": s.detection_id,
    });
    Some(InsertSelectedCandidate {
        setup_id: s.id,
        exchange: s.exchange.clone(),
        symbol: s.symbol.clone(),
        timeframe: s.timeframe.clone(),
        direction,
        entry_price: entry,
        sl_price: sl,
        tp_ladder,
        risk_pct,
        mode: "dry", // Faz 9.8.11 — dry first; live wiring behind a flag in a later step.
        selector_score: s.ai_score.and_then(decimal_from_f32),
        selector_meta: meta,
    })
}

fn decimal_from_f32(v: f32) -> Option<Decimal> {
    Decimal::from_f32_retain(v)
}

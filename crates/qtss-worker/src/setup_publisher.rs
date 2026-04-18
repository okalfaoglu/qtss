//! Faz 9.7.8 — New-setup public broadcast publisher.
//!
//! Polls `setup_broadcast_outbox` for pending rows, loads the setup
//! from `qtss_setups`, builds a [`PublicCard`] with AI brief
//! populated from `raw_meta.ai` + `ai_score`, dispatches the Telegram
//! body via the shared [`NotificationDispatcher`], and enqueues the
//! X-compatible body into `x_outbox` for the X publisher to pick up.
//!
//! Idempotency: one outbox row per setup (UNIQUE). Retries are gated
//! by `setup_publisher.max_attempts`. All knobs live in `qtss_config`.

use std::time::Duration;

use qtss_notify::{
    card::{
        config::{load_category_thresholds, load_tier_thresholds},
        AiBrief, PublicCard, SetupDirection, SetupSnapshot,
    },
    render_public_card, render_public_card_x, NotificationChannel, NotificationDispatcher,
};
use qtss_storage::{
    claim_setup_broadcast_batch, enqueue_x_outbox, fetch_v2_setup, mark_setup_broadcast_failed,
    mark_setup_broadcast_sent, resolve_system_u64, resolve_worker_enabled_flag,
    resolve_worker_tick_secs, SetupBroadcastRow, V2SetupRow, XOutboxInsert,
};
use rust_decimal::Decimal;
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use std::str::FromStr;
use tracing::{debug, info, warn};
use uuid::Uuid;

const MODULE: &str = "notify";

/// Single tick: claim batch, build + dispatch each row.
async fn tick_once(
    pool: &PgPool,
    dispatcher: &NotificationDispatcher,
    batch_limit: i64,
    max_attempts: i32,
) -> usize {
    let rows = match claim_setup_broadcast_batch(pool, batch_limit).await {
        Ok(v) => v,
        Err(e) => {
            warn!(%e, "setup_publisher: claim failed");
            return 0;
        }
    };
    if rows.is_empty() {
        return 0;
    }
    let mut sent = 0usize;
    for row in rows {
        if publish_one(pool, dispatcher, &row, max_attempts).await {
            sent += 1;
        }
    }
    sent
}

async fn publish_one(
    pool: &PgPool,
    dispatcher: &NotificationDispatcher,
    row: &SetupBroadcastRow,
    max_attempts: i32,
) -> bool {
    let setup = match fetch_v2_setup(pool, row.setup_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            // Setup vanished (pruned?). Mark terminally failed so we
            // stop retrying.
            let _ = mark_setup_broadcast_failed(pool, row.id, "setup not found", 0).await;
            return false;
        }
        Err(e) => {
            let _ = mark_setup_broadcast_failed(pool, row.id, &e.to_string(), max_attempts).await;
            return false;
        }
    };

    // Build the channel-agnostic PublicCard once.
    let tier_thresholds = load_tier_thresholds(pool).await;
    let category_thresholds = load_category_thresholds(pool).await;
    let snapshot = build_snapshot(&setup);
    let card = PublicCard::build(pool, snapshot, tier_thresholds, category_thresholds).await;

    // Telegram dispatch (best-effort — failure doesn't block X).
    let tg_notification = render_public_card(&card);
    let telegram_ok = match dispatcher
        .send(NotificationChannel::Telegram, &tg_notification)
        .await
    {
        Ok(_) => true,
        Err(e) => {
            warn!(%e, setup_id = %row.setup_id, "setup_publisher: telegram send failed");
            false
        }
    };

    // X enqueue (to the unified x_outbox — x_publisher handles the
    // venue call + daily cap). Body rendered to ≤280 chars inline.
    let x_body = render_public_card_x(&card);
    let x_insert = XOutboxInsert {
        setup_id: Some(setup.id),
        lifecycle_event_id: None,
        event_key: format!("new_setup:{}", setup.id),
        body: x_body,
        image_path: None,
    };
    let x_ok = match enqueue_x_outbox(pool, &x_insert).await {
        Ok(_) => true,
        Err(e) => {
            warn!(%e, setup_id = %row.setup_id, "setup_publisher: x enqueue failed");
            false
        }
    };

    // Decide outcome. Either channel succeeding counts as sent; if
    // both failed, bounce back to pending (unless past max_attempts).
    if telegram_ok || x_ok {
        if let Err(e) = mark_setup_broadcast_sent(pool, row.id, telegram_ok, x_ok).await {
            warn!(%e, id = %row.id, "setup_publisher: mark_sent failed");
        } else {
            debug!(id = %row.id, telegram_ok, x_ok, "setup_publisher: dispatched");
        }
        true
    } else {
        let _ = mark_setup_broadcast_failed(
            pool,
            row.id,
            "telegram + x both failed",
            max_attempts,
        )
        .await;
        false
    }
}

/// Map a raw `V2SetupRow` into the channel-independent [`SetupSnapshot`]
/// consumed by the card builder. Missing fields degrade gracefully to
/// safe defaults — a render never fails because of sparse metadata.
fn build_snapshot(setup: &V2SetupRow) -> SetupSnapshot {
    let direction = match setup.direction.to_lowercase().as_str() {
        "short" => SetupDirection::Short,
        _ => SetupDirection::Long,
    };
    let meta = &setup.raw_meta;
    let pattern_family = str_from_meta(meta, "pattern_family")
        .or_else(|| str_from_meta(meta, "profile"))
        .unwrap_or_else(|| setup.profile.clone());
    let pattern_subkind = setup.alt_type.clone();
    let ai_brief = build_ai_brief(setup);
    let entry = decimal_from_f32(setup.entry_price).unwrap_or_default();
    let stop = decimal_from_f32(setup.entry_sl).unwrap_or_default();
    let tp1 = decimal_from_f32(setup.target_ref);
    let tp2 = decimal_from_meta(meta, "target_ref2");
    let tp3 = decimal_from_meta(meta, "target_ref3");
    SetupSnapshot {
        setup_id: setup.id,
        exchange: setup.exchange.clone(),
        symbol: setup.symbol.clone(),
        timeframe: setup.timeframe.clone(),
        venue_class: setup.venue_class.clone(),
        market_cap_rank: None,
        direction,
        pattern_family,
        pattern_subkind,
        ai_score: setup
            .ai_score
            .map(|s| s as f64)
            .unwrap_or_else(|| structural_fallback_score(setup)),
        entry_price: entry,
        stop_price: stop,
        tp1_price: tp1,
        tp2_price: tp2,
        tp3_price: tp3,
        current_price: None,
        created_at: setup.created_at,
        ai_brief,
    }
}

/// Derive a minimal AI brief from what the setup engine already wrote
/// into `raw_meta.ai` + the `ai_score` column. Reasoning/top_features
/// stay empty when the decision layer didn't contribute — the
/// renderers skip empty fields gracefully.
fn build_ai_brief(setup: &V2SetupRow) -> Option<AiBrief> {
    let confidence = setup.ai_score.map(|s| s.clamp(0.0, 1.0) as f64);
    let ai_meta = setup.raw_meta.get("ai");
    let reasoning = ai_meta
        .and_then(|v| v.get("reasoning"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let action = ai_meta
        .and_then(|v| v.get("action"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let top_features = ai_meta
        .and_then(|v| v.get("top_features"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .take(3)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if confidence.is_none() && action.is_none() && reasoning.is_none() && top_features.is_empty() {
        return None;
    }
    Some(AiBrief {
        action,
        reasoning,
        confidence,
        top_features,
    })
}

fn decimal_from_f32(v: Option<f32>) -> Option<Decimal> {
    let v = v?;
    if !v.is_finite() {
        return None;
    }
    Decimal::from_str(&format!("{v}")).ok()
}

fn decimal_from_meta(meta: &JsonValue, key: &str) -> Option<Decimal> {
    meta.get(key)
        .and_then(|v| v.as_f64())
        .filter(|v| v.is_finite())
        .and_then(|v| Decimal::from_str(&format!("{v}")).ok())
}

/// Faz 9.7.9 — deterministic structural score used when the LightGBM
/// inference sidecar hasn't produced an `ai_score` yet (model not
/// loaded, sidecar 503, timeout, etc). Without this fallback every
/// un-scored setup lands on `ai_score = 0.0` → Zayif → "3/10", which
/// is misleading once the setup engine itself already has strong
/// structural signals. Formula: detector confidence (`raw_meta.guven`)
/// weighted with the R:R ratio clamped against a 3:1 anchor.
///
/// CLAUDE.md #2: weights live on constants marked for config
/// migration. Move to `system_config.notify.fallback_score.*` once
/// the real AI score is producing calibrated outputs.
const FALLBACK_W_GUVEN: f64 = 0.6;
const FALLBACK_W_RR: f64 = 0.4;
const FALLBACK_RR_ANCHOR: f64 = 3.0;

fn structural_fallback_score(setup: &V2SetupRow) -> f64 {
    let guven = setup
        .raw_meta
        .get("guven")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);
    let rr_norm = setup_rr(setup)
        .map(|rr| (rr / FALLBACK_RR_ANCHOR).clamp(0.0, 1.0))
        .unwrap_or(0.5);
    (FALLBACK_W_GUVEN * guven + FALLBACK_W_RR * rr_norm).clamp(0.0, 1.0)
}

fn setup_rr(setup: &V2SetupRow) -> Option<f64> {
    let entry = setup.entry_price? as f64;
    let stop = setup.entry_sl? as f64;
    let tp = setup.target_ref? as f64;
    let is_long = !setup.direction.eq_ignore_ascii_case("short");
    let (risk, reward) = match is_long {
        true => (entry - stop, tp - entry),
        false => (stop - entry, entry - tp),
    };
    if risk <= 0.0 || reward <= 0.0 {
        return None;
    }
    Some(reward / risk)
}

fn str_from_meta(meta: &JsonValue, key: &str) -> Option<String> {
    meta.get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

pub async fn setup_publisher_loop(pool: PgPool) {
    info!("setup_publisher loop spawned");
    // CLAUDE.md #2: business config (bot_token, chat_id) lives in system_config, not env.
    // `load_notify_config_merged` reads `notify.dispatcher_config` then overlays
    // `notify.telegram_bot_token` + `notify.telegram_chat_id` from DB.
    let ncfg = qtss_ai::notify_telegram_config::load_notify_config_merged(&pool).await;
    let dispatcher = NotificationDispatcher::new(ncfg);
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            MODULE,
            "setup_publisher.enabled",
            "QTSS_SETUP_PUBLISHER_ENABLED",
            false,
        )
        .await;
        if !enabled {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        let tick_secs = resolve_worker_tick_secs(
            &pool,
            MODULE,
            "setup_publisher.tick_secs",
            "QTSS_SETUP_PUBLISHER_TICK_SECS",
            10,
            2,
        )
        .await;
        let batch_limit = resolve_system_u64(
            &pool,
            MODULE,
            "setup_publisher.batch_limit",
            "QTSS_SETUP_PUBLISHER_BATCH_LIMIT",
            20,
            1,
            500,
        )
        .await as i64;
        let max_attempts = resolve_system_u64(
            &pool,
            MODULE,
            "setup_publisher.max_attempts",
            "QTSS_SETUP_PUBLISHER_MAX_ATTEMPTS",
            5,
            1,
            50,
        )
        .await as i32;

        let n = tick_once(&pool, &dispatcher, batch_limit, max_attempts).await;
        if n > 0 {
            debug!(n, "setup_publisher: dispatched");
        }
        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
    }
}

// Silence unused-import in dev profiles.
#[allow(dead_code)]
fn _unused_types(_: Uuid) {}

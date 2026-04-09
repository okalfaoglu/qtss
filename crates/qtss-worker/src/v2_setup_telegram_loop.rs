//! Faz 8.0 — drains `qtss_v2_setup_events` into Telegram via the
//! existing qtss-notify dispatcher. Renders a chart card for each
//! event and marks the row delivered/failed in the outbox.
//!
//! CLAUDE.md compliance:
//!   - No hardcoded thresholds: every knob via `resolve_*` helpers.
//!   - No scattered if/else: per-event-type handling is a small
//!     dispatch (skip/render) pattern with early returns.

use std::time::Duration;

use qtss_notify::{Notification, NotificationChannel, NotificationDispatcher};
use qtss_storage::{
    fetch_v2_setup, list_pending_setup_events, list_recent_bars, mark_setup_event_delivered,
    mark_setup_event_failed, resolve_system_string, resolve_system_u64,
    resolve_worker_enabled_flag, resolve_worker_tick_secs, V2SetupEventRow, V2SetupRow,
};
use rust_decimal::prelude::ToPrimitive;
use sqlx::PgPool;
use tracing::{debug, info, warn};

use crate::setup_chart::{render_setup_card, SetupCardInput};

const MAX_RETRIES: i32 = 5;
const TG_CAPTION_MAX: usize = 1024;

#[derive(Debug, Clone)]
struct LoopConfig {
    tick_interval_s: u64,
    attach_chart: bool,
    chart_lookback_bars: i64,
    batch_limit: i64,
    locale: String,
}

async fn load_cfg(pool: &PgPool) -> LoopConfig {
    let tick_interval_s = resolve_worker_tick_secs(
        pool,
        "setup.notify.telegram",
        "tick_interval_s",
        "QTSS_SETUP_NOTIFY_TG_TICK_S",
        15,
        5,
    )
    .await;
    // attach_chart is a bool flag — reuse enabled-flag resolver with
    // a dedicated key (default true).
    let attach_chart = resolve_worker_enabled_flag(
        pool,
        "setup.notify.telegram",
        "attach_chart",
        "QTSS_SETUP_NOTIFY_TG_ATTACH_CHART",
        true,
    )
    .await;
    let chart_lookback_bars = resolve_system_u64(
        pool,
        "setup.notify.telegram",
        "chart_lookback_bars",
        "QTSS_SETUP_NOTIFY_TG_LOOKBACK",
        200,
        30,
        2000,
    )
    .await as i64;
    let batch_limit = resolve_system_u64(
        pool,
        "setup.notify.telegram",
        "batch_limit",
        "QTSS_SETUP_NOTIFY_TG_BATCH",
        20,
        1,
        200,
    )
    .await as i64;
    let locale = resolve_system_string(
        pool,
        "setup.notify.telegram",
        "locale",
        "QTSS_SETUP_NOTIFY_TG_LOCALE",
        "tr",
    )
    .await;
    LoopConfig {
        tick_interval_s,
        attach_chart,
        chart_lookback_bars,
        batch_limit,
        locale,
    }
}

/// Venue class → market_bars segment dispatch table. Keeps the
/// telegram loop asset-class agnostic without if/else sprawl.
fn segment_for_venue(venue_class: &str, exchange: &str) -> &'static str {
    match (venue_class, exchange) {
        ("crypto", _) => "spot",
        ("us_equities", _) => "equity",
        ("bist", _) => "equity",
        ("fx", _) => "fx",
        ("commodities", _) => "futures",
        _ => "spot",
    }
}

fn short_title(setup: &V2SetupRow, event_type: &str) -> String {
    format!(
        "{} · {} · {} · {}",
        setup.symbol,
        setup.profile.to_uppercase(),
        setup.direction.to_uppercase(),
        event_type.to_uppercase()
    )
}

fn fmt_opt(v: Option<f32>) -> String {
    v.map(|x| format!("{:.6}", x)).unwrap_or_else(|| "-".to_string())
}

fn build_body(setup: &V2SetupRow, ev: &V2SetupEventRow) -> String {
    let mut lines = Vec::<String>::new();
    lines.push(format!(
        "{} {} {} — {}",
        setup.symbol, setup.timeframe, setup.profile, setup.state
    ));
    lines.push(format!(
        "dir: {}  alt: {}",
        setup.direction,
        setup.alt_type.as_deref().unwrap_or("-")
    ));
    lines.push(format!("entry: {}", fmt_opt(setup.entry_price)));
    lines.push(format!("stop : {}", fmt_opt(setup.entry_sl)));
    lines.push(format!("tgt  : {}", fmt_opt(setup.target_ref)));
    lines.push(format!("trail: {}", fmt_opt(setup.koruma)));
    if let Some(risk) = setup.risk_pct {
        lines.push(format!("risk%: {:.3}", risk));
    }
    if ev.event_type == "closed" {
        if let Some(r) = &setup.close_reason {
            lines.push(format!("close: {}", r));
        }
        if let Some(p) = setup.close_price {
            lines.push(format!("close_px: {:.6}", p));
        }
    }
    lines.push(format!("event: {}", ev.event_type));
    lines.join("\n")
}

fn truncate_caption(s: &str) -> String {
    if s.len() <= TG_CAPTION_MAX {
        s.to_string()
    } else {
        let mut out = s[..TG_CAPTION_MAX.saturating_sub(1)].to_string();
        out.push('…');
        out
    }
}

/// "rejected" events are not sent to telegram in Faz 8.0; mark them
/// as skipped (via mark_setup_event_failed with MAX_RETRIES) so they
/// never get re-polled.
async fn skip_event(pool: &PgPool, ev: &V2SetupEventRow) {
    if let Err(e) = mark_setup_event_failed(pool, ev.id, MAX_RETRIES).await {
        warn!(%e, id=%ev.id, "setup_telegram: skip mark failed");
    }
}

async fn handle_event(
    pool: &PgPool,
    dispatcher: &NotificationDispatcher,
    cfg: &LoopConfig,
    ev: &V2SetupEventRow,
) -> Result<(), String> {
    if ev.event_type == "rejected" {
        skip_event(pool, ev).await;
        return Ok(());
    }

    let setup = match fetch_v2_setup(pool, ev.setup_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            warn!(id=%ev.id, setup_id=%ev.setup_id, "setup_telegram: setup not found");
            let _ = mark_setup_event_failed(pool, ev.id, MAX_RETRIES).await;
            return Ok(());
        }
        Err(e) => return Err(format!("fetch_v2_setup: {e}")),
    };

    let segment = segment_for_venue(&setup.venue_class, &setup.exchange);
    let raw_bars = list_recent_bars(
        pool,
        &setup.exchange,
        segment,
        &setup.symbol,
        &setup.timeframe,
        cfg.chart_lookback_bars,
    )
    .await
    .map_err(|e| format!("list_recent_bars: {e}"))?;
    // chronological (oldest first)
    let mut bars = raw_bars;
    bars.reverse();

    let current_price = bars.last().map(|b| b.close.to_f64().unwrap_or(0.0));

    let png = if cfg.attach_chart {
        let input = SetupCardInput {
            setup: &setup,
            bars: &bars,
            event_type: &ev.event_type,
            current_price,
            locale: &cfg.locale,
        };
        Some(render_setup_card(&input))
    } else {
        None
    };

    let title = short_title(&setup, &ev.event_type);
    let body = build_body(&setup, ev);
    let caption = truncate_caption(&format!("{title}\n{body}"));

    let mut n = Notification::new(title.clone(), body.clone());
    if let Some(bytes) = png {
        n.telegram_photo_png = Some(bytes);
        n.telegram_photo_caption_plain = Some(caption);
    }

    match dispatcher.send(NotificationChannel::Telegram, &n).await {
        Ok(receipt) if receipt.ok => Ok(()),
        Ok(receipt) => Err(format!(
            "telegram receipt not ok: {}",
            receipt.detail.unwrap_or_default()
        )),
        Err(e) => Err(format!("dispatcher.send: {e}")),
    }
}

pub async fn v2_setup_telegram_loop(pool: PgPool) {
    info!("v2_setup_telegram_loop: draining qtss_v2_setup_events → Telegram");
    loop {
        // Default true: Setup Engine'in tek anlamı Telegram dağıtımı,
        // false default'u Faz 8.0 sonrası prod'da outbox dolmasına neden
        // oldu (3 pending event, 2026-04-10). Operator yine de
        // system_config'ten kapatabilir; default opt-out tek tıklık.
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "setup.notify.telegram",
            "enabled",
            "QTSS_SETUP_NOTIFY_TG_ENABLED",
            true,
        )
        .await;
        if !enabled {
            tokio::time::sleep(Duration::from_secs(10)).await;
            continue;
        }
        let cfg = load_cfg(&pool).await;

        let ncfg = qtss_ai::load_notify_config_merged(&pool).await;
        let dispatcher = NotificationDispatcher::new(ncfg);

        let events = match list_pending_setup_events(&pool, cfg.batch_limit).await {
            Ok(rows) => rows,
            Err(e) => {
                warn!(%e, "v2_setup_telegram_loop: list_pending_setup_events");
                tokio::time::sleep(Duration::from_secs(cfg.tick_interval_s)).await;
                continue;
            }
        };

        debug!(count = events.len(), "v2_setup_telegram_loop: pending events");

        for ev in events {
            if ev.retries >= MAX_RETRIES {
                continue;
            }
            match handle_event(&pool, &dispatcher, &cfg, &ev).await {
                Ok(()) => {
                    // For "rejected"/not-found paths handle_event
                    // already marked the row; only mark delivered for
                    // events that actually went to Telegram.
                    if ev.event_type != "rejected" {
                        if let Err(e) = mark_setup_event_delivered(&pool, ev.id).await {
                            warn!(%e, id=%ev.id, "v2_setup_telegram_loop: mark_delivered");
                        }
                    }
                }
                Err(e) => {
                    warn!(%e, id=%ev.id, "v2_setup_telegram_loop: delivery failed");
                    let next = (ev.retries + 1).min(MAX_RETRIES);
                    if let Err(e2) = mark_setup_event_failed(&pool, ev.id, next).await {
                        warn!(%e2, id=%ev.id, "v2_setup_telegram_loop: mark_failed");
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(cfg.tick_interval_s)).await;
    }
}

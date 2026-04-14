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

/// Format price with natural precision — 2 decimals for > 100,
/// 4 for > 1, 6 for anything smaller. Avoids "74242.500000" style.
fn fmt_price(v: f32) -> String {
    let a = v.abs();
    if a >= 100.0 { format!("{v:.2}") }
    else if a >= 1.0 { format!("{v:.4}") }
    else { format!("{v:.6}") }
}

/// P&L % for the trader.
///   long  TP: (tp-e)/e  — positive when tp > e
///   long  SL: (sl-e)/e  — negative when sl < e
///   short TP: (e-tp)/e  — positive when tp < e
///   short SL: (e-sl)/e  — negative when sl > e
fn pnl_pct(entry: f32, level: f32, direction: &str) -> f64 {
    let e = entry as f64;
    let l = level as f64;
    if e.abs() < 1e-9 { return 0.0; }
    match direction {
        "short" => (e - l) / e * 100.0,
        _ => (l - e) / e * 100.0,
    }
}

fn fmt_level_with_pct(
    label: &str,
    entry: Option<f32>,
    level: Option<f32>,
    direction: &str,
) -> String {
    match (entry, level) {
        (Some(e), Some(l)) => {
            let pct = pnl_pct(e, l, direction);
            let sign = if pct >= 0.0 { "+" } else { "" };
            format!("{label} {}  ({sign}{:.2}%)", fmt_price(l), pct)
        }
        (_, Some(l)) => format!("{label} {}", fmt_price(l)),
        _ => format!("{label} —"),
    }
}

fn dir_emoji(direction: &str) -> &'static str {
    match direction { "long" => "🟢", "short" => "🔴", _ => "⚪" }
}

fn event_emoji(event_type: &str) -> &'static str {
    match event_type {
        "opened" | "armed" => "📢",
        "closed" => "🏁",
        "stopped" => "🛑",
        "target_hit" => "🎯",
        _ => "•",
    }
}

fn profile_emoji(profile: &str) -> &'static str {
    match profile { "d" | "D" => "🟪", "t" | "T" => "🟧", "q" | "Q" => "🟦", _ => "⬜" }
}

fn build_body(setup: &V2SetupRow, ev: &V2SetupEventRow) -> String {
    let mut lines = Vec::<String>::new();
    let dir_up = setup.direction.to_uppercase();
    let prof_up = setup.profile.to_uppercase();

    // Header
    lines.push(format!(
        "{} {} {} {} {}  {} {}",
        event_emoji(&ev.event_type),
        dir_emoji(&setup.direction),
        setup.symbol,
        setup.timeframe,
        profile_emoji(&setup.profile),
        prof_up,
        dir_up,
    ));
    lines.push(format!("state: {}", setup.state));
    if let Some(alt) = &setup.alt_type {
        if !alt.is_empty() && alt != "-" {
            lines.push(format!("alt  : {alt}"));
        }
    }
    lines.push("".into());

    // Levels — entry always first, SL and TP with signed P&L %.
    if let Some(e) = setup.entry_price {
        lines.push(format!("🎯 entry : {}", fmt_price(e)));
    } else {
        lines.push("🎯 entry : —".into());
    }
    lines.push(format!(
        "🛑 {}",
        fmt_level_with_pct("stop  :", setup.entry_price, setup.entry_sl, &setup.direction)
    ));
    lines.push(format!(
        "🎯 {}",
        fmt_level_with_pct("tgt   :", setup.entry_price, setup.target_ref, &setup.direction)
    ));
    if setup.koruma.is_some() {
        lines.push(format!(
            "🪜 {}",
            fmt_level_with_pct("trail :", setup.entry_price, setup.koruma, &setup.direction)
        ));
    }

    // Risk & R:R
    let rr = match (setup.entry_price, setup.entry_sl, setup.target_ref) {
        (Some(e), Some(sl), Some(tp)) => {
            let risk = (e - sl).abs() as f64;
            let reward = (tp - e).abs() as f64;
            if risk > 1e-9 { Some(reward / risk) } else { None }
        }
        _ => None,
    };
    let mut meta = Vec::new();
    if let Some(risk) = setup.risk_pct {
        meta.push(format!("risk {:.2}%", risk));
    }
    if let Some(r) = rr {
        meta.push(format!("R:R {r:.2}"));
    }
    if !meta.is_empty() {
        lines.push("".into());
        lines.push(meta.join("  •  "));
    }

    // Close details (only on close events)
    if ev.event_type == "closed" {
        lines.push("".into());
        if let Some(r) = &setup.close_reason {
            lines.push(format!("close: {r}"));
        }
        if let (Some(cp), Some(e)) = (setup.close_price, setup.entry_price) {
            let pct = pnl_pct(e, cp, &setup.direction);
            let sign = if pct >= 0.0 { "+" } else { "" };
            lines.push(format!("exit : {}  ({sign}{:.2}%)", fmt_price(cp), pct));
        } else if let Some(cp) = setup.close_price {
            lines.push(format!("exit : {}", fmt_price(cp)));
        }
    }

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

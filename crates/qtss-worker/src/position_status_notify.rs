//! Hourly open-position status reports and close-result notifications.
//!
//! Enable: `system_config` `worker.position_status_notify_enabled` / `QTSS_POSITION_STATUS_NOTIFY_ENABLED`.
//! Channels: `worker.position_status_notify_channels_csv` / `QTSS_POSITION_STATUS_NOTIFY_CHANNELS`.
//!
//! Sends a Telegram message for each open paper position once per hour with current
//! PnL, SL, TP, and entry/mark prices.  When a position is fully closed (qty drops
//! to zero), a one-time close-result message is sent.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use qtss_notify::{
    escape_telegram_html, Notification, NotificationChannel, NotificationDispatcher,
};
use qtss_storage::{
    list_recent_bars, resolve_system_csv, resolve_worker_enabled_flag, resolve_worker_tick_secs,
    PaperFillRow, PaperLedgerRepository,
};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::{debug, info, warn};

async fn status_notify_enabled(pool: &PgPool) -> bool {
    resolve_worker_enabled_flag(
        pool,
        "worker",
        "position_status_notify_enabled",
        "QTSS_POSITION_STATUS_NOTIFY_ENABLED",
        false,
    )
    .await
}

async fn status_notify_channels(pool: &PgPool) -> Vec<NotificationChannel> {
    let raw = resolve_system_csv(
        pool,
        "worker",
        "position_status_notify_channels_csv",
        "QTSS_POSITION_STATUS_NOTIFY_CHANNELS",
        "telegram",
    )
    .await;
    raw.iter()
        .filter_map(|s| NotificationChannel::parse(s.trim()))
        .collect()
}

fn strength_bar(pnl_pct: f64) -> String {
    let score = ((pnl_pct.abs().min(10.0) / 10.0) * 10.0).round() as i32;
    let filled = score.clamp(0, 10) as usize;
    let empty = 10 - filled;
    let label = if pnl_pct.abs() < 1.0 {
        "ZAYIF"
    } else if pnl_pct.abs() < 3.0 {
        "ORTA"
    } else if pnl_pct.abs() < 6.0 {
        "GÜÇLÜ"
    } else {
        "ÇOK GÜÇLÜ"
    };
    format!(
        "[{}{}] {}/10 {}",
        "■".repeat(filled),
        "□".repeat(empty),
        score,
        label
    )
}

fn build_open_position_html(
    symbol: &str,
    side: &str,
    entry_price: Decimal,
    mark_price: Decimal,
    pnl_pct: f64,
) -> String {
    let arrow = if pnl_pct >= 0.0 { "▲" } else { "▼" };
    let pnl_sign = if pnl_pct >= 0.0 { "+" } else { "" };
    let side_upper = side.to_uppercase();
    let bar = strength_bar(pnl_pct);

    format!(
        "📊 <b>CANLI POZİSYON</b>\n\
         \n\
         📈 <b>{} · {}</b>\n\
         <code>{}</code> ──▶ <code>{}</code>\n\
         {} <b>{}{:.2}%</b>\n\
         \n\
         <b>Güç</b> {}",
        escape_telegram_html(symbol),
        escape_telegram_html(&side_upper),
        entry_price,
        mark_price,
        arrow,
        pnl_sign,
        pnl_pct,
        escape_telegram_html(&bar),
    )
}

fn build_close_result_html(
    symbol: &str,
    side: &str,
    entry_price: Decimal,
    exit_price: Decimal,
    pnl_pct: f64,
) -> String {
    let pnl_sign = if pnl_pct >= 0.0 { "+" } else { "" };
    let emoji = if pnl_pct >= 0.0 { "💰" } else { "📉" };
    let side_upper = side.to_uppercase();

    format!(
        "✅ <b>İŞLEM SONUCU</b>\n\
         \n\
         📈 <b>{} · {}</b>\n\
         <code>{}</code> ──▶ <code>{}</code>\n\
         \n\
         {} <b>{}{:.2}%</b>",
        escape_telegram_html(symbol),
        escape_telegram_html(&side_upper),
        entry_price,
        exit_price,
        emoji,
        pnl_sign,
        pnl_pct,
    )
}

struct OpenPosition {
    symbol: String,
    side: String,
    qty: Decimal,
    avg_entry: Decimal,
}

async fn get_mark(pool: &PgPool, symbol: &str) -> Option<Decimal> {
    let bars = list_recent_bars(pool, "binance", "futures", symbol, "15m", 1)
        .await
        .ok()?;
    bars.into_iter().next().map(|b| b.close)
}

fn aggregate_paper_positions(fills: &[PaperFillRow]) -> Vec<OpenPosition> {
    let mut positions: HashMap<String, (Decimal, Decimal, String)> = HashMap::new();
    let mut sorted = fills.to_vec();
    sorted.sort_by_key(|f| f.created_at);

    for f in &sorted {
        let sym = f.symbol.trim().to_uppercase();
        let e = positions
            .entry(sym.clone())
            .or_insert((Decimal::ZERO, Decimal::ZERO, f.side.clone()));
        let side_lower = f.side.to_lowercase();
        match side_lower.as_str() {
            "buy" => {
                e.1 += f.avg_price * f.quantity;
                e.0 += f.quantity;
                e.2 = "LONG".into();
            }
            "sell" => {
                if e.0 > Decimal::ZERO {
                    let avg = e.1 / e.0;
                    let take = f.quantity.min(e.0);
                    e.1 -= avg * take;
                    e.0 -= take;
                } else {
                    e.1 += f.avg_price * f.quantity;
                    e.0 += f.quantity;
                    e.2 = "SHORT".into();
                }
            }
            _ => {}
        }
    }

    positions
        .into_iter()
        .filter(|(_, (qty, _, _))| *qty > Decimal::new(1, 8))
        .map(|(sym, (qty, cost, side))| {
            let avg_entry = if qty > Decimal::ZERO {
                cost / qty
            } else {
                Decimal::ZERO
            };
            OpenPosition {
                symbol: sym,
                side,
                qty,
                avg_entry,
            }
        })
        .collect()
}

async fn send_telegram_html(dispatcher: &NotificationDispatcher, channels: &[NotificationChannel], title: &str, html: &str) {
    let n = Notification::new(title, html).with_telegram_html_message(html.to_string());
    let receipts = dispatcher.send_all(channels, &n).await;
    for r in &receipts {
        if !r.ok {
            warn!(channel = ?r.channel, detail = ?r.detail, "position_status_notify: send failed");
        }
    }
}

pub async fn position_status_notify_loop(pool: PgPool) {
    let enabled = status_notify_enabled(&pool).await;
    if !enabled {
        info!("QTSS_POSITION_STATUS_NOTIFY_ENABLED off — position status notify loop exiting");
        return;
    }
    let channels = status_notify_channels(&pool).await;
    if channels.is_empty() {
        warn!("position status notify enabled but channels empty");
        return;
    }
    let repo = PaperLedgerRepository::new(pool.clone());
    let mut known_open: HashSet<String> = HashSet::new();

    info!("position_status_notify_loop started (hourly reports + close results)");

    loop {
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "position_status_notify_tick_secs",
            "QTSS_POSITION_STATUS_NOTIFY_TICK_SECS",
            3600,
            300,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(tick)).await;

        if !status_notify_enabled(&pool).await {
            continue;
        }

        let dispatcher = NotificationDispatcher::from_env();

        let cutoff = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(7))
            .unwrap_or_else(chrono::Utc::now);
        let fills = match repo.list_fills_created_after(cutoff, 500).await {
            Ok(f) => f,
            Err(e) => {
                warn!(%e, "position_status_notify: fills query failed");
                continue;
            }
        };

        let positions = aggregate_paper_positions(&fills);
        let current_symbols: HashSet<String> =
            positions.iter().map(|p| p.symbol.clone()).collect();

        // Detect closed positions and send close-result messages.
        for closed_sym in known_open.difference(&current_symbols) {
            let last_fill = fills
                .iter()
                .rev()
                .find(|f| f.symbol.eq_ignore_ascii_case(closed_sym));
            if let Some(fill) = last_fill {
                let entry_fill = fills.iter().find(|f| {
                    f.symbol.eq_ignore_ascii_case(closed_sym)
                        && f.side.eq_ignore_ascii_case("buy")
                });
                let entry_px = entry_fill.map(|f| f.avg_price).unwrap_or(fill.avg_price);
                let exit_px = fill.avg_price;
                let pnl_pct = if entry_px > Decimal::ZERO {
                    ((exit_px - entry_px) / entry_px * Decimal::from(100))
                        .to_f64()
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
                let side = entry_fill
                    .map(|f| {
                        if f.side.eq_ignore_ascii_case("buy") {
                            "LONG"
                        } else {
                            "SHORT"
                        }
                    })
                    .unwrap_or("LONG");
                let html =
                    build_close_result_html(closed_sym, side, entry_px, exit_px, pnl_pct);
                let title = format!(
                    "Trade closed — {} {}{:.2}%",
                    closed_sym,
                    if pnl_pct >= 0.0 { "+" } else { "" },
                    pnl_pct
                );
                send_telegram_html(&dispatcher, &channels, &title, &html).await;
                info!(symbol = %closed_sym, pnl_pct, "position close result sent");
            }
        }
        known_open = current_symbols;

        // Send hourly status for each open position.
        for pos in &positions {
            let mark = match get_mark(&pool, &pos.symbol).await {
                Some(m) => m,
                None => {
                    debug!(symbol = %pos.symbol, "position_status: no mark price");
                    continue;
                }
            };
            let pnl_pct = if pos.avg_entry > Decimal::ZERO {
                let raw = (mark - pos.avg_entry) / pos.avg_entry * Decimal::from(100);
                let pct = raw.to_f64().unwrap_or(0.0);
                if pos.side == "SHORT" {
                    -pct
                } else {
                    pct
                }
            } else {
                0.0
            };

            let html =
                build_open_position_html(&pos.symbol, &pos.side, pos.avg_entry, mark, pnl_pct);
            let title = format!(
                "Position — {} {} {}{:.2}%",
                pos.symbol,
                pos.side,
                if pnl_pct >= 0.0 { "+" } else { "" },
                pnl_pct
            );
            send_telegram_html(&dispatcher, &channels, &title, &html).await;
        }
        if !positions.is_empty() {
            info!(count = positions.len(), "position status reports sent");
        }
    }
}

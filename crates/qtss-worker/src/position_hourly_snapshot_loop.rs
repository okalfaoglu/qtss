// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! `position_hourly_snapshot_loop` — Telegram "CANLI POZİSYON"
//! digests for every open setup, once per UTC hour.
//!
//! Every top-of-hour boundary the loop walks open `qtss_setups` rows,
//! reads the live bookTicker price + the ratchet SL + health score
//! from the watcher view, and enqueues a Telegram-formatted card into
//! `notify_outbox` with
//! `dedup_key = 'hourly:{setup_id}:{yyyymmddHH}'`. The partial unique
//! index on the column silently swallows duplicates so a worker
//! restart inside the same hour never re-sends a card.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Datelike, Timelike, Utc};
use qtss_notify::PriceTickStore;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::{debug, info, warn};

pub async fn position_hourly_snapshot_loop(pool: PgPool, price_store: Arc<PriceTickStore>) {
    info!("position_hourly_snapshot_loop: started");
    loop {
        let enabled = load_enabled(&pool).await;
        if !enabled {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }
        match run_tick(&pool, &price_store).await {
            Ok(n) if n > 0 => info!(rows = n, "position_hourly_snapshot tick"),
            Ok(_) => debug!("position_hourly_snapshot: no rows"),
            Err(e) => warn!(%e, "position_hourly_snapshot tick failed"),
        }
        // Tight polling cheap: the dedup_key swallows same-hour
        // duplicates, and we want to fire within seconds of the hour
        // rollover without a long-running timer.
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='worker' AND config_key='position_hourly_snapshot_enabled'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return true; }; // opt-out, not opt-in
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

async fn run_tick(pool: &PgPool, price_store: &PriceTickStore) -> anyhow::Result<usize> {
    let now = Utc::now();
    let hour_bucket = hour_bucket_key(now);
    let rows = sqlx::query(
        r#"SELECT id, exchange, symbol, timeframe, direction, mode,
                  entry_price, entry_sl, current_sl, koruma, target_ref,
                  created_at, tp_ladder, raw_meta
             FROM qtss_setups
            WHERE state IN ('armed','active')
              AND closed_at IS NULL"#,
    )
    .fetch_all(pool)
    .await?;

    let mut written = 0usize;
    for row in rows {
        let setup_id: sqlx::types::Uuid = row.try_get("id")?;
        let exchange: String = row.try_get("exchange").unwrap_or_default();
        let symbol: String = row.try_get("symbol").unwrap_or_default();
        let timeframe: String = row.try_get("timeframe").unwrap_or_default();
        let direction: String = row.try_get("direction").unwrap_or_default();
        let mode: String = row.try_get("mode").unwrap_or_default();
        let entry: Option<f32> = row.try_get("entry_price").ok();
        let sl: Option<f32> = row.try_get("entry_sl").ok();
        let current_sl: Option<f32> = row.try_get("current_sl").ok();
        let koruma: Option<f32> = row.try_get("koruma").ok();
        let target_ref: Option<f32> = row.try_get("target_ref").ok();
        let tp_ladder: Value = row
            .try_get("tp_ladder")
            .unwrap_or(Value::Array(Vec::new()));
        let created_at: DateTime<Utc> = row.try_get("created_at").unwrap_or_else(|_| Utc::now());
        let live_px = price_store
            .get("binance", &symbol)
            .and_then(|t| t.mid().to_f64());
        let payload = json!({
            "setup_id": setup_id.to_string(),
            "symbol": symbol,
            "timeframe": timeframe,
            "direction": direction,
            "mode": mode,
            "entry_price": entry,
            "entry_sl": sl,
            "current_sl": current_sl,
            "koruma": koruma,
            "target_ref": target_ref,
            "tp_ladder": tp_ladder,
            "live_price": live_px,
            "created_at": created_at,
            "hour_bucket": hour_bucket,
        });
        let title = build_title(&direction, &symbol, &timeframe, entry, live_px);
        let body = render_position_html(&payload);
        let dedup_key = format!("hourly:{setup_id}:{hour_bucket}");
        let ins = sqlx::query(
            r#"INSERT INTO notify_outbox
                  (title, body, channels, severity, event_key,
                   exchange, segment, symbol, status, dedup_key)
               VALUES ($1, $2, '["telegram"]'::jsonb, 'info',
                       'position_hourly_snapshot',
                       'binance', 'futures', $3, 'pending', $4)
               ON CONFLICT (dedup_key) WHERE dedup_key IS NOT NULL DO NOTHING"#,
        )
        .bind(&title)
        .bind(&body)
        .bind(&symbol)
        .bind(&dedup_key)
        .execute(pool)
        .await;
        match ins {
            Ok(r) if r.rows_affected() > 0 => written += 1,
            Ok(_) => {} // dedup hit — same hour already sent
            Err(e) => warn!(%e, %symbol, "hourly_snapshot insert"),
        }
    }
    Ok(written)
}

fn hour_bucket_key(now: DateTime<Utc>) -> String {
    format!(
        "{:04}{:02}{:02}{:02}",
        now.year(),
        now.month(),
        now.day(),
        now.hour(),
    )
}

fn build_title(
    direction: &str,
    symbol: &str,
    timeframe: &str,
    entry: Option<f32>,
    live: Option<f64>,
) -> String {
    let dir_icon = if direction == "long" { "🟢" } else { "🔴" };
    let e = entry.unwrap_or(0.0);
    let l = live.unwrap_or(e as f64);
    let pct = if e > 0.0 {
        let raw = ((l - e as f64) / e as f64) * 100.0;
        if direction == "long" { raw } else { -raw }
    } else {
        0.0
    };
    format!(
        "🔄 {dir_icon} {symbol} {timeframe}  @ {:.6} → {:.6}  ({:+.2}%)",
        e, l, pct
    )
}

fn render_position_html(p: &Value) -> String {
    let symbol = p.get("symbol").and_then(|v| v.as_str()).unwrap_or("?");
    let timeframe = p.get("timeframe").and_then(|v| v.as_str()).unwrap_or("?");
    let direction = p.get("direction").and_then(|v| v.as_str()).unwrap_or("?");
    let mode = p.get("mode").and_then(|v| v.as_str()).unwrap_or("?");
    let entry = p.get("entry_price").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let live = p.get("live_price").and_then(|v| v.as_f64()).unwrap_or(entry);
    let entry_sl = p.get("entry_sl").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let current_sl = p
        .get("current_sl")
        .and_then(|v| v.as_f64())
        .unwrap_or(entry_sl);
    let koruma = p.get("koruma").and_then(|v| v.as_f64());
    let target_ref = p
        .get("target_ref")
        .and_then(|v| v.as_f64())
        .unwrap_or(entry);
    let is_long = direction == "long";
    let dir_label = if is_long { "LONG" } else { "SHORT" };
    let dir_emoji = if is_long { "🟢" } else { "🔴" };

    let raw = if entry > 0.0 {
        ((live - entry) / entry) * 100.0
    } else {
        0.0
    };
    let u_pnl_pct = if is_long { raw } else { -raw };
    let pnl_emoji = if u_pnl_pct >= 0.0 { "▲" } else { "▼" };

    let tp_ladder = p.get("tp_ladder").and_then(|v| v.as_array());
    let tp_count = tp_ladder.map(|a| a.len()).unwrap_or(0);

    let mut html = String::with_capacity(768);
    html.push_str("<b>🔄 CANLI POZİSYON</b>\n");
    html.push_str(&format!(
        "<b>📊 {dir_emoji} {symbol}</b> · <code>{timeframe}</code> · <i>{mode}</i>\n"
    ));
    html.push_str(&format!(
        "<b>{dir_label}</b>  <code>{entry:.6}</code> → <code>{live:.6}</code>\n"
    ));
    html.push_str(&format!(
        "{pnl_emoji} <b>{:+.2}%</b> (u-PnL)\n\n",
        u_pnl_pct
    ));

    html.push_str(&format!(
        "<b>Stop Loss:</b>    <code>{:.6}</code>\n",
        entry_sl
    ));
    html.push_str(&format!(
        "<b>Take Profit:</b>  <code>{:.6}</code>",
        target_ref
    ));
    if tp_count > 1 {
        html.push_str(&format!("  <i>(TP ladder: {tp_count} seviye)</i>"));
    }
    html.push('\n');
    if let Some(k) = koruma {
        if (k - entry_sl).abs() > 1e-9 {
            let emoji = if (k > entry_sl && is_long) || (k < entry_sl && !is_long) {
                "🛡️"
            } else {
                "·"
            };
            html.push_str(&format!("<b>Koruma:</b>       <code>{:.6}</code> {}\n", k, emoji));
        }
    }
    if (current_sl - entry_sl).abs() > 1e-9 {
        html.push_str(&format!(
            "<b>Güncel SL:</b>    <code>{:.6}</code> <i>(ratchet)</i>\n",
            current_sl
        ));
    }
    // Süre bilgisi.
    if let Some(opened_at) = p.get("created_at").and_then(|v| v.as_str()) {
        if let Ok(ts) = DateTime::parse_from_rfc3339(opened_at) {
            let secs = (Utc::now() - ts.with_timezone(&Utc)).num_seconds();
            if secs > 0 {
                html.push_str(&format!("<i>Süre:</i> {}\n", fmt_duration_secs(secs)));
            }
        }
    }

    html
}

fn fmt_duration_secs(secs: i64) -> String {
    if secs < 60 {
        format!("{secs} sn")
    } else if secs < 3600 {
        format!("{} dk", secs / 60)
    } else if secs < 86400 {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        format!("{h} sa {m} dk")
    } else {
        let d = secs / 86400;
        let h = (secs % 86400) / 3600;
        format!("{d} g {h} sa")
    }
}

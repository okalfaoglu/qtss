use std::time::Duration;

use chrono::TimeZone;
use futures_util::StreamExt;
use qtss_binance::{
    connect_url, spot_user_data_stream_url, usdm_user_data_stream_url, BinanceClient,
    BinanceClientConfig,
};
use qtss_common::env_override;
use qtss_storage::{
    ExchangeAccountRepository, ExchangeFillRepository, ExchangeOrderRepository, NotifyOutboxRepository,
};
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

fn bool_env(name: &str) -> bool {
    matches!(
        std::env::var(name).unwrap_or_default().trim(),
        "1" | "true" | "TRUE" | "yes" | "YES"
    )
}

fn user_stream_enabled() -> bool {
    bool_env("QTSS_BINANCE_USER_STREAM_ENABLED")
}

fn user_stream_keepalive_secs() -> u64 {
    let raw = env_override("QTSS_BINANCE_USER_STREAM_KEEPALIVE_SECS").unwrap_or_else(|| "1800".into());
    raw.trim().parse::<u64>().ok().unwrap_or(1800).clamp(60, 3600)
}

fn notify_enabled() -> bool {
    bool_env("QTSS_BINANCE_USER_STREAM_NOTIFY_ENABLED")
}

fn notify_channels() -> Vec<String> {
    let raw = env_override("QTSS_BINANCE_USER_STREAM_NOTIFY_CHANNELS").unwrap_or_default();
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn is_terminal_status(status: &str) -> bool {
    matches!(status, "filled" | "canceled" | "rejected" | "expired")
}

fn severity_for_terminal_status(status: &str) -> &'static str {
    match status {
        "filled" => "info",
        "canceled" | "expired" => "warn",
        "rejected" => "error",
        _ => "info",
    }
}

fn is_futures_order_trade_update(v: &Value) -> bool {
    v.get("e")
        .and_then(Value::as_str)
        .map(|s| s == "ORDER_TRADE_UPDATE")
        .unwrap_or(false)
}

fn extract_futures_order_update_fields(v: &Value) -> Option<(i64, &str)> {
    let o = v.get("o")?;
    let venue_order_id = o.get("i")?.as_i64()?;
    let status = o.get("X")?.as_str()?;
    Some((venue_order_id, status))
}

fn parse_decimal_str(v: &Value) -> Option<Decimal> {
    let s = v.as_str()?.trim();
    if s.is_empty() {
        return None;
    }
    Decimal::from_str_exact(s).ok()
}

fn normalize_binance_order_status(raw: &str) -> &'static str {
    match raw.trim() {
        "NEW" => "open",
        "PARTIALLY_FILLED" => "partially_filled",
        "FILLED" => "filled",
        "CANCELED" => "canceled",
        "REJECTED" => "rejected",
        "EXPIRED" => "expired",
        other if other.is_empty() => "open",
        _ => "open",
    }
}

fn is_spot_execution_report(v: &Value) -> bool {
    v.get("e")
        .and_then(Value::as_str)
        .map(|s| s == "executionReport")
        .unwrap_or(false)
}

fn extract_spot_execution_report_fields(v: &Value) -> Option<(i64, &str)> {
    let venue_order_id = v.get("i")?.as_i64()?;
    let status = v.get("X")?.as_str()?;
    Some((venue_order_id, status))
}

async fn futures_user_stream_for_user(pool: PgPool, user_id: Uuid) {
    let accounts = ExchangeAccountRepository::new(pool.clone());
    let orders = ExchangeOrderRepository::new(pool.clone());
    let fills = ExchangeFillRepository::new(pool.clone());
    let outbox = NotifyOutboxRepository::new(pool.clone());

    let creds = match accounts.binance_for_user(user_id, "futures").await {
        Ok(Some(c)) => c,
        Ok(None) => return,
        Err(e) => {
            warn!(%e, %user_id, "binance futures user stream: credentials lookup failed");
            return;
        }
    };

    let cfg = BinanceClientConfig::mainnet_with_keys(creds.api_key, creds.api_secret);
    let client = match BinanceClient::new(cfg) {
        Ok(c) => c,
        Err(e) => {
            warn!(%user_id, err = %e, "binance futures user stream: client init failed");
            return;
        }
    };

    let keepalive_secs = user_stream_keepalive_secs();
    loop {
        let listen_key = match client.fapi_user_data_stream_start().await {
            Ok(v) => v.get("listenKey").and_then(Value::as_str).map(str::to_string),
            Err(e) => {
                warn!(%user_id, err = %e, "binance futures user stream: listenKey start failed");
                None
            }
        };

        let Some(listen_key) = listen_key else {
            tokio::time::sleep(Duration::from_secs(10)).await;
            continue;
        };

        let url = usdm_user_data_stream_url(&listen_key);
        info!(%user_id, %url, "binance futures user stream connecting");

        let mut ws = match connect_url(&url).await {
            Ok(ws) => ws,
            Err(e) => {
                warn!(%user_id, err = %e, "binance futures user stream: ws connect failed");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let mut last_keepalive = tokio::time::Instant::now();
        loop {
            tokio::select! {
                msg = ws.next() => {
                    let Some(msg) = msg else {
                        warn!(%user_id, "binance futures user stream: ws closed");
                        break;
                    };
                    let Ok(msg) = msg else {
                        warn!(%user_id, "binance futures user stream: ws read error");
                        break;
                    };

                    let text = match msg {
                        tokio_tungstenite::tungstenite::protocol::Message::Text(t) => t,
                        tokio_tungstenite::tungstenite::protocol::Message::Binary(b) => String::from_utf8_lossy(&b).to_string(),
                        tokio_tungstenite::tungstenite::protocol::Message::Close(_) => break,
                        _ => continue,
                    };

                    let Ok(v) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    if !is_futures_order_trade_update(&v) {
                        continue;
                    }
                    let Some((venue_order_id, status)) = extract_futures_order_update_fields(&v) else {
                        continue;
                    };
                    let normalized = normalize_binance_order_status(status);

                    // Try to insert a fill record if this update carries a last fill.
                    if let Some(o) = v.get("o") {
                        let last_qty = o.get("l").and_then(parse_decimal_str);
                        let last_price = o.get("L").and_then(parse_decimal_str);
                        if let (Some(q), Some(p)) = (last_qty, last_price) {
                            if q > Decimal::ZERO && p > Decimal::ZERO {
                                let fee = o.get("n").and_then(parse_decimal_str);
                                let fee_asset = o.get("N").and_then(Value::as_str);
                                let symbol = o.get("s").and_then(Value::as_str).unwrap_or("");
                                let trade_id = o.get("t").and_then(Value::as_i64);
                                let event_ms = v.get("T").and_then(Value::as_i64);
                                let event_time = event_ms.and_then(|ms| chrono::Utc.timestamp_millis_opt(ms).single());

                                if let Ok(Some(org_id)) = orders
                                    .fetch_org_id_for_venue_order(user_id, "binance", "futures", venue_order_id)
                                    .await
                                {
                                    let inserted = fills
                                        .insert_if_absent(
                                            org_id,
                                            user_id,
                                            "binance",
                                            "futures",
                                            symbol,
                                            venue_order_id,
                                            trade_id,
                                            Some(p),
                                            Some(q),
                                            fee,
                                            fee_asset,
                                            event_time,
                                            Some(v.clone()),
                                        )
                                        .await;
                                    if notify_enabled() {
                                        if let Ok(Some(_row)) = inserted {
                                            let ch = notify_channels();
                                            if !ch.is_empty() {
                                                let title = format!("Fill (Binance futures) {symbol}");
                                                let body = format!(
                                                    "order_id={venue_order_id} qty={q} price={p} fee={} {}",
                                                    fee.map(|x| x.to_string()).unwrap_or_else(|| "-".into()),
                                                    fee_asset.unwrap_or("-")
                                                );
                                                let _ = outbox
                                                    .enqueue_with_meta(
                                                        Some(org_id),
                                                        Some("binance.user_stream.fill"),
                                                        "info",
                                                        Some("binance"),
                                                        Some("futures"),
                                                        Some(symbol),
                                                        &title,
                                                        &body,
                                                        ch,
                                                    )
                                                    .await;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Best-effort: attach WS event while order is not terminal.
                    match orders.update_status_and_venue_response_if_not_terminal(
                        user_id,
                        "binance",
                        "futures",
                        venue_order_id,
                        normalized,
                        &v,
                    ).await {
                        Ok(n) => {
                            if n > 0 {
                                info!(%user_id, venue_order_id, raw_status=%status, status=%normalized, rows=n, "exchange_orders updated from ws");
                                if notify_enabled() && is_terminal_status(normalized) {
                                    if let Ok(Some(org_id)) = orders
                                        .fetch_org_id_for_venue_order(user_id, "binance", "futures", venue_order_id)
                                        .await
                                    {
                                        let symbol = orders
                                            .fetch_symbol_for_venue_order(user_id, "binance", "futures", venue_order_id)
                                            .await
                                            .ok()
                                            .flatten();
                                        let ch = notify_channels();
                                        if !ch.is_empty() {
                                            let title = format!("Order {normalized} (Binance futures)");
                                            let body = format!("order_id={venue_order_id} raw_status={status}");
                                            let _ = outbox
                                                .enqueue_with_meta(
                                                    Some(org_id),
                                                    Some("binance.user_stream.order_terminal"),
                                                    severity_for_terminal_status(normalized),
                                                    Some("binance"),
                                                    Some("futures"),
                                                    symbol.as_deref(),
                                                    &title,
                                                    &body,
                                                    ch,
                                                )
                                                .await;
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => warn!(%user_id, venue_order_id, err=%e, "exchange_orders ws update failed"),
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    if last_keepalive.elapsed().as_secs() >= keepalive_secs {
                        last_keepalive = tokio::time::Instant::now();
                        if let Err(e) = client.fapi_user_data_stream_keepalive(&listen_key).await {
                            warn!(%user_id, err=%e, "binance futures user stream: keepalive failed");
                            break;
                        }
                    }
                }
            }
        }

        let _ = client.fapi_user_data_stream_close(&listen_key).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn spot_user_stream_for_user(pool: PgPool, user_id: Uuid) {
    let accounts = ExchangeAccountRepository::new(pool.clone());
    let orders = ExchangeOrderRepository::new(pool.clone());
    let fills = ExchangeFillRepository::new(pool.clone());
    let outbox = NotifyOutboxRepository::new(pool.clone());

    let creds = match accounts.binance_for_user(user_id, "spot").await {
        Ok(Some(c)) => c,
        Ok(None) => return,
        Err(e) => {
            warn!(%e, %user_id, "binance spot user stream: credentials lookup failed");
            return;
        }
    };

    let cfg = BinanceClientConfig::mainnet_with_keys(creds.api_key, creds.api_secret);
    let client = match BinanceClient::new(cfg) {
        Ok(c) => c,
        Err(e) => {
            warn!(%user_id, err = %e, "binance spot user stream: client init failed");
            return;
        }
    };

    let keepalive_secs = user_stream_keepalive_secs();
    loop {
        let listen_key = match client.spot_user_data_stream_start().await {
            Ok(v) => v.get("listenKey").and_then(Value::as_str).map(str::to_string),
            Err(e) => {
                warn!(%user_id, err = %e, "binance spot user stream: listenKey start failed");
                None
            }
        };

        let Some(listen_key) = listen_key else {
            tokio::time::sleep(Duration::from_secs(10)).await;
            continue;
        };

        let url = spot_user_data_stream_url(&listen_key);
        info!(%user_id, %url, "binance spot user stream connecting");

        let mut ws = match connect_url(&url).await {
            Ok(ws) => ws,
            Err(e) => {
                warn!(%user_id, err = %e, "binance spot user stream: ws connect failed");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let mut last_keepalive = tokio::time::Instant::now();
        loop {
            tokio::select! {
                msg = ws.next() => {
                    let Some(msg) = msg else {
                        warn!(%user_id, "binance spot user stream: ws closed");
                        break;
                    };
                    let Ok(msg) = msg else {
                        warn!(%user_id, "binance spot user stream: ws read error");
                        break;
                    };

                    let text = match msg {
                        tokio_tungstenite::tungstenite::protocol::Message::Text(t) => t,
                        tokio_tungstenite::tungstenite::protocol::Message::Binary(b) => String::from_utf8_lossy(&b).to_string(),
                        tokio_tungstenite::tungstenite::protocol::Message::Close(_) => break,
                        _ => continue,
                    };

                    let Ok(v) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    if !is_spot_execution_report(&v) {
                        continue;
                    }
                    let Some((venue_order_id, status)) = extract_spot_execution_report_fields(&v) else {
                        continue;
                    };
                    let normalized = normalize_binance_order_status(status);

                    // Spot executionReport: insert fill if last executed qty is present.
                    let last_qty = v.get("l").and_then(parse_decimal_str);
                    let last_price = v.get("L").and_then(parse_decimal_str);
                    if let (Some(q), Some(p)) = (last_qty, last_price) {
                        if q > Decimal::ZERO && p > Decimal::ZERO {
                            let fee = v.get("n").and_then(parse_decimal_str);
                            let fee_asset = v.get("N").and_then(Value::as_str);
                            let symbol = v.get("s").and_then(Value::as_str).unwrap_or("");
                            let trade_id = v.get("t").and_then(Value::as_i64);
                            let event_ms = v.get("T").and_then(Value::as_i64);
                            let event_time = event_ms.and_then(|ms| chrono::Utc.timestamp_millis_opt(ms).single());
                            if let Ok(Some(org_id)) = orders
                                .fetch_org_id_for_venue_order(user_id, "binance", "spot", venue_order_id)
                                .await
                            {
                                let inserted = fills
                                    .insert_if_absent(
                                        org_id,
                                        user_id,
                                        "binance",
                                        "spot",
                                        symbol,
                                        venue_order_id,
                                        trade_id,
                                        Some(p),
                                        Some(q),
                                        fee,
                                        fee_asset,
                                        event_time,
                                        Some(v.clone()),
                                    )
                                    .await;
                                if notify_enabled() {
                                    if let Ok(Some(_row)) = inserted {
                                        let ch = notify_channels();
                                        if !ch.is_empty() {
                                            let title = format!("Fill (Binance spot) {symbol}");
                                            let body = format!(
                                                "order_id={venue_order_id} qty={q} price={p} fee={} {}",
                                                fee.map(|x| x.to_string()).unwrap_or_else(|| "-".into()),
                                                fee_asset.unwrap_or("-")
                                            );
                                            let _ = outbox
                                                .enqueue_with_meta(
                                                    Some(org_id),
                                                    Some("binance.user_stream.fill"),
                                                    "info",
                                                    Some("binance"),
                                                    Some("spot"),
                                                    Some(symbol),
                                                    &title,
                                                    &body,
                                                    ch,
                                                )
                                                .await;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    match orders.update_status_and_venue_response_if_not_terminal(
                        user_id,
                        "binance",
                        "spot",
                        venue_order_id,
                        normalized,
                        &v,
                    ).await {
                        Ok(n) => {
                            if n > 0 {
                                info!(%user_id, venue_order_id, raw_status=%status, status=%normalized, rows=n, "exchange_orders updated from ws");
                                if notify_enabled() && is_terminal_status(normalized) {
                                    if let Ok(Some(org_id)) = orders
                                        .fetch_org_id_for_venue_order(user_id, "binance", "spot", venue_order_id)
                                        .await
                                    {
                                        let symbol = orders
                                            .fetch_symbol_for_venue_order(user_id, "binance", "spot", venue_order_id)
                                            .await
                                            .ok()
                                            .flatten();
                                        let ch = notify_channels();
                                        if !ch.is_empty() {
                                            let title = format!("Order {normalized} (Binance spot)");
                                            let body = format!("order_id={venue_order_id} raw_status={status}");
                                            let _ = outbox
                                                .enqueue_with_meta(
                                                    Some(org_id),
                                                    Some("binance.user_stream.order_terminal"),
                                                    severity_for_terminal_status(normalized),
                                                    Some("binance"),
                                                    Some("spot"),
                                                    symbol.as_deref(),
                                                    &title,
                                                    &body,
                                                    ch,
                                                )
                                                .await;
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => warn!(%user_id, venue_order_id, err=%e, "exchange_orders ws update failed"),
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    if last_keepalive.elapsed().as_secs() >= keepalive_secs {
                        last_keepalive = tokio::time::Instant::now();
                        if let Err(e) = client.spot_user_data_stream_keepalive(&listen_key).await {
                            warn!(%user_id, err=%e, "binance spot user stream: keepalive failed");
                            break;
                        }
                    }
                }
            }
        }

        let _ = client.spot_user_data_stream_close(&listen_key).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

pub async fn spawn_binance_user_stream_tasks(pool: &PgPool) {
    if !user_stream_enabled() {
        return;
    }
    let accounts = ExchangeAccountRepository::new(pool.clone());
    let futures_user_ids = match accounts.list_user_ids_binance_segment("futures").await {
        Ok(u) => u,
        Err(e) => {
            warn!(%e, "binance user stream: list users failed");
            return;
        }
    };
    let spot_user_ids = match accounts.list_user_ids_binance_segment("spot").await {
        Ok(u) => u,
        Err(e) => {
            warn!(%e, "binance user stream: list users (spot) failed");
            return;
        }
    };

    if futures_user_ids.is_empty() && spot_user_ids.is_empty() {
        info!("binance user stream enabled, but no accounts found");
        return;
    }

    if !futures_user_ids.is_empty() {
        info!(count = futures_user_ids.len(), "spawning binance futures user stream tasks");
        for user_id in futures_user_ids {
            let p = pool.clone();
            tokio::spawn(async move {
                futures_user_stream_for_user(p, user_id).await;
            });
        }
    }

    if !spot_user_ids.is_empty() {
        info!(count = spot_user_ids.len(), "spawning binance spot user stream tasks");
        for user_id in spot_user_ids {
            let p = pool.clone();
            tokio::spawn(async move {
                spot_user_stream_for_user(p, user_id).await;
            });
        }
    }
}


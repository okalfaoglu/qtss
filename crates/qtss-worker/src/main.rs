//! Arka plan işleri: rollup, mutabakat; isteğe bağlı kline WebSocket → `market_bars`.

use std::str::FromStr;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use futures_util::{SinkExt, StreamExt};
use qtss_binance::{
    connect_url, parse_closed_kline_json, public_spot_kline_url, public_usdm_kline_url,
};
use qtss_common::{init_logging, load_dotenv};
use qtss_storage::{create_pool, run_migrations, upsert_market_bar, MarketBarUpsert};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv();
    init_logging("info,qtss_worker=debug");

    if let Ok(sym) = std::env::var("QTSS_KLINE_SYMBOL") {
        let sym = sym.trim().to_string();
        if !sym.is_empty() {
            let interval =
                std::env::var("QTSS_KLINE_INTERVAL").unwrap_or_else(|_| "1m".into());
            let segment =
                std::env::var("QTSS_KLINE_SEGMENT").unwrap_or_else(|_| "spot".into());
            info!(%sym, %interval, %segment, "kline WebSocket görevi başlatılıyor");

            match std::env::var("DATABASE_URL") {
                Ok(db_url) if !db_url.trim().is_empty() => {
                    let pool = create_pool(&db_url, 3).await?;
                    run_migrations(&pool).await?;
                    tokio::spawn(kline_ws_loop(sym, interval, segment, Some(pool)));
                }
                _ => {
                    warn!("DATABASE_URL yok — kline yalnızca log (market_bars yazılmaz)");
                    tokio::spawn(kline_ws_loop(sym, interval, segment, None));
                }
            }
        }
    } else {
        warn!(
            "QTSS_KLINE_SYMBOL tanımsız — kline WebSocket kapalı. \
             systemd: `EnvironmentFile=` ile yüklenen dosyada satır yorumlu (#) olmamalı; \
             veya `[Service]` altında `Environment=QTSS_KLINE_SYMBOL=BTCUSDT` ekleyip \
             `systemctl daemon-reload && systemctl restart qtss-worker` çalıştırın."
        );
    }

    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
        info!("worker heartbeat");
    }
}

fn segment_ws_db(segment: &str) -> &'static str {
    match segment {
        "futures" | "usdt_futures" | "fapi" => "futures",
        _ => "spot",
    }
}

fn kline_url(symbol: &str, interval: &str, segment: &str) -> String {
    match segment {
        "futures" | "usdt_futures" | "fapi" => {
            public_usdm_kline_url(symbol, interval)
        }
        _ => public_spot_kline_url(symbol, interval),
    }
}

fn decimal_field(s: &str, field: &'static str) -> Option<Decimal> {
    match Decimal::from_str(s.trim()) {
        Ok(d) => Some(d),
        Err(e) => {
            warn!(%e, %field, "geçersiz decimal");
            None
        }
    }
}

async fn kline_ws_loop(symbol: String, interval: String, segment: String, pool: Option<PgPool>) {
    let url = kline_url(&symbol, &interval, segment.as_str());
    let exchange = "binance";
    let seg_db = segment_ws_db(segment.as_str());

    loop {
        match connect_url(&url).await {
            Ok(mut ws) => {
                info!(%url, "WebSocket bağlandı");
                while let Some(msg) = ws.next().await {
                    match msg {
                        Ok(Message::Text(t)) => {
                            if let Some(pool) = pool.as_ref() {
                                if let Some(k) = parse_closed_kline_json(&t) {
                                    let Some(ot) =
                                        Utc.timestamp_millis_opt(k.open_time_ms).single()
                                    else {
                                        warn!(open_time_ms = k.open_time_ms, "open_time");
                                        continue;
                                    };
                                    let Some(open) = decimal_field(&k.open, "open") else {
                                        continue;
                                    };
                                    let Some(high) = decimal_field(&k.high, "high") else {
                                        continue;
                                    };
                                    let Some(low) = decimal_field(&k.low, "low") else {
                                        continue;
                                    };
                                    let Some(close) = decimal_field(&k.close, "close") else {
                                        continue;
                                    };
                                    let Some(volume) = decimal_field(&k.volume, "volume") else {
                                        continue;
                                    };
                                    let quote_volume = k
                                        .quote_volume
                                        .as_deref()
                                        .and_then(|q| decimal_field(q, "quote_volume"));
                                    let trade_count = k.trade_count.map(|n| n as i64);
                                    let row = MarketBarUpsert {
                                        exchange: exchange.to_string(),
                                        segment: seg_db.to_string(),
                                        symbol: k.symbol.clone(),
                                        interval: k.interval.clone(),
                                        open_time: ot,
                                        open,
                                        high,
                                        low,
                                        close,
                                        volume,
                                        quote_volume,
                                        trade_count,
                                    };
                                    if let Err(e) = upsert_market_bar(pool, &row).await {
                                        warn!(%e, symbol = %row.symbol, "market_bars upsert");
                                    } else {
                                        info!(symbol = %row.symbol, interval = %row.interval, "mum yazıldı");
                                    }
                                }
                            } else if t.len() > 200 {
                                tracing::debug!(len = t.len(), "kline frame");
                            } else {
                                tracing::debug!(%t, "kline");
                            }
                        }
                        Ok(Message::Ping(p)) => {
                            let _ = ws.send(Message::Pong(p)).await;
                        }
                        Ok(Message::Close(_)) => break,
                        Err(e) => {
                            warn!(%e, "ws okuma hatası");
                            break;
                        }
                        _ => {}
                    }
                }
                warn!("WebSocket kapandı, 5 sn sonra yeniden bağlanılacak");
            }
            Err(e) => {
                warn!(%e, "WebSocket bağlantı hatası");
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

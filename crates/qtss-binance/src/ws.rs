//! Halka açık WebSocket uçları — mum (kline) akışı bağlantısı.

use tokio_tungstenite::connect_async;

use crate::error::BinanceError;

pub type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

/// Spot user data stream (listen key): `wss://stream.binance.com:9443/ws/{listen_key}`
#[must_use]
pub fn spot_user_data_stream_url(listen_key: &str) -> String {
    let k = listen_key.trim();
    format!("wss://stream.binance.com:9443/ws/{k}")
}

/// USDT-M futures user data stream (listen key): `wss://fstream.binance.com/ws/{listen_key}`
#[must_use]
pub fn usdm_user_data_stream_url(listen_key: &str) -> String {
    let k = listen_key.trim();
    format!("wss://fstream.binance.com/ws/{k}")
}

/// Spot: `wss://stream.binance.com:9443/ws/{symbol_lower}@kline_{interval}`
pub fn public_spot_kline_url(symbol: &str, interval: &str) -> String {
    let s = symbol
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>();
    format!("wss://stream.binance.com:9443/ws/{s}@kline_{interval}")
}

/// USDT-M: `wss://fstream.binance.com/ws/{symbol_lower}@kline_{interval}`
pub fn public_usdm_kline_url(symbol: &str, interval: &str) -> String {
    let s = symbol
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>();
    format!("wss://fstream.binance.com/ws/{s}@kline_{interval}")
}

fn sanitize_stream_symbol(symbol: &str) -> String {
    symbol
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// Spot combined stream: `wss://stream.binance.com:9443/stream?streams=btcusdt@kline_1m/ethusdt@kline_1m`
#[must_use]
pub fn public_spot_combined_kline_url(symbols: &[String], interval: &str) -> String {
    let streams: Vec<String> = symbols
        .iter()
        .map(|s| format!("{}@kline_{interval}", sanitize_stream_symbol(s)))
        .collect();
    format!(
        "wss://stream.binance.com:9443/stream?streams={}",
        streams.join("/")
    )
}

/// USDT-M combined stream: `wss://fstream.binance.com/stream?streams=...`
#[must_use]
pub fn public_usdm_combined_kline_url(symbols: &[String], interval: &str) -> String {
    let streams: Vec<String> = symbols
        .iter()
        .map(|s| format!("{}@kline_{interval}", sanitize_stream_symbol(s)))
        .collect();
    format!(
        "wss://fstream.binance.com/stream?streams={}",
        streams.join("/")
    )
}

/// Tam URL ile TLS WebSocket açar (işçi / test için).
#[cfg(test)]
mod combined_url_tests {
    use super::{public_spot_combined_kline_url, public_usdm_combined_kline_url};

    #[test]
    fn spot_combined_streams_joined() {
        let u = public_spot_combined_kline_url(
            &["BTCUSDT".into(), "ETHUSDT".into()],
            "1m",
        );
        assert!(u.contains("btcusdt@kline_1m"));
        assert!(u.contains("ethusdt@kline_1m"));
        assert!(u.contains("/stream?streams="));
    }

    #[test]
    fn usdm_combined_streams_joined() {
        let u = public_usdm_combined_kline_url(&["BTCUSDT".into()], "5m");
        assert!(u.contains("btcusdt@kline_5m"));
    }
}

pub async fn connect_url(url: &str) -> Result<WsStream, BinanceError> {
    let (ws, _) = connect_async(url)
        .await
        .map_err(|e| BinanceError::Other(format!("ws bağlantı: {e}")))?;
    Ok(ws)
}

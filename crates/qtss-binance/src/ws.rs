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

/// Single multiplex path, e.g. `btcusdt@kline_15m` (used to mix intervals in one combined URL).
#[must_use]
pub fn kline_stream_path(symbol: &str, interval: &str) -> String {
    format!(
        "{}@kline_{}",
        sanitize_stream_symbol(symbol),
        interval.trim()
    )
}

/// Spot combined stream: `wss://stream.binance.com:9443/stream?streams=btcusdt@kline_1m/ethusdt@kline_1m`
#[must_use]
pub fn public_spot_combined_kline_url(symbols: &[String], interval: &str) -> String {
    let streams: Vec<String> = symbols
        .iter()
        .map(|s| format!("{}@kline_{interval}", sanitize_stream_symbol(s)))
        .collect();
    public_spot_combined_streams_url(&streams)
}

/// Spot combined stream with arbitrary `symbol@kline_iv` paths (mixed intervals per symbol).
#[must_use]
pub fn public_spot_combined_streams_url(stream_paths: &[String]) -> String {
    format!(
        "wss://stream.binance.com:9443/stream?streams={}",
        stream_paths.join("/")
    )
}

/// USDT-M combined stream: `wss://fstream.binance.com/stream?streams=...`
#[must_use]
pub fn public_usdm_combined_kline_url(symbols: &[String], interval: &str) -> String {
    let streams: Vec<String> = symbols
        .iter()
        .map(|s| format!("{}@kline_{interval}", sanitize_stream_symbol(s)))
        .collect();
    public_usdm_combined_streams_url(&streams)
}

/// USDT-M combined stream with arbitrary stream paths.
#[must_use]
pub fn public_usdm_combined_streams_url(stream_paths: &[String]) -> String {
    format!(
        "wss://fstream.binance.com/stream?streams={}",
        stream_paths.join("/")
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

    #[test]
    fn mixed_interval_paths_in_one_url() {
        let paths = vec![
            kline_stream_path("BTCUSDT", "15m"),
            kline_stream_path("ETHUSDT", "4h"),
        ];
        let u = public_usdm_combined_streams_url(&paths);
        assert!(u.contains("btcusdt@kline_15m"));
        assert!(u.contains("ethusdt@kline_4h"));
    }
}

pub async fn connect_url(url: &str) -> Result<WsStream, BinanceError> {
    let (ws, _) = connect_async(url)
        .await
        .map_err(|e| BinanceError::Other(format!("ws bağlantı: {e}")))?;
    Ok(ws)
}

//! Halka açık WebSocket uçları — mum (kline) akışı bağlantısı.

use tokio_tungstenite::connect_async;

use crate::error::BinanceError;

pub type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

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

/// Tam URL ile TLS WebSocket açar (işçi / test için).
pub async fn connect_url(url: &str) -> Result<WsStream, BinanceError> {
    let (ws, _) = connect_async(url)
        .await
        .map_err(|e| BinanceError::Other(format!("ws bağlantı: {e}")))?;
    Ok(ws)
}

//! Binance kline (OHLCV) JSON yanıtını yapılandırılmış türe çevirir.

use serde::Serialize;
use serde_json::Value;

use crate::error::BinanceError;

/// Spot ve USDT-M kline satırı (Binance REST ile uyumlu alanlar).
#[derive(Debug, Clone, Serialize)]
pub struct KlineBar {
    pub open_time: u64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
    pub close_time: u64,
    pub quote_asset_volume: String,
    pub number_of_trades: u64,
}

fn parse_u64(v: &Value) -> Result<u64, BinanceError> {
    match v {
        Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| BinanceError::Other("kline: u64 bekleniyor".into())),
        Value::String(s) => s
            .parse()
            .map_err(|_| BinanceError::Other("kline: sayı parse".into())),
        _ => Err(BinanceError::Other("kline: beklenmeyen tür".into())),
    }
}

fn parse_str(v: &Value) -> Result<String, BinanceError> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        _ => Err(BinanceError::Other("kline: string bekleniyor".into())),
    }
}

/// Binance `/api/v3/klines` veya `/fapi/v1/klines` ham JSON dizisini ayrıştırır.
pub fn parse_klines_json(value: &Value) -> Result<Vec<KlineBar>, BinanceError> {
    let arr = value
        .as_array()
        .ok_or_else(|| BinanceError::Other("klines: dizi değil".into()))?;
    let mut out = Vec::with_capacity(arr.len());
    for row in arr {
        let r = row
            .as_array()
            .ok_or_else(|| BinanceError::Other("klines: satır dizi değil".into()))?;
        if r.len() < 9 {
            return Err(BinanceError::Other("klines: satır çok kısa".into()));
        }
        out.push(KlineBar {
            open_time: parse_u64(&r[0])?,
            open: parse_str(&r[1])?,
            high: parse_str(&r[2])?,
            low: parse_str(&r[3])?,
            close: parse_str(&r[4])?,
            volume: parse_str(&r[5])?,
            close_time: parse_u64(&r[6])?,
            quote_asset_volume: parse_str(&r[7])?,
            number_of_trades: parse_u64(&r[8])?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_two_klines() {
        let v = json!([
            [
                1_000_000_000u64,
                "1.0",
                "2.0",
                "0.5",
                "1.5",
                "100",
                1_000_000_999u64,
                "150",
                10u64,
                "0",
                "0",
                "0"
            ],
            [
                1_000_000_001u64,
                "1.1",
                "2.1",
                "0.6",
                "1.6",
                "200",
                1_000_001_000u64,
                "320",
                20u64,
                "0",
                "0",
                "0"
            ]
        ]);
        let bars = parse_klines_json(&v).unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].open_time, 1_000_000_000);
        assert_eq!(bars[0].close, "1.5");
        assert_eq!(bars[1].number_of_trades, 20);
    }
}

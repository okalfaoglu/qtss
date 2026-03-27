//! Binance REST emir yanıtlarından alan çıkarma.

use serde_json::Value;

/// Spot / FAPI `POST .../order` gövdesindeki `orderId` (sayı veya string olabilir).
pub fn venue_order_id_from_binance_order_response(v: &Value) -> Option<i64> {
    v.get("orderId")
        .and_then(|x| x.as_i64())
        .or_else(|| v.get("orderId").and_then(|x| x.as_u64()).map(|u| u as i64))
        .or_else(|| {
            v.get("orderId")
                .and_then(|x| x.as_str())
                .and_then(|s| s.parse().ok())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn order_id_i64() {
        let v = json!({ "orderId": 12345, "symbol": "BTCUSDT" });
        assert_eq!(venue_order_id_from_binance_order_response(&v), Some(12345));
    }

    #[test]
    fn order_id_string() {
        let v = json!({ "orderId": "987" });
        assert_eq!(venue_order_id_from_binance_order_response(&v), Some(987));
    }
}

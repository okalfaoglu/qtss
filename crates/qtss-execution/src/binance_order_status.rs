//! Binance spot / FAPI `GET .../order` yanıtındaki `status` → `exchange_orders.status`.

use serde_json::Value;

/// Binance `status` alanını yerel duruma çevirir; çözülemeyen veya hâlâ açık sayılanlar `None`.
pub fn exchange_order_status_from_binance_json(v: &Value) -> Option<String> {
    let s = v.get("status")?.as_str()?.trim();
    let u = s.to_ascii_uppercase();
    match u.as_str() {
        "FILLED" => Some("filled".into()),
        "PARTIALLY_FILLED" => Some("partially_filled".into()),
        "CANCELED" | "CANCELLED" => Some("canceled".into()),
        "REJECTED" => Some("canceled".into()),
        "EXPIRED" | "EXPIRED_IN_MATCH" => Some("canceled".into()),
        "NEW" | "PENDING_CANCEL" => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_filled() {
        let v = json!({"status": "FILLED"});
        assert_eq!(
            exchange_order_status_from_binance_json(&v).as_deref(),
            Some("filled")
        );
    }

    #[test]
    fn maps_canceled_case_insensitive() {
        let v = json!({"status": "canceled"});
        assert_eq!(
            exchange_order_status_from_binance_json(&v).as_deref(),
            Some("canceled")
        );
    }

    #[test]
    fn new_is_none() {
        let v = json!({"status": "NEW"});
        assert!(exchange_order_status_from_binance_json(&v).is_none());
    }
}

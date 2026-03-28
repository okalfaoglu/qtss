//! Komisyon oranları — `exchangeInfo` ipuçları + imzalı `tradeFee` / `commissionRate` ayrıştırma.

use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use std::str::FromStr;

fn json_f64(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// Oran (0–1, örn. `0.001` ≈ 10 bps) veya doğrudan bps benzeri sayı.
fn ratio_to_bps(x: f64) -> f64 {
    if x > 0.0 && x <= 1.0 {
        x * 10_000.0
    } else {
        x
    }
}

/// Maker / taker komisyonu (baz puan = 0.01%).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CommissionBps {
    pub maker_bps: f64,
    pub taker_bps: f64,
}

/// Tier 0 tipik spot değerleri (yaklaşık); gerçek ücret `account` / `exchangeInfo` ile doğrulanmalı.
pub fn default_spot_commission_bps() -> CommissionBps {
    CommissionBps {
        maker_bps: 10.0,
        taker_bps: 10.0,
    }
}

/// USDT-M futures tipik başlangıç değerleri (yaklaşık).
pub fn default_usdt_futures_commission_bps() -> CommissionBps {
    CommissionBps {
        maker_bps: 2.0,
        taker_bps: 4.0,
    }
}

/// İleride: `exchangeInfo` içindeki `filters` ve sembol bazlı indirimler.
pub fn resolve_from_exchange_info_stub() -> Option<CommissionBps> {
    None
}

/// Binance JSON ücret alanı — string veya sayı (kesir oran, örn. `0.00075`).
#[must_use]
pub fn decimal_from_binance_fee_field(v: &Value) -> Option<Decimal> {
    match v {
        Value::String(s) => Decimal::from_str(s.trim()).ok(),
        Value::Number(n) => Decimal::from_str(&n.to_string()).ok(),
        _ => None,
    }
}

/// `GET /sapi/v1/asset/tradeFee` gövdesi (dizi); `want_symbol` ile satır seçilir.
#[must_use]
pub fn trade_fee_from_sapi_response(value: &Value, want_symbol: &str) -> Option<(Decimal, Decimal)> {
    let arr = value.as_array()?;
    let want = want_symbol.trim().to_uppercase();
    for row in arr {
        let sym = row.get("symbol")?.as_str()?;
        if sym.to_uppercase() != want {
            continue;
        }
        let m = decimal_from_binance_fee_field(row.get("makerCommission")?)?;
        let t = decimal_from_binance_fee_field(row.get("takerCommission")?)?;
        return Some((m, t));
    }
    None
}

/// `GET /fapi/v1/commissionRate` tek nesne yanıtı.
#[must_use]
pub fn commission_rate_from_fapi_response(value: &Value) -> Option<(Decimal, Decimal)> {
    let m = decimal_from_binance_fee_field(value.get("makerCommissionRate")?)?;
    let t = decimal_from_binance_fee_field(value.get("takerCommissionRate")?)?;
    Some((m, t))
}

/// Spot `GET /api/v3/exchangeInfo` gövdesinden sembol satırı: varsa ham oran alanları (broker / özel uçlar).
/// Standart halka açık `exchangeInfo` çoğu sembolde komisyon içermez → `None` döner; gerçek ücret için imzalı uçlar gerekir.
pub fn spot_commission_hint_from_exchange_info(
    value: &Value,
    symbol_upper: &str,
) -> Option<CommissionBps> {
    let symbols = value.get("symbols")?.as_array()?;
    for sym in symbols {
        let sym_name = sym.get("symbol")?.as_str()?;
        if !sym_name.eq_ignore_ascii_case(symbol_upper) {
            continue;
        }
        if let (Some(m), Some(t)) = (
            sym.get("makerCommission").and_then(json_f64),
            sym.get("takerCommission").and_then(json_f64),
        ) {
            return Some(CommissionBps {
                maker_bps: ratio_to_bps(m),
                taker_bps: ratio_to_bps(t),
            });
        }
        return None;
    }
    None
}

/// FAPI `GET /fapi/v1/exchangeInfo` — varsa sembol düzeyi ücret ipuçları (yapı sürüme göre değişir).
pub fn futures_commission_hint_from_exchange_info(
    value: &Value,
    symbol_upper: &str,
) -> Option<CommissionBps> {
    let symbols = value.get("symbols")?.as_array()?;
    for sym in symbols {
        let sym_name = sym.get("symbol")?.as_str()?;
        if !sym_name.eq_ignore_ascii_case(symbol_upper) {
            continue;
        }
        if let (Some(m), Some(t)) = (
            sym.get("makerCommissionRate").and_then(|v| v.as_str()),
            sym.get("takerCommissionRate").and_then(|v| v.as_str()),
        ) {
            let m = m.parse::<f64>().ok()?;
            let t = t.parse::<f64>().ok()?;
            return Some(CommissionBps {
                maker_bps: ratio_to_bps(m),
                taker_bps: ratio_to_bps(t),
            });
        }
        return None;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn spot_hint_ratio_to_bps() {
        let body = json!({
            "symbols": [{
                "symbol": "BTCUSDT",
                "makerCommission": 0.00075,
                "takerCommission": 0.00075
            }]
        });
        let c = spot_commission_hint_from_exchange_info(&body, "BTCUSDT").expect("hint");
        assert!((c.maker_bps - 7.5).abs() < 1e-9);
        assert!((c.taker_bps - 7.5).abs() < 1e-9);
    }

    #[test]
    fn spot_hint_string_fields() {
        let body = json!({
            "symbols": [{
                "symbol": "ETHUSDT",
                "makerCommission": "12",
                "takerCommission": "15"
            }]
        });
        let c = spot_commission_hint_from_exchange_info(&body, "ethusdt").expect("hint");
        assert!((c.maker_bps - 12.0).abs() < 1e-9);
        assert!((c.taker_bps - 15.0).abs() < 1e-9);
    }

    #[test]
    fn futures_hint_from_rates() {
        let body = json!({
            "symbols": [{
                "symbol": "BTCUSDT",
                "makerCommissionRate": "0.0002",
                "takerCommissionRate": "0.0004"
            }]
        });
        let c =
            futures_commission_hint_from_exchange_info(&body, "BTCUSDT").expect("hint");
        assert!((c.maker_bps - 2.0).abs() < 1e-9);
        assert!((c.taker_bps - 4.0).abs() < 1e-9);
    }

    #[test]
    fn sapi_trade_fee_parse() {
        let body = json!([{
            "symbol": "BTCUSDT",
            "makerCommission": "0.00075",
            "takerCommission": "0.00075"
        }]);
        let (m, t) = trade_fee_from_sapi_response(&body, "btcusdt").expect("parse");
        assert_eq!(m, Decimal::new(75, 5));
        assert_eq!(t, Decimal::new(75, 5));
    }

    #[test]
    fn fapi_commission_rate_parse() {
        let body = json!({
            "symbol": "ETHUSDT",
            "makerCommissionRate": "0.0002",
            "takerCommissionRate": "0.0004"
        });
        let (m, t) = commission_rate_from_fapi_response(&body).expect("parse");
        assert_eq!(m, Decimal::new(2, 4));
        assert_eq!(t, Decimal::new(4, 4));
    }
}

//! Borsa ile yerel emir mutabakatı.

use std::collections::HashSet;

use serde::Serialize;
use serde_json::Value;

use crate::gateway::ExecutionError;

/// Yereldeki emrin borsa tarafı kimliği (API’den gelen satırlar için).
#[derive(Debug, Clone)]
pub struct ExchangeOrderVenueSnapshot {
    pub venue_order_id: i64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReconcileReport {
    pub venue: &'static str,
    /// Binance `/api/v3/openOrders` satır sayısı.
    pub checked_remote_orders: u64,
    /// Karşılaştırmaya dahil edilen yerel kayıt (binance + spot + venue_order_id dolu).
    pub checked_local_orders: u64,
    /// `local_submitted_not_open_on_venue` + `remote_open_unknown_locally`.
    pub mismatches: u64,
    /// Durumu hâlâ `submitted` görünen ama Binance açık listesinde olmayan emirler (dolmuş / iptal / API gecikmesi).
    pub local_submitted_not_open_on_venue: u64,
    /// Binance’ta açık görünen ama bizim `venue_order_id` setimizde olmayan emirler (manuel emir, başka uygulama vb.).
    pub remote_open_unknown_locally: u64,
    pub notes: String,
}

fn parse_spot_open_order_ids(remote: &Value) -> Result<Vec<i64>, ExecutionError> {
    let arr = remote
        .as_array()
        .ok_or_else(|| ExecutionError::Other("openOrders: JSON dizi değil".into()))?;
    let mut out = Vec::with_capacity(arr.len());
    for o in arr {
        let id = o
            .get("orderId")
            .and_then(|v| v.as_i64())
            .or_else(|| o.get("orderId").and_then(|v| v.as_u64()).map(|u| u as i64))
            .ok_or_else(|| ExecutionError::Other("openOrders: orderId eksik veya geçersiz".into()))?;
        out.push(id);
    }
    Ok(out)
}

/// `remote_open`: `spot_open_orders` yanıtı. `local`: yalnızca `exchange=binance`, `segment=spot` ve `venue_order_id` dolu satırların özeti.
pub fn reconcile_binance_spot_open_orders(
    remote_open: &Value,
    local: &[ExchangeOrderVenueSnapshot],
) -> Result<ReconcileReport, ExecutionError> {
    let remote_ids = parse_spot_open_order_ids(remote_open)?;
    let remote_set: HashSet<i64> = remote_ids.iter().copied().collect();
    let local_venue_set: HashSet<i64> = local.iter().map(|s| s.venue_order_id).collect();

    let mut local_submitted_not_open = 0_u64;
    for s in local {
        if s.status == "submitted" && !remote_set.contains(&s.venue_order_id) {
            local_submitted_not_open += 1;
        }
    }

    let mut remote_unknown = 0_u64;
    for id in &remote_set {
        if !local_venue_set.contains(id) {
            remote_unknown += 1;
        }
    }

    let mismatches = local_submitted_not_open + remote_unknown;
    let notes = if mismatches == 0 {
        "Spot açık emir listesi ile yerel venue_order_id eşlemesi tutarlı (submitted ve bilinmeyen uzaktan emir yok)."
            .into()
    } else {
        format!(
            "Dikkat: {local_submitted_not_open} yerel submitted borsada açık değil; {remote_unknown} borsa açık emri yerelde venue_order_id ile bulunamadı."
        )
    };

    Ok(ReconcileReport {
        venue: "binance_spot",
        checked_remote_orders: remote_set.len() as u64,
        checked_local_orders: local.len() as u64,
        mismatches,
        local_submitted_not_open_on_venue: local_submitted_not_open,
        remote_open_unknown_locally: remote_unknown,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_both_ok() {
        let r = reconcile_binance_spot_open_orders(&json!([]), &[]).unwrap();
        assert_eq!(r.checked_remote_orders, 0);
        assert_eq!(r.mismatches, 0);
    }

    #[test]
    fn submitted_missing_on_venue() {
        let remote = json!([{"orderId": 100}]);
        let local = vec![ExchangeOrderVenueSnapshot {
            venue_order_id: 200,
            status: "submitted".into(),
        }];
        let r = reconcile_binance_spot_open_orders(&remote, &local).unwrap();
        assert_eq!(r.local_submitted_not_open_on_venue, 1);
        assert_eq!(r.remote_open_unknown_locally, 1);
        assert_eq!(r.mismatches, 2);
    }
}

//! Onchain Pillar — Nansen/on-chain veri ile smart money analizi.
//! Faz B'de placeholder; Faz E'de nansen entegrasyonu ile doldurulacak.

use crate::pillar::{PillarKind, PillarScore};

/// On-chain veri metrikleri.
#[derive(Debug, Clone, Default)]
pub struct OnchainMetrics {
    /// Smart money net flow (pozitif = borsaya giriş = satış baskısı)
    pub smart_money_net_flow: Option<f64>,
    /// Exchange netflow (pozitif = borsaya giriş)
    pub exchange_netflow: Option<f64>,
    /// Whale transaction count (büyük işlem sayısı, son 24h)
    pub whale_tx_count: Option<u32>,
    /// Funding rate (negatif = short ağırlıklı)
    pub funding_rate: Option<f64>,
}

/// Onchain pillar skoru hesaplar.
/// Veri yoksa nötr (50) döner, düşük ağırlıkla.
#[must_use]
pub fn score_onchain(metrics: &OnchainMetrics, is_bottom_search: bool) -> PillarScore {
    let mut score = 0.0_f64;
    let mut details = Vec::new();
    let mut has_data = false;

    // 1) Smart money flow (max 30)
    if let Some(flow) = metrics.smart_money_net_flow {
        has_data = true;
        if is_bottom_search && flow < 0.0 {
            // Borsadan çıkış = akümülasyon
            score += 30.0;
            details.push(format!("Smart money outflow (accumulation) flow={flow:.0}"));
        } else if !is_bottom_search && flow > 0.0 {
            score += 30.0;
            details.push(format!("Smart money inflow (distribution) flow={flow:.0}"));
        }
    }

    // 2) Exchange netflow (max 25)
    if let Some(nf) = metrics.exchange_netflow {
        has_data = true;
        if is_bottom_search && nf < 0.0 {
            score += 25.0;
            details.push("Exchange outflow (bullish)".into());
        } else if !is_bottom_search && nf > 0.0 {
            score += 25.0;
            details.push("Exchange inflow (bearish)".into());
        }
    }

    // 3) Funding rate (max 25)
    if let Some(fr) = metrics.funding_rate {
        has_data = true;
        if is_bottom_search && fr < -0.01 {
            score += 25.0;
            details.push(format!("Negative funding {fr:.4} (over-shorted)"));
        } else if !is_bottom_search && fr > 0.01 {
            score += 25.0;
            details.push(format!("High funding {fr:.4} (over-leveraged longs)"));
        }
    }

    // 4) Whale activity (max 20)
    if let Some(wc) = metrics.whale_tx_count {
        has_data = true;
        if wc > 50 {
            score += 20.0;
            details.push(format!("High whale activity: {wc} txns"));
        } else if wc > 20 {
            score += 10.0;
            details.push(format!("Moderate whale activity: {wc} txns"));
        }
    }

    if !has_data {
        details.push("No on-chain data available".into());
    }

    PillarScore {
        kind: PillarKind::Onchain,
        score: score.min(100.0),
        weight: if has_data { 0.15 } else { 0.0 },
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_data_returns_zero_weight() {
        let s = score_onchain(&OnchainMetrics::default(), true);
        assert_eq!(s.weight, 0.0);
    }

    #[test]
    fn accumulation_signal() {
        let m = OnchainMetrics {
            smart_money_net_flow: Some(-500.0),
            exchange_netflow: Some(-1000.0),
            funding_rate: Some(-0.02),
            whale_tx_count: Some(60),
        };
        let s = score_onchain(&m, true);
        assert!(s.score >= 80.0);
        assert_eq!(s.weight, 0.15);
    }
}

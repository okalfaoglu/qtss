//! Paralel İngilizce anahtarlar — PLAN Phase F / `SignalDashboardV2` geçişi.
//! `SignalDashboardV1` ile aynı anlamlar; değerler Türkçe etiketlerden türetilir.

use serde::{Deserialize, Serialize};

use crate::SignalDashboardV1;

fn ascii_upper_tr(s: &str) -> String {
    s.trim().to_uppercase().replace('İ', "I")
}

fn map_trend(s: &str) -> String {
    match ascii_upper_tr(s).as_str() {
        "YUKARI" => "up".into(),
        "ASAGI" => "down".into(),
        "YATAY" => "flat".into(),
        "KAPALI" => "closed".into(),
        _ => "unknown".into(),
    }
}

fn map_market_mode(s: &str) -> String {
    match ascii_upper_tr(s).as_str() {
        "RANGE" => "range".into(),
        "KOPUS" => "breakout".into(),
        "TREND" => "trend".into(),
        "BELIRSIZ" => "uncertain".into(),
        _ => "uncertain".into(),
    }
}

fn map_entry_mode(s: &str) -> String {
    match ascii_upper_tr(s).as_str() {
        "DONUS" => "reversal".into(),
        "TAKIP" => "follow_through".into(),
        "—" | "-" | "" => "none".into(),
        _ => "other".into(),
    }
}

fn map_momentum(s: &str) -> String {
    match ascii_upper_tr(s).as_str() {
        "POZITIF" => "positive".into(),
        "NEGATIF" => "negative".into(),
        "NOTR" => "neutral".into(),
        _ => "unknown".into(),
    }
}

/// İngilizce `snake_case` alanlar; `schema_version` **3** (v1 gövdesindeki 2 ile karıştırılmamalı).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalDashboardV2Envelope {
    pub schema_version: i32,
    pub status: String,
    pub status_model_raw: String,
    pub local_trend: String,
    pub global_trend: String,
    pub market_mode: String,
    pub entry_mode: String,
    pub volatility_pct: f64,
    /// v1 `momentum_1` — aynı mantık; isim PLAN’daki `momentum_rsi` yuvasıyla uyumlu.
    pub momentum_rsi: String,
    /// v1 `momentum_2` — PLAN’daki `momentum_roc` yuvası.
    pub momentum_roc: String,
    pub entry_price: Option<f64>,
    pub stop_initial: Option<f64>,
    pub take_profit_initial: Option<f64>,
    pub stop_trail: Option<f64>,
    pub take_profit_dynamic: Option<f64>,
    pub signal_source: String,
    pub trend_exhaustion: bool,
    pub structure_shift: bool,
    pub position_strength_10: u8,
    pub system_active: bool,
}

#[must_use]
pub fn signal_dashboard_v2_envelope_from_v1(d: &SignalDashboardV1) -> SignalDashboardV2Envelope {
    SignalDashboardV2Envelope {
        schema_version: 3,
        status: d.durum.clone(),
        status_model_raw: d.durum_model_raw.clone(),
        local_trend: map_trend(&d.yerel_trend),
        global_trend: map_trend(&d.global_trend),
        market_mode: map_market_mode(&d.piyasa_modu),
        entry_mode: map_entry_mode(&d.giris_modu),
        volatility_pct: d.oynaklik_pct,
        momentum_rsi: map_momentum(&d.momentum_1),
        momentum_roc: map_momentum(&d.momentum_2),
        entry_price: d.giris_gercek,
        stop_initial: d.stop_ilk,
        take_profit_initial: d.kar_al_ilk,
        stop_trail: d.stop_trail_aktif,
        take_profit_dynamic: d.kar_al_dinamik,
        signal_source: d.sinyal_kaynagi.clone(),
        trend_exhaustion: d.trend_tukenmesi,
        structure_shift: d.yapi_kaymasi,
        position_strength_10: d.pozisyon_gucu_10,
        system_active: d.sistem_aktif,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading_range::{analyze_trading_range, TradingRangeParams};
    use crate::OhlcBar;
    use crate::compute_signal_dashboard_v1;

    fn bar(i: i64, c: f64) -> OhlcBar {
        OhlcBar {
            open: c,
            high: c + 0.5,
            low: c - 0.5,
            close: c,
            bar_index: i,
        }
    }

    #[test]
    fn v2_envelope_schema_and_maps_trends() {
        let v: Vec<OhlcBar> = (0..120_i64).map(|i| bar(i, 100.0 + (i as f64) * 0.01)).collect();
        let p = TradingRangeParams {
            lookback: 40,
            atr_period: 14,
            atr_sma_period: 20,
            require_range_regime: false,
        };
        let tr = analyze_trading_range(&v, &p);
        let d = compute_signal_dashboard_v1(&v, &tr);
        let e = signal_dashboard_v2_envelope_from_v1(&d);
        assert_eq!(e.schema_version, 3);
        assert!(!e.local_trend.is_empty());
        assert!(!e.market_mode.is_empty());
        assert_eq!(e.position_strength_10, d.pozisyon_gucu_10);
    }

    #[test]
    fn belirsiz_market_mode_maps() {
        let v: Vec<OhlcBar> = (0..120_i64).map(|i| bar(i, 100.0)).collect();
        let p = TradingRangeParams {
            lookback: 40,
            atr_period: 14,
            atr_sma_period: 20,
            require_range_regime: false,
        };
        let tr = analyze_trading_range(&v, &p);
        let mut d = compute_signal_dashboard_v1(&v, &tr);
        d.piyasa_modu = "BELİRSİZ".into();
        let e = signal_dashboard_v2_envelope_from_v1(&d);
        assert_eq!(e.market_mode, "uncertain");
    }
}

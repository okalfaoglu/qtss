//! CandleConfig — thresholds for single/2/3-bar candlestick classification.
//! Seeded from `system_config` via migration 0159 (CLAUDE.md #2).

use crate::error::{CandleError, CandleResult};
use serde::{Deserialize, Serialize};

/// How prior-trend context is determined for reversal patterns.
///
/// TV parity (tr.tradingview.com/support/folders/43000570503): TV'nin her
/// sayfasında 3 opsiyon açıklanır — `SMA50`, `SMA50 + SMA200`, "tespit
/// yok". Kümülatif yüzde getirisi (`Pct`) bizim legacy yaklaşımımız; TV
/// sayfalarında geçmez ama kısa bar pencerelerinde (test fixture'ları,
/// SMA bootstrap edilmemiş canlı sembol) fallback olarak kullanılır.
///
/// Dispatch tek noktadan yapılır (`has_prior_uptrend`/`downtrend`) —
/// CLAUDE.md #1 uyumu: her eval fonksiyonunda mod kontrolü yok.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendMode {
    /// Fiyat SMA50 üstünde → uptrend, altında → downtrend.
    Sma50,
    /// Fiyat SMA50 *ve* SMA50 SMA200 tarafında — sadece güçlü trendler.
    /// TV'nin "stronger trends" modu; false-positive az, varsayılan.
    Sma50And200,
    /// Legacy: son N bar üzerinden kümülatif yüzde getirisi eşiği.
    Pct,
    /// Trend guard devre dışı — reversal pattern'ler trend context
    /// olmadan tetiklenir. TV'de "algılama yok" seçeneği karşılığı.
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandleConfig {
    /// Doji: body / range <= this threshold counts as a doji body.
    pub doji_body_ratio_max: f64,
    /// Marubozu: (upper_shadow + lower_shadow) / range <= this.
    pub marubozu_shadow_ratio_max: f64,
    /// Hammer / hanging man: lower_shadow / body >= this, upper shadow small.
    pub hammer_lower_shadow_ratio_min: f64,
    /// Hammer: upper_shadow / body <= this (small upper shadow).
    pub hammer_upper_shadow_ratio_max: f64,
    /// Spinning top: body / range <= this and shadows relatively balanced.
    pub spinning_top_body_ratio_max: f64,
    /// Tweezer equality tolerance: |h1-h2|/mid or |l1-l2|/mid.
    pub tweezer_price_tol: f64,
    /// Prior-trend classifier. TV uyumu için default `Sma50And200`;
    /// yetersiz bar (<50) otomatik `Pct` fallback'e düşer.
    pub trend_mode: TrendMode,
    /// Trend context: number of prior bars to confirm "prior trend"
    /// required by reversal patterns (hanging_man, shooting_star, …).
    /// `Pct` modunda kullanılır; `Sma*` modlarında yok sayılır.
    pub trend_context_bars: usize,
    /// Minimum cumulative return over `trend_context_bars` for an
    /// established trend (|return| threshold). `Pct` mode only.
    pub trend_context_min_pct: f64,
    /// Minimum structural score for emission.
    pub min_structural_score: f32,
    /// Minimum timeframe (in seconds) on which candle detections are
    /// considered meaningful. Sub-threshold timeframes produce too much
    /// noise (e.g. morning_star on 1m is statistically indistinguishable
    /// from random). Default 900 (= 15m). DB-tunable via
    /// `detection.candle.min_timeframe_seconds`.
    pub min_timeframe_seconds: i64,
}

impl Default for CandleConfig {
    fn default() -> Self {
        Self {
            doji_body_ratio_max: 0.1,
            marubozu_shadow_ratio_max: 0.05,
            hammer_lower_shadow_ratio_min: 2.0,
            hammer_upper_shadow_ratio_max: 0.5,
            spinning_top_body_ratio_max: 0.3,
            tweezer_price_tol: 0.002,
            trend_mode: TrendMode::Sma50And200,
            trend_context_bars: 5,
            trend_context_min_pct: 0.015,
            min_structural_score: 0.5,
            min_timeframe_seconds: 900,
        }
    }
}

impl CandleConfig {
    pub fn validate(&self) -> CandleResult<()> {
        if !(0.0..=1.0).contains(&self.doji_body_ratio_max) {
            return Err(CandleError::InvalidConfig(
                "doji_body_ratio_max must be in [0,1]".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.marubozu_shadow_ratio_max) {
            return Err(CandleError::InvalidConfig(
                "marubozu_shadow_ratio_max must be in [0,1]".into(),
            ));
        }
        if self.hammer_lower_shadow_ratio_min <= 0.0 {
            return Err(CandleError::InvalidConfig(
                "hammer_lower_shadow_ratio_min must be > 0".into(),
            ));
        }
        if self.trend_context_bars < 2 {
            return Err(CandleError::InvalidConfig(
                "trend_context_bars must be >= 2".into(),
            ));
        }
        if self.min_timeframe_seconds < 0 {
            return Err(CandleError::InvalidConfig(
                "min_timeframe_seconds must be >= 0".into(),
            ));
        }
        Ok(())
    }
}

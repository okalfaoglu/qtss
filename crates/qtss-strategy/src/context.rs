use qtss_storage::{fetch_latest_onchain_signal_score, list_recent_bars};
use rust_decimal::Decimal;
use sqlx::PgPool;

/// Per-symbol view for strategy loops (`onchain_signal_scores` + last bar close).
#[derive(Debug, Clone)]
pub struct MarketContext {
    pub symbol: String,
    pub aggregate_score: f64,
    pub confidence: f64,
    pub direction: String,
    pub conflict_detected: bool,
    pub market_regime: Option<String>,
    pub funding_score: Option<f64>,
    pub nansen_perp_score: Option<f64>,
    pub nansen_netflow_score: Option<f64>,
    pub last_close: Option<Decimal>,
}

impl MarketContext {
    /// `bar_*` must match how `market_bars` rows are written (usually same as `engine_symbols`).
    pub async fn load(
        pool: &PgPool,
        symbol: &str,
        bar_exchange: &str,
        bar_segment: &str,
        bar_interval: &str,
    ) -> Option<Self> {
        let sym = symbol.trim().to_uppercase();
        let row = fetch_latest_onchain_signal_score(pool, &sym).await.ok()??;

        let last_close = list_recent_bars(pool, bar_exchange, bar_segment, &sym, bar_interval, 1)
            .await
            .ok()
            .and_then(|bars| bars.into_iter().next().map(|b| b.close));

        Some(Self {
            symbol: sym,
            aggregate_score: row.aggregate_score,
            confidence: row.confidence,
            direction: row.direction,
            conflict_detected: row.conflict_detected,
            market_regime: row.market_regime,
            funding_score: row.funding_score,
            nansen_perp_score: row.nansen_perp_score,
            nansen_netflow_score: row.nansen_netflow_score,
            last_close,
        })
    }
}

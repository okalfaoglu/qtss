//! SPEC_ONCHAIN_SIGNALS §7 — on-chain skor tablosu okuma uçları.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use qtss_storage::{
    fetch_data_snapshot, fetch_latest_onchain_signal_score, list_onchain_signal_scores_history,
    DataSnapshotRow, OnchainSignalScoreRow,
};

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;
use axum::Extension;

#[derive(Deserialize)]
pub struct OnchainSymbolQuery {
    pub symbol: String,
}

#[derive(Deserialize)]
pub struct OnchainHistoryQuery {
    pub symbol: String,
    #[serde(default = "default_history_limit")]
    pub limit: i64,
}

fn default_history_limit() -> i64 {
    100
}

/// Son birleşik skor + kolonlar (`onchain_signal_scores`).
async fn onchain_signals_latest(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<OnchainSymbolQuery>,
) -> Result<Json<Option<OnchainSignalScoreRow>>, ApiError> {
    let sym = q.symbol.trim();
    if sym.is_empty() {
        return Err(ApiError::bad_request("query symbol is required"));
    }
    let row = fetch_latest_onchain_signal_score(&st.pool, sym).await?;
    Ok(Json(row))
}

/// Skor geçmişi (yeni → eski).
async fn onchain_signals_history(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<OnchainHistoryQuery>,
) -> Result<Json<Vec<OnchainSignalScoreRow>>, ApiError> {
    let sym = q.symbol.trim();
    if sym.is_empty() {
        return Err(ApiError::bad_request("query symbol is required"));
    }
    let rows = list_onchain_signal_scores_history(&st.pool, sym, q.limit).await?;
    Ok(Json(rows))
}

/// Son satır + ilgili `data_snapshots` ham satırları (confluence ile aynı anahtar kümesi).
async fn onchain_signals_breakdown(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<OnchainSymbolQuery>,
) -> Result<Json<Value>, ApiError> {
    let sym = q.symbol.trim().to_uppercase();
    if sym.is_empty() {
        return Err(ApiError::bad_request("query symbol is required"));
    }
    let score = fetch_latest_onchain_signal_score(&st.pool, &sym).await?;
    let base = sym
        .strip_suffix("USDT")
        .unwrap_or(sym.as_str())
        .to_lowercase();
    let mut keys = vec![
        "nansen_token_screener".to_string(),
        format!("binance_taker_{base}usdt"),
        format!("binance_premium_{base}usdt"),
        format!("binance_open_interest_{base}usdt"),
        format!("binance_ls_ratio_{base}usdt"),
        "hl_meta_asset_ctxs".to_string(),
    ];
    if base == "btc" {
        keys.push("coinglass_netflow_btc".into());
        keys.push("coinglass_liquidations_btc".into());
        keys.push("coinglass_exchange_balance_btc".into());
    }
    let mut snapshots: Vec<DataSnapshotRow> = Vec::new();
    for k in &keys {
        if let Ok(Some(r)) = fetch_data_snapshot(&st.pool, k).await {
            snapshots.push(r);
        }
    }
    let onchain_breakdown = score.as_ref().and_then(|r| r.meta_json.as_ref()).map(|m| {
        json!({
            "schema_version": m.get("schema_version"),
            "weights_config_key": m.get("weights_config_key"),
            "weights_used": m.get("weights_used"),
            "source_breakdown": m.get("source_breakdown"),
            "aggregate_formula": m.get("aggregate_formula"),
            "per_key_confidence": m.get("per_key_confidence"),
        })
    });
    Ok(Json(json!({
        "symbol": sym,
        "latest_score_row": score,
        "onchain_breakdown": onchain_breakdown,
        "data_snapshots": snapshots,
    })))
}

pub fn onchain_signals_router() -> Router<SharedState> {
    Router::new()
        .route("/analysis/onchain-signals/latest", get(onchain_signals_latest))
        .route("/analysis/onchain-signals/history", get(onchain_signals_history))
        .route("/analysis/onchain-signals/breakdown", get(onchain_signals_breakdown))
}

//! Merkezi analiz — `qtss-chart-patterns` ile formasyon iskelesi.

use std::collections::BTreeMap;

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use qtss_chart_patterns::{
    analyze_channel_six_from_bars, channel_six_drawing_hints, channel_six_pattern_drawing_batch,
    check_breakout_volume, compute_apex_from_outcome, compute_formation_trade_levels,
    detect_failure_swing, formation_to_drawing_batch, pattern_name_by_acp_id,
    pivots_chronological, scan_formations, zigzag_from_ohlc_bars, ApexResult,
    BreakoutVolumeResult, ChannelSixDrawingHints, ChannelSixReject, ChannelSixScanOutcome,
    ChannelSixWindowFilter, FailureSwingResult, FormationMatch, FormationParams,
    FormationTradeLevels, OhlcBar, PatternDrawingBatch, SixPivotScanParams, SizeFilters,
};
use qtss_common::{log_business, QtssLogLevel};
use qtss_storage::{
    fetch_analysis_snapshot_payload, fetch_data_snapshot, fetch_intake_playbook_candidate_by_id,
    fetch_intake_playbook_run_by_id, fetch_latest_intake_playbook_run,
    fetch_latest_nansen_setup_with_rows, fetch_nansen_snapshot, fetch_range_engine_json,
    insert_engine_symbol, list_analysis_snapshots_with_symbols, list_data_snapshots,
    list_engine_symbols_all, list_engine_symbols_with_ingestion, list_engine_symbols_matching,
    list_intake_playbook_candidates_for_run, list_market_confluence_snapshots_for_symbol,
    list_market_context_summaries, list_range_signal_events_joined,
    list_recent_intake_playbook_runs, merge_json_deep, update_engine_symbol_enabled,
    update_engine_symbol_patch, update_intake_candidate_merged_engine_symbol,
    upsert_range_engine_json, AnalysisSnapshotJoinedRow, DataSnapshotRow, EngineSymbolInsert,
    EngineSymbolRow, IntakePlaybookCandidateRow, IntakePlaybookRunRow,
    MarketConfluenceSnapshotRow, MarketContextSummaryRow, NansenSetupRowDetail, NansenSetupRunRow,
    NansenSnapshotRow, EngineSymbolIngestionJoinedRow, RangeSignalEventJoinedRow,
};

use crate::error::ApiError;
use crate::locale::NegotiatedLocale;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

const ACP_CHART_PATTERNS_CONFIG_KEY: &str = "acp_chart_patterns";
const ELLIOTT_WAVE_CONFIG_KEY: &str = "elliott_wave";

fn map_analysis_storage_err(
    e: qtss_storage::StorageError,
    loc: &NegotiatedLocale,
    error_key: &'static str,
    message_en: &'static str,
) -> ApiError {
    tracing::warn!(target: "qtss_api", %error_key, error = %e, "analysis storage");
    ApiError::internal(message_en.to_string())
        .with_locale(loc.as_str().to_string())
        .with_error_key(error_key)
}

/// Salt okunur / dashboard rolleri (`viewer`+).
pub fn analysis_read_router() -> Router<SharedState> {
    Router::new()
        .route("/analysis/health", get(analysis_health))
        .route(
            "/analysis/chart-patterns-config",
            get(get_chart_patterns_config),
        )
        .route(
            "/analysis/elliott-wave-config",
            get(get_elliott_wave_config),
        )
        .route("/analysis/patterns/channel-six", post(channel_six_scan))
        .route("/analysis/engine/symbols", get(list_engine_symbols_api))
        .route("/analysis/engine/snapshots", get(list_engine_snapshots_api))
        .route(
            "/analysis/engine/ingestion-state",
            get(list_engine_symbol_ingestion_api),
        )
        .route(
            "/analysis/engine/confluence/latest",
            get(list_confluence_snapshots_api),
        )
        .route(
            "/analysis/confluence/latest",
            get(get_confluence_latest_by_symbol_api),
        )
        .route("/analysis/data-snapshots", get(list_data_snapshots_api))
        .route(
            "/analysis/market-context/latest",
            get(get_market_context_latest_api),
        )
        .route(
            "/analysis/market-context/summary",
            get(list_market_context_summary_api),
        )
        .route(
            "/analysis/market-confluence/history",
            get(list_market_confluence_history_api),
        )
        .route(
            "/analysis/engine/range-signals",
            get(list_range_signals_api),
        )
        .route("/analysis/range-engine/config", get(get_range_engine_config_api))
        .route("/analysis/nansen/snapshot", get(get_nansen_snapshot_api))
        .route(
            "/analysis/nansen/setups/latest",
            get(get_nansen_setups_latest_api),
        )
        .route(
            "/analysis/intake-playbook/latest",
            get(get_intake_playbook_latest_api),
        )
        .route(
            "/analysis/intake-playbook/recent",
            get(list_intake_playbook_recent_api),
        )
}

/// `engine_symbols` yazımı — `trader` / `admin` (`require_ops_roles`).
pub fn analysis_write_router() -> Router<SharedState> {
    Router::new()
        // Not under `symbols/bulk` — that path is captured by `symbols/{id}` (PATCH), yielding POST → 405.
        .route("/analysis/engine/symbols-bulk", post(post_engine_symbols_bulk_api))
        .route("/analysis/engine/symbols", post(post_engine_symbol_api))
        .route(
            "/analysis/engine/symbols/{id}",
            patch(patch_engine_symbol_api),
        )
        .route(
            "/analysis/range-engine/config",
            patch(patch_range_engine_config_api),
        )
        .route(
            "/analysis/intake-playbook/promote",
            post(post_intake_playbook_promote_api),
        )
}

async fn get_range_engine_config_api(
    State(st): State<SharedState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let doc = fetch_range_engine_json(&st.pool)
        .await
        .map_err(|e| {
            tracing::warn!(target: "qtss_api", error = %e, "range_engine fetch");
            ApiError::internal("range_engine config read failed")
        })?;
    log_business(
        QtssLogLevel::Debug,
        "qtss_api::range_engine",
        "get_range_engine_config",
    );
    Ok(Json(doc))
}

async fn patch_range_engine_config_api(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(patch): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut doc = fetch_range_engine_json(&st.pool)
        .await
        .map_err(|e| {
            tracing::warn!(target: "qtss_api", error = %e, "range_engine fetch before patch");
            ApiError::internal("range_engine config read failed")
        })?;
    merge_json_deep(&mut doc, &patch);
    let actor = Uuid::parse_str(&claims.sub).ok();
    upsert_range_engine_json(&st.pool, doc.clone(), actor)
        .await
        .map_err(|e| {
            tracing::warn!(target: "qtss_api", error = %e, "range_engine upsert");
            ApiError::internal("range_engine config write failed")
        })?;
    log_business(
        QtssLogLevel::Info,
        "qtss_api::range_engine",
        "patch_range_engine_config",
    );
    Ok(Json(doc))
}

async fn list_engine_symbols_api(
    State(st): State<SharedState>,
) -> Result<Json<Vec<EngineSymbolRow>>, ApiError> {
    let rows = list_engine_symbols_all(&st.pool).await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
struct PostEngineSymbolBody {
    #[serde(default)]
    pub exchange: Option<String>,
    #[serde(default)]
    pub segment: Option<String>,
    pub symbol: String,
    pub interval: String,
    pub label: Option<String>,
    /// `both` | `long_only` | `short_only` | `auto_segment`
    #[serde(default)]
    pub signal_direction_mode: Option<String>,
}

async fn post_engine_symbol_api(
    State(st): State<SharedState>,
    Json(body): Json<PostEngineSymbolBody>,
) -> Result<Json<EngineSymbolRow>, ApiError> {
    let sym = body.symbol.trim().to_uppercase();
    if sym.is_empty() {
        return Err(ApiError::bad_request("symbol boş olamaz"));
    }
    let iv = body.interval.trim().to_string();
    if iv.is_empty() {
        return Err(ApiError::bad_request("interval boş olamaz"));
    }
    let mode = body
        .signal_direction_mode
        .as_deref()
        .map(normalize_signal_direction_mode)
        .transpose()?;
    let row = EngineSymbolInsert {
        exchange: body
            .exchange
            .unwrap_or_else(|| "binance".into())
            .trim()
            .to_lowercase(),
        segment: body
            .segment
            .unwrap_or_else(|| "spot".into())
            .trim()
            .to_lowercase(),
        symbol: sym,
        interval: iv,
        label: body.label,
        signal_direction_mode: mode,
    };
    let inserted = insert_engine_symbol(&st.pool, &row).await?;
    Ok(Json(inserted))
}

#[derive(Deserialize)]
struct BulkEngineSymbolTarget {
    #[serde(default)]
    pub exchange: Option<String>,
    #[serde(default)]
    pub segment: Option<String>,
    pub symbol: String,
    pub interval: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub signal_direction_mode: Option<String>,
}

#[derive(Deserialize)]
struct PostEngineSymbolsBulkBody {
    #[serde(default)]
    pub exchange: Option<String>,
    #[serde(default)]
    pub segment: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub signal_direction_mode: Option<String>,
    pub targets: Vec<BulkEngineSymbolTarget>,
}

#[derive(Serialize)]
struct EngineSymbolBulkError {
    pub index: usize,
    pub message: String,
}

#[derive(Serialize)]
struct PostEngineSymbolsBulkResponse {
    pub inserted: Vec<EngineSymbolRow>,
    pub errors: Vec<EngineSymbolBulkError>,
}

async fn post_engine_symbols_bulk_api(
    State(st): State<SharedState>,
    Json(body): Json<PostEngineSymbolsBulkBody>,
) -> Result<Json<PostEngineSymbolsBulkResponse>, ApiError> {
    if body.targets.is_empty() {
        return Err(ApiError::bad_request("targets boş olamaz"));
    }
    if body.targets.len() > 500 {
        return Err(ApiError::bad_request("en fazla 500 hedef"));
    }
    let default_exchange = body
        .exchange
        .clone()
        .unwrap_or_else(|| "binance".into())
        .trim()
        .to_lowercase();
    let default_segment = body
        .segment
        .clone()
        .unwrap_or_else(|| "spot".into())
        .trim()
        .to_lowercase();
    let default_mode = body
        .signal_direction_mode
        .as_deref()
        .map(normalize_signal_direction_mode)
        .transpose()?;

    let mut inserted: Vec<EngineSymbolRow> = Vec::new();
    let mut errors: Vec<EngineSymbolBulkError> = Vec::new();

    for (index, t) in body.targets.into_iter().enumerate() {
        let sym = t.symbol.trim().to_uppercase();
        if sym.is_empty() {
            errors.push(EngineSymbolBulkError {
                index,
                message: "symbol boş".into(),
            });
            continue;
        }
        let iv = t.interval.trim().to_string();
        if iv.is_empty() {
            errors.push(EngineSymbolBulkError {
                index,
                message: "interval boş".into(),
            });
            continue;
        }
        let mode = t
            .signal_direction_mode
            .as_deref()
            .map(normalize_signal_direction_mode)
            .transpose();
        let mode = match mode {
            Ok(m) => m.or_else(|| default_mode.clone()),
            Err(e) => {
                errors.push(EngineSymbolBulkError {
                    index,
                    message: e.to_string(),
                });
                continue;
            }
        };
        let row = EngineSymbolInsert {
            exchange: t
                .exchange
                .as_ref()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| default_exchange.clone()),
            segment: t
                .segment
                .as_ref()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| default_segment.clone()),
            symbol: sym,
            interval: iv,
            label: t.label.or_else(|| body.label.clone()),
            signal_direction_mode: mode,
        };
        match insert_engine_symbol(&st.pool, &row).await {
            Ok(r) => inserted.push(r),
            Err(e) => errors.push(EngineSymbolBulkError {
                index,
                message: e.to_string(),
            }),
        }
    }

    log_business(
        QtssLogLevel::Info,
        "qtss_api::engine_symbols",
        "post_engine_symbols_bulk",
    );
    Ok(Json(PostEngineSymbolsBulkResponse { inserted, errors }))
}

async fn list_engine_snapshots_api(
    State(st): State<SharedState>,
) -> Result<Json<Vec<AnalysisSnapshotJoinedRow>>, ApiError> {
    let rows = list_analysis_snapshots_with_symbols(&st.pool).await?;
    Ok(Json(rows))
}

/// `engine_symbols` + worker `market_bars` health (counts, gaps, stale feed, last REST backfill).
async fn list_engine_symbol_ingestion_api(
    State(st): State<SharedState>,
) -> Result<Json<Vec<EngineSymbolIngestionJoinedRow>>, ApiError> {
    let rows = list_engine_symbols_with_ingestion(&st.pool).await.map_err(|e| {
        tracing::warn!(target: "qtss_api", error = %e, "list_engine_symbols_with_ingestion");
        ApiError::internal("ingestion state list failed".to_string())
    })?;
    Ok(Json(rows))
}

/// SPEC §7.1 — tek sembol için son `confluence` JSON (`market-context/latest` ile aynı eşleştirme).
#[derive(Deserialize)]
struct ConfluenceLatestQuery {
    pub symbol: String,
    #[serde(default)]
    pub interval: Option<String>,
    #[serde(default)]
    pub exchange: Option<String>,
    #[serde(default)]
    pub segment: Option<String>,
}

async fn get_confluence_latest_by_symbol_api(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<ConfluenceLatestQuery>,
) -> Result<Json<Option<serde_json::Value>>, ApiError> {
    let sym_in = q.symbol.trim();
    if sym_in.is_empty() {
        return Err(ApiError::bad_request("query symbol is required"));
    }
    let interval = q
        .interval
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let exchange = q
        .exchange
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let segment = q
        .segment
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let matches =
        list_engine_symbols_matching(&st.pool, sym_in, interval, exchange, segment).await?;
    let row = matches.into_iter().next().ok_or_else(|| {
        ApiError::not_found(format!(
            "no engine_symbols row for symbol={}",
            sym_in.to_uppercase()
        ))
    })?;
    let conf = fetch_analysis_snapshot_payload(&st.pool, row.id, "confluence").await?;
    Ok(Json(conf))
}

async fn list_confluence_snapshots_api(
    State(st): State<SharedState>,
) -> Result<Json<Vec<AnalysisSnapshotJoinedRow>>, ApiError> {
    let rows = list_analysis_snapshots_with_symbols(&st.pool).await?;
    Ok(Json(
        rows.into_iter()
            .filter(|r| r.engine_kind == "confluence")
            .collect(),
    ))
}

async fn list_data_snapshots_api(
    State(st): State<SharedState>,
) -> Result<Json<Vec<DataSnapshotRow>>, ApiError> {
    let rows = list_data_snapshots(&st.pool).await?;
    Ok(Json(rows))
}

/// Confluence ile aynı bağlam anahtarları (Nansen DEX + Binance funding/OI + HL + BTC Coinglass).
fn context_data_snapshot_keys_for_symbol(symbol_upper: &str) -> Vec<String> {
    let sym = symbol_upper.trim().to_uppercase();
    let base = sym
        .strip_suffix("USDT")
        .unwrap_or(sym.as_str())
        .to_lowercase();
    let mut keys = vec![
        "nansen_token_screener".to_string(),
        format!("binance_taker_{base}usdt"),
        format!("binance_premium_{base}usdt"),
        format!("binance_open_interest_{base}usdt"),
        "hl_meta_asset_ctxs".to_string(),
    ];
    if base == "btc" {
        keys.push("coinglass_netflow_btc".to_string());
        keys.push("coinglass_liquidations_btc".to_string());
    }
    keys
}

#[derive(Deserialize)]
struct MarketContextQuery {
    pub symbol: String,
    #[serde(default)]
    pub interval: Option<String>,
    #[serde(default)]
    pub exchange: Option<String>,
    #[serde(default)]
    pub segment: Option<String>,
}

#[derive(Serialize)]
struct MarketContextTechnical {
    pub signal_dashboard: Option<serde_json::Value>,
    pub trading_range: Option<serde_json::Value>,
}

/// Birleşik tek-hedef görünüm: TA snapshot’ları + `confluence` + ilgili `data_snapshots` (F7 / PLAN Phase C).
/// When no `engine_symbols` row matches, `found` is `false` and remaining fields are omitted or empty (HTTP 200 — avoids browser 404 noise).
#[derive(Serialize)]
struct MarketContextLatestResponse {
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_symbol_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exchange: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub technical: Option<MarketContextTechnical>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confluence: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_data_snapshots: Vec<DataSnapshotRow>,
}

async fn get_market_context_latest_api(
    State(st): State<SharedState>,
    Query(q): Query<MarketContextQuery>,
) -> Result<Json<MarketContextLatestResponse>, ApiError> {
    let sym_in = q.symbol.trim();
    if sym_in.is_empty() {
        return Err(ApiError::bad_request("query symbol is required"));
    }
    let interval = q
        .interval
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let exchange = q
        .exchange
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let segment = q
        .segment
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let matches =
        list_engine_symbols_matching(&st.pool, sym_in, interval, exchange, segment).await?;
    let Some(row) = matches.into_iter().next() else {
        let sym_u = sym_in.to_uppercase();
        return Ok(Json(MarketContextLatestResponse {
            found: false,
            engine_symbol_id: None,
            exchange: exchange.map(|s| s.to_string()),
            segment: segment.map(|s| s.to_string()),
            symbol: Some(sym_u),
            interval: interval.map(|s| s.to_string()),
            technical: None,
            confluence: None,
            context_data_snapshots: vec![],
        }));
    };
    let id = row.id;
    let signal_dashboard =
        fetch_analysis_snapshot_payload(&st.pool, id, "signal_dashboard").await?;
    let trading_range = fetch_analysis_snapshot_payload(&st.pool, id, "trading_range").await?;
    let confluence = fetch_analysis_snapshot_payload(&st.pool, id, "confluence").await?;
    let mut context_data_snapshots: Vec<DataSnapshotRow> = Vec::new();
    for key in context_data_snapshot_keys_for_symbol(&row.symbol) {
        if let Ok(Some(r)) = fetch_data_snapshot(&st.pool, &key).await {
            context_data_snapshots.push(r);
        }
    }
    Ok(Json(MarketContextLatestResponse {
        found: true,
        engine_symbol_id: Some(id),
        exchange: Some(row.exchange),
        segment: Some(row.segment),
        symbol: Some(row.symbol),
        interval: Some(row.interval),
        technical: Some(MarketContextTechnical {
            signal_dashboard,
            trading_range,
        }),
        confluence,
        context_data_snapshots,
    }))
}

#[derive(Deserialize)]
struct MarketContextSummaryQuery {
    #[serde(default = "default_summary_enabled_only")]
    enabled_only: bool,
    #[serde(default = "default_summary_limit")]
    limit: i64,
    #[serde(default)]
    exchange: Option<String>,
    #[serde(default)]
    segment: Option<String>,
    #[serde(default)]
    symbol: Option<String>,
}

fn default_summary_enabled_only() -> bool {
    true
}

fn default_summary_limit() -> i64 {
    100
}

#[derive(Serialize)]
struct MarketContextConfluenceBrief {
    #[serde(skip_serializing_if = "Option::is_none")]
    regime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    composite_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence_0_100: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lot_scale_hint: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    conflicts_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    conflict_codes_preview: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    computed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct MarketContextSummaryItem {
    engine_symbol_id: Uuid,
    exchange: String,
    segment: String,
    symbol: String,
    interval: String,
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    ta_durum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ta_piyasa_modu: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confluence: Option<MarketContextConfluenceBrief>,
}

fn json_f64(v: &serde_json::Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|i| i as f64))
        .or_else(|| v.as_u64().map(|u| u as f64))
}

fn confluence_brief_from_row(
    row: &MarketContextSummaryRow,
) -> Option<MarketContextConfluenceBrief> {
    if row.confluence_payload.is_none()
        && row.confluence_computed_at.is_none()
        && row.confluence_error.is_none()
    {
        return None;
    }
    let p = row.confluence_payload.as_ref();
    let regime = p
        .and_then(|x| x.get("regime"))
        .and_then(|x| x.as_str())
        .map(String::from);
    let composite_score = p.and_then(|x| x.get("composite_score")).and_then(json_f64);
    let confidence_0_100 = p.and_then(|x| x.get("confidence_0_100")).and_then(json_f64);
    let lot_scale_hint = p.and_then(|x| x.get("lot_scale_hint")).and_then(json_f64);
    let (conflicts_count, preview) = p
        .and_then(|x| x.get("conflicts"))
        .and_then(|c| c.as_array())
        .map(|arr| {
            let codes: Vec<String> = arr
                .iter()
                .filter_map(|o| o.get("code").and_then(|c| c.as_str()).map(String::from))
                .take(3)
                .collect();
            (Some(arr.len()), codes)
        })
        .unwrap_or((None, vec![]));
    Some(MarketContextConfluenceBrief {
        regime,
        composite_score,
        confidence_0_100,
        lot_scale_hint,
        conflicts_count,
        conflict_codes_preview: preview,
        computed_at: row.confluence_computed_at,
        error: row.confluence_error.clone(),
    })
}

/// `signal_dashboard` payload: nested `signal_dashboard_v2` with `schema_version` 3 wins for TA labels exposed as `ta_durum` / `ta_piyasa_modu` (`status`, `market_mode`); else v1 `durum` / `piyasa_modu`.
fn signal_dashboard_ta_brief(d: &serde_json::Value) -> (Option<String>, Option<String>) {
    let mut ta_durum = d
        .get("durum")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let mut ta_piyasa = d
        .get("piyasa_modu")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    if let Some(v2) = d.get("signal_dashboard_v2") {
        if v2.get("schema_version").and_then(|x| x.as_u64()) == Some(3) {
            if let Some(s) = v2
                .get("status")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                ta_durum = Some(s.to_string());
            }
            if let Some(s) = v2
                .get("market_mode")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                ta_piyasa = Some(s.to_string());
            }
        }
    }
    (ta_durum, ta_piyasa)
}

fn map_summary_row(row: MarketContextSummaryRow) -> MarketContextSummaryItem {
    let (ta_durum, ta_piyasa_modu) = row
        .signal_dashboard_payload
        .as_ref()
        .map(signal_dashboard_ta_brief)
        .unwrap_or((None, None));
    let confluence = confluence_brief_from_row(&row);
    MarketContextSummaryItem {
        engine_symbol_id: row.engine_symbol_id,
        exchange: row.exchange,
        segment: row.segment,
        symbol: row.symbol,
        interval: row.interval,
        enabled: row.enabled,
        ta_durum,
        ta_piyasa_modu,
        confluence,
    }
}

#[derive(Deserialize)]
struct MarketConfluenceHistoryQuery {
    /// Doğrudan hedef satırı (tercih edilen).
    #[serde(default)]
    pub engine_symbol_id: Option<Uuid>,
    /// `engine_symbol_id` yoksa: `market-context/latest` ile aynı eşleştirme kuralları.
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub interval: Option<String>,
    #[serde(default)]
    pub exchange: Option<String>,
    #[serde(default)]
    pub segment: Option<String>,
    #[serde(default = "default_confluence_history_limit")]
    pub limit: i64,
}

fn default_confluence_history_limit() -> i64 {
    50
}

/// PLAN Phase B — append-only `market_confluence_snapshots` (newest first).
async fn list_market_confluence_history_api(
    State(st): State<SharedState>,
    Query(q): Query<MarketConfluenceHistoryQuery>,
) -> Result<Json<Vec<MarketConfluenceSnapshotRow>>, ApiError> {
    let lim = q.limit.clamp(1, 200);
    let id = if let Some(id) = q.engine_symbol_id {
        id
    } else {
        let sym_in = q
            .symbol
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ApiError::bad_request("query engine_symbol_id or symbol is required"))?;
        let interval = q
            .interval
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let exchange = q
            .exchange
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let segment = q
            .segment
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let matches =
            list_engine_symbols_matching(&st.pool, sym_in, interval, exchange, segment).await?;
        let row = matches.into_iter().next().ok_or_else(|| {
            ApiError::not_found(format!(
                "no engine_symbols row for symbol={} (optional interval/exchange/segment)",
                sym_in.to_uppercase()
            ))
        })?;
        row.id
    };
    let rows = list_market_confluence_snapshots_for_symbol(&st.pool, id, lim).await?;
    Ok(Json(rows))
}

/// F7 — filtreli motor hedefleri + TA / confluence özeti (`SPEC_EXECUTION_RANGE_SIGNALS_UI` §9).
async fn list_market_context_summary_api(
    State(st): State<SharedState>,
    Query(q): Query<MarketContextSummaryQuery>,
) -> Result<Json<Vec<MarketContextSummaryItem>>, ApiError> {
    let ex = q
        .exchange
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let seg = q
        .segment
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let sym = q.symbol.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let rows =
        list_market_context_summaries(&st.pool, ex, seg, sym, q.enabled_only, q.limit).await?;
    Ok(Json(rows.into_iter().map(map_summary_row).collect()))
}

/// Son Nansen token screener snapshot’ı (`qtss-worker` + `NANSEN_API_KEY`). Satır yoksa `null`.
async fn get_nansen_snapshot_api(
    State(st): State<SharedState>,
) -> Result<Json<Option<NansenSnapshotRow>>, ApiError> {
    let row = fetch_nansen_snapshot(&st.pool, "token_screener").await?;
    Ok(Json(row))
}

#[derive(Serialize)]
struct NansenSetupsLatestResponse {
    pub run: Option<NansenSetupRunRow>,
    pub rows: Vec<NansenSetupRowDetail>,
}

/// Son başarılı `nansen_setup_scan` koşusu + en fazla 10 sıralı satır (`qtss-worker` + migration 0020).
async fn get_nansen_setups_latest_api(
    State(st): State<SharedState>,
    Extension(loc): Extension<NegotiatedLocale>,
) -> Result<Json<NansenSetupsLatestResponse>, ApiError> {
    let out = fetch_latest_nansen_setup_with_rows(&st.pool)
        .await
        .map_err(|e| {
            map_analysis_storage_err(
                e,
                &loc,
                "analysis.nansen_setups_load_failed",
                "Failed to load Nansen setup scan data.",
            )
        })?;
    let resp = match out {
        Some((run, rows)) => NansenSetupsLatestResponse {
            run: Some(run),
            rows,
        },
        None => NansenSetupsLatestResponse {
            run: None,
            rows: vec![],
        },
    };
    Ok(Json(resp))
}

#[derive(Serialize)]
struct IntakePlaybookLatestResponse {
    pub run: Option<IntakePlaybookRunRow>,
    pub candidates: Vec<IntakePlaybookCandidateRow>,
}

#[derive(Deserialize)]
struct IntakePlaybookLatestQuery {
    pub playbook_id: String,
}

/// Latest intake playbook run + ranked candidates (`qtss-worker` `intake_playbook_engine`).
async fn get_intake_playbook_latest_api(
    State(st): State<SharedState>,
    Extension(loc): Extension<NegotiatedLocale>,
    Query(q): Query<IntakePlaybookLatestQuery>,
) -> Result<Json<IntakePlaybookLatestResponse>, ApiError> {
    let pid = q.playbook_id.trim();
    if pid.is_empty() {
        return Err(ApiError::bad_request("query playbook_id is required"));
    }
    let run = fetch_latest_intake_playbook_run(&st.pool, pid)
        .await
        .map_err(|e| {
            map_analysis_storage_err(
                e,
                &loc,
                "analysis.intake_playbook_load_failed",
                "Failed to load intake playbook run.",
            )
        })?;
    let candidates = match &run {
        Some(r) => list_intake_playbook_candidates_for_run(&st.pool, r.id)
            .await
            .map_err(|e| {
                map_analysis_storage_err(
                    e,
                    &loc,
                    "analysis.intake_playbook_candidates_failed",
                    "Failed to load intake playbook candidates.",
                )
            })?,
        None => vec![],
    };
    Ok(Json(IntakePlaybookLatestResponse { run, candidates }))
}

#[derive(Deserialize)]
struct IntakePlaybookRecentQuery {
    #[serde(default = "default_intake_playbook_recent_limit")]
    limit: i64,
}

fn default_intake_playbook_recent_limit() -> i64 {
    50
}

async fn list_intake_playbook_recent_api(
    State(st): State<SharedState>,
    Extension(loc): Extension<NegotiatedLocale>,
    Query(q): Query<IntakePlaybookRecentQuery>,
) -> Result<Json<Vec<IntakePlaybookRunRow>>, ApiError> {
    let rows = list_recent_intake_playbook_runs(&st.pool, q.limit)
        .await
        .map_err(|e| {
            map_analysis_storage_err(
                e,
                &loc,
                "analysis.intake_playbook_recent_failed",
                "Failed to list recent intake playbook runs.",
            )
        })?;
    Ok(Json(rows))
}

fn normalize_intake_listing_symbol(raw: &str) -> String {
    let u = raw.trim().to_uppercase();
    if u.is_empty() {
        return u;
    }
    if u.ends_with("USDT") || u.ends_with("USDC") || u.ends_with("BUSD") {
        u
    } else {
        format!("{u}USDT")
    }
}

fn signal_direction_mode_from_intake(direction: &str) -> Option<String> {
    match direction.trim().to_uppercase().as_str() {
        "LONG" | "WATCH" => Some("long_only".into()),
        "SHORT" | "AVOID" => Some("short_only".into()),
        "LONG_OR_SHORT" => Some("both".into()),
        _ => Some("both".into()),
    }
}

fn default_intake_promote_interval() -> String {
    "15m".into()
}

#[derive(Deserialize)]
struct PostIntakePromoteBody {
    candidate_id: Uuid,
    #[serde(default)]
    exchange: Option<String>,
    #[serde(default)]
    segment: Option<String>,
    #[serde(default = "default_intake_promote_interval")]
    interval: String,
}

/// Add `engine_symbols` row from an intake candidate (**disabled** until operator enables).
async fn post_intake_playbook_promote_api(
    State(st): State<SharedState>,
    Extension(loc): Extension<NegotiatedLocale>,
    Json(body): Json<PostIntakePromoteBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let c = fetch_intake_playbook_candidate_by_id(&st.pool, body.candidate_id)
        .await
        .map_err(|e| {
            map_analysis_storage_err(
                e,
                &loc,
                "analysis.intake_promote_candidate_load_failed",
                "Failed to load intake candidate.",
            )
        })?;
    let Some(c) = c else {
        return Err(ApiError::bad_request("intake candidate not found"));
    };
    if c.merged_engine_symbol_id.is_some() {
        return Err(ApiError::bad_request(
            "candidate already promoted (merged_engine_symbol_id set)",
        ));
    }
    let run = fetch_intake_playbook_run_by_id(&st.pool, c.run_id)
        .await
        .map_err(|e| {
            map_analysis_storage_err(
                e,
                &loc,
                "analysis.intake_promote_run_load_failed",
                "Failed to load intake playbook run.",
            )
        })?;
    let Some(run) = run else {
        return Err(ApiError::internal("intake run row missing"));
    };

    let sym = normalize_intake_listing_symbol(&c.symbol);
    if sym.is_empty() {
        return Err(ApiError::bad_request("empty symbol after normalization"));
    }
    let iv = body.interval.trim();
    if iv.is_empty() {
        return Err(ApiError::bad_request("interval boş olamaz"));
    }
    let mode = signal_direction_mode_from_intake(&c.direction);
    let label = Some(format!("intake:{}", run.playbook_id));
    let row_ins = EngineSymbolInsert {
        exchange: body
            .exchange
            .unwrap_or_else(|| "binance".into())
            .trim()
            .to_lowercase(),
        segment: body
            .segment
            .unwrap_or_else(|| "futures".into())
            .trim()
            .to_lowercase(),
        symbol: sym,
        interval: iv.to_string(),
        label,
        signal_direction_mode: mode,
    };
    let es = insert_engine_symbol(&st.pool, &row_ins)
        .await
        .map_err(|e| {
            map_analysis_storage_err(
                e,
                &loc,
                "analysis.intake_promote_insert_engine_failed",
                "Failed to insert engine_symbol.",
            )
        })?;
    update_engine_symbol_enabled(&st.pool, es.id, false)
        .await
        .map_err(|e| {
            map_analysis_storage_err(
                e,
                &loc,
                "analysis.intake_promote_disable_failed",
                "Failed to set engine_symbol enabled=false.",
            )
        })?;
    update_intake_candidate_merged_engine_symbol(&st.pool, c.id, es.id)
        .await
        .map_err(|e| {
            map_analysis_storage_err(
                e,
                &loc,
                "analysis.intake_promote_merge_flag_failed",
                "Failed to record merged_engine_symbol_id.",
            )
        })?;

    log_business(
        QtssLogLevel::Info,
        "qtss_api::intake_playbook",
        "promote_candidate",
    );
    Ok(Json(json!({
        "engine_symbol": es,
        "candidate_id": c.id,
        "note": "Engine symbol created with enabled=false; enable from engine targets when ready."
    })))
}

#[derive(Deserialize)]
struct RangeSignalsQuery {
    #[serde(default = "default_range_signals_limit")]
    limit: i64,
    engine_symbol_id: Option<Uuid>,
}

fn default_range_signals_limit() -> i64 {
    100
}

async fn list_range_signals_api(
    State(st): State<SharedState>,
    Query(q): Query<RangeSignalsQuery>,
) -> Result<Json<Vec<RangeSignalEventJoinedRow>>, ApiError> {
    let rows = list_range_signal_events_joined(&st.pool, q.engine_symbol_id, q.limit).await?;
    Ok(Json(rows))
}

#[derive(Deserialize, Default)]
struct PatchEngineSymbolBody {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub signal_direction_mode: Option<String>,
}

fn normalize_signal_direction_mode(raw: &str) -> Result<String, ApiError> {
    match raw.trim().to_lowercase().as_str() {
        "both" | "bidirectional" | "long_short" | "long_and_short" => Ok("both".into()),
        "long_only" | "longonly" => Ok("long_only".into()),
        "short_only" | "shortonly" => Ok("short_only".into()),
        "auto_segment" | "auto" => Ok("auto_segment".into()),
        _ => Err(ApiError::bad_request(format!(
            "signal_direction_mode geçersiz: {raw} (both | long_only | short_only | auto_segment)"
        ))),
    }
}

async fn patch_engine_symbol_api(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchEngineSymbolBody>,
) -> Result<StatusCode, ApiError> {
    if body.enabled.is_none() && body.signal_direction_mode.is_none() {
        return Err(ApiError::bad_request(
            "gövdede enabled veya signal_direction_mode gerekli",
        ));
    }
    let mode = body
        .signal_direction_mode
        .as_deref()
        .map(normalize_signal_direction_mode)
        .transpose()?;
    let n = update_engine_symbol_patch(&st.pool, id, body.enabled, mode.as_deref()).await?;
    if n == 0 {
        return Err(ApiError::not_found("engine_symbol bulunamadı"));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `app_config.acp_chart_patterns` — DB’de yoksa Pine ACP v6 fabrika varsayılanları (migrations 0007–0009).
async fn get_chart_patterns_config(
    State(st): State<SharedState>,
    Extension(loc): Extension<NegotiatedLocale>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row = st
        .config
        .get_by_key(ACP_CHART_PATTERNS_CONFIG_KEY)
        .await
        .map_err(|e| {
            map_analysis_storage_err(
                e,
                &loc,
                "analysis.chart_patterns_config_load_failed",
                "Failed to load chart patterns configuration.",
            )
        })?;
    Ok(Json(
        row.map(|e| e.value)
            .unwrap_or_else(default_acp_chart_patterns_json),
    ))
}

/// `app_config.elliott_wave` — yoksa web ile uyumlu fabrika varsayılanları.
async fn get_elliott_wave_config(
    State(st): State<SharedState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row = st.config.get_by_key(ELLIOTT_WAVE_CONFIG_KEY).await?;
    Ok(Json(
        row.map(|e| e.value)
            .unwrap_or_else(default_elliott_wave_json),
    ))
}

#[must_use]
fn default_elliott_wave_json() -> serde_json::Value {
    json!({
        "version": 1,
        "enabled": false,
        "engine_version": "v2",
        "formations": {
            "impulse": true
        },
        "subdivision_levels": 1,
        "swing_depth": 3,
        "max_pivot_windows": 120,
        "show_projection_1w": false,
        "show_projection_1d": false,
        "show_projection_4h": false,
        "show_projection_1h": false,
        "show_projection_15m": false,
        "show_historical_waves": true,
        "show_nested_formations": true,
        "projection_multi_corrective_scenarios": false,
        "use_acp_zigzag_swing": false,
        "acp_zigzag_row_index": 0,
        "pattern_menu": {
            "motive_impulse": true,
            "motive_diagonal_leading": true,
            "motive_diagonal_ending": true,
            "corrective_zigzag": true,
            "corrective_flat": true,
            "corrective_triangle": true,
            "corrective_complex_double": true,
            "corrective_complex_triple": true
        },
        "pattern_menu_by_tf": {
            "1w": {
                "motive_impulse": true,
                "motive_diagonal_leading": true,
                "motive_diagonal_ending": true,
                "corrective_zigzag": true,
                "corrective_flat": true,
                "corrective_triangle": true,
                "corrective_complex_double": true,
                "corrective_complex_triple": true
            },
            "1d": {
                "motive_impulse": true,
                "motive_diagonal_leading": true,
                "motive_diagonal_ending": true,
                "corrective_zigzag": true,
                "corrective_flat": true,
                "corrective_triangle": true,
                "corrective_complex_double": true,
                "corrective_complex_triple": true
            },
            "4h": {
                "motive_impulse": true,
                "motive_diagonal_leading": true,
                "motive_diagonal_ending": true,
                "corrective_zigzag": true,
                "corrective_flat": true,
                "corrective_triangle": true,
                "corrective_complex_double": true,
                "corrective_complex_triple": true
            },
            "1h": {
                "motive_impulse": true,
                "motive_diagonal_leading": true,
                "motive_diagonal_ending": true,
                "corrective_zigzag": true,
                "corrective_flat": true,
                "corrective_triangle": true,
                "corrective_complex_double": true,
                "corrective_complex_triple": true
            },
            "15m": {
                "motive_impulse": true,
                "motive_diagonal_leading": true,
                "motive_diagonal_ending": true,
                "corrective_zigzag": true,
                "corrective_flat": true,
                "corrective_triangle": true,
                "corrective_complex_double": true,
                "corrective_complex_triple": true
            }
        },
        "mtf_wave_color_4h": "#e53935",
        "mtf_wave_color_1h": "#43a047",
        "mtf_wave_color_15m": "#fb8c00",
        "mtf_label_color_4h": "#e53935",
        "mtf_label_color_1h": "#43a047",
        "mtf_label_color_15m": "#fb8c00",
        "show_line_4h": true,
        "show_line_1h": true,
        "show_line_15m": true,
        "show_label_4h": true,
        "show_label_1h": true,
        "show_label_15m": true,
        "mtf_line_style_4h": "solid",
        "mtf_line_style_1h": "dashed",
        "mtf_line_style_15m": "dotted",
        "mtf_line_width_4h": 4,
        "mtf_line_width_1h": 3,
        "mtf_line_width_15m": 2,
        "show_zigzag_pivot_4h": true,
        "show_zigzag_pivot_1h": true,
        "show_zigzag_pivot_15m": true,
        "mtf_zigzag_color_4h": "#e53935",
        "mtf_zigzag_color_1h": "#43a047",
        "mtf_zigzag_color_15m": "#fb8c00",
        "mtf_zigzag_line_style_4h": "dotted",
        "mtf_zigzag_line_style_1h": "dotted",
        "mtf_zigzag_line_style_15m": "dotted",
        "mtf_zigzag_line_width_4h": 2,
        "mtf_zigzag_line_width_1h": 2,
        "mtf_zigzag_line_width_15m": 2
    })
}

#[must_use]
fn default_acp_chart_patterns_json() -> serde_json::Value {
    json!({
        "version": 1,
        "ohlc": { "open": "open", "high": "high", "low": "low", "close": "close" },
        "zigzag": [
            { "enabled": true, "length": 8, "depth": 55 },
            { "enabled": false, "length": 13, "depth": 34 },
            { "enabled": false, "length": 21, "depth": 21 },
            { "enabled": false, "length": 34, "depth": 13 }
        ],
        "scanning": {
            "number_of_pivots": 5,
            "error_threshold_percent": 20,
            "flat_threshold_percent": 20,
            "verify_bar_ratio": true,
            "bar_ratio_limit": 0.382,
            "avoid_overlap": true,
            "repaint": false,
            "last_pivot_direction": "both",
            "pivot_tail_skip_max": 0,
            "max_zigzag_levels": 0,
            "upper_direction": 1,
            "lower_direction": -1,
            "ignore_if_entry_crossed": false,
            "auto_scan_on_timeframe_change": false,
            "size_filters": {
                "filter_by_bar": false,
                "min_pattern_bars": 0,
                "max_pattern_bars": 1000,
                "filter_by_percent": false,
                "min_pattern_percent": 0,
                "max_pattern_percent": 100
            }
        },
        "pattern_groups": {
            "geometric": { "channels": true, "wedges": true, "triangles": true },
            "direction": { "rising": true, "falling": true, "flat_bidirectional": true },
            "formation_dynamics": { "expanding": true, "contracting": true, "parallel": true }
        },
        "patterns": {
            "1": { "enabled": true, "last_pivot": "both" },
            "2": { "enabled": true, "last_pivot": "both" },
            "3": { "enabled": true, "last_pivot": "both" },
            "4": { "enabled": true, "last_pivot": "both" },
            "5": { "enabled": true, "last_pivot": "both" },
            "6": { "enabled": true, "last_pivot": "both" },
            "7": { "enabled": true, "last_pivot": "both" },
            "8": { "enabled": true, "last_pivot": "both" },
            "9": { "enabled": true, "last_pivot": "both" },
            "10": { "enabled": true, "last_pivot": "both" },
            "11": { "enabled": true, "last_pivot": "both" },
            "12": { "enabled": true, "last_pivot": "both" },
            "13": { "enabled": true, "last_pivot": "both" }
        },
        "display": {
            "theme": "dark",
            "pattern_line_width": 2,
            "zigzag_line_width": 1,
            "show_pattern_label": true,
            "show_pivot_labels": true,
            "show_zigzag": true,
            "max_patterns": 20
        },
        "calculated_bars": 5000
    })
}

async fn analysis_health(Extension(_claims): Extension<AccessClaims>) -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "engine": "qtss-chart-patterns (zigzag + 6-pivot kanal)"
    }))
}

#[derive(Deserialize)]
pub struct ChannelSixBody {
    pub bars: Vec<OhlcBar>,
    /// Pine `useZigzag1..4` benzeri çoklu set: sırayla denenir, ilk eşleşme döner.
    #[serde(default)]
    pub zigzag_configs: Vec<ZigzagConfigInput>,
    #[serde(default = "default_zigzag_length")]
    pub zigzag_length: usize,
    #[serde(default = "default_zigzag_max_pivots")]
    pub zigzag_max_pivots: usize,
    #[serde(default)]
    pub zigzag_offset: usize,
    #[serde(default = "default_true")]
    pub bar_ratio_enabled: bool,
    #[serde(default = "default_bar_ratio_limit")]
    pub bar_ratio_limit: f64,
    #[serde(default = "default_flat_ratio")]
    pub flat_ratio: f64,
    #[serde(default = "default_number_of_pivots")]
    pub number_of_pivots: usize,
    #[serde(default = "default_upper_dir")]
    pub upper_direction: f64,
    #[serde(default = "default_lower_dir")]
    pub lower_direction: f64,
    /// Pine `find` ofset araması: 0..=N arası “en yeni pivotları atla” dilimleri dener.
    #[serde(default = "default_pivot_tail_skip_max")]
    pub pivot_tail_skip_max: usize,
    /// Çoklu seviye zigzag taraması: `0` yalnızca temel seviye; `N` kadar `nextlevel` denenir.
    #[serde(default = "default_max_zigzag_levels")]
    pub max_zigzag_levels: usize,
    /// Pine `allowedPatterns` benzeri filtre: boşsa tüm id'ler kabul edilir.
    #[serde(default)]
    pub allowed_pattern_ids: Vec<i32>,
    /// Pine `errorThresold/100` — `inspect` skor oranı üst sınırı (varsayılan 0.2).
    #[serde(default = "default_error_score_ratio_max")]
    pub error_score_ratio_max: f64,
    /// Pine `avoidOverlap`.
    #[serde(default = "default_true")]
    pub avoid_overlap: bool,
    /// Mevcut formasyon aralıkları: `(first_bar, last_bar)` mum indeksi.
    #[serde(default)]
    pub existing_pattern_ranges: Vec<PatternBarRange>,
    /// Pine `existingPattern`: son taramadaki ilk 5 pivot `bar_index` (6’lı pencere).
    #[serde(default)]
    pub duplicate_pivot_bars: Vec<i64>,
    /// Pine `allowedLastPivotDirections`: indeks = `pattern_type_id` (0..=13); `0` = serbest; `1`/`-1` = son pivot yönü.
    #[serde(default)]
    pub allowed_last_pivot_directions: Vec<i32>,
    /// Çizim batch’i için tema (Pine `Theme.DARK` / `LIGHT`).
    #[serde(default = "default_true")]
    pub theme_dark: bool,
    #[serde(default = "default_pattern_line_width")]
    pub pattern_line_width: u32,
    #[serde(default = "default_zigzag_line_width")]
    pub zigzag_line_width: u32,
    /// Aynı zigzag üzerinde ardışık pivot pencerelerinden en fazla kaç eşleşme dönsün (1 = eski davranış).
    #[serde(default = "default_max_matches")]
    pub max_matches: usize,
    /// Pine `abstractchartpatterns.ScanProperties.filters` (`checkSize`).
    #[serde(default)]
    pub size_filters: SizeFilters,
    /// Pine `ignoreIfEntryCrossed` — son mum kapanışı kanal bandı dışındaysa elenir.
    #[serde(default)]
    pub ignore_if_entry_crossed: bool,
    /// Pine `repaint`: `false` ise en yeni (genelde açık) mum taramaya dahil edilmez — yalnız kapanmış mumlar.
    #[serde(default)]
    pub repaint: bool,
}

fn default_max_matches() -> usize {
    1
}

#[derive(Deserialize)]
pub struct PatternBarRange {
    pub first_bar: i64,
    pub last_bar: i64,
}

#[derive(Deserialize, Clone)]
pub struct ZigzagConfigInput {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_zigzag_length")]
    pub length: usize,
    #[serde(default = "default_zigzag_max_pivots")]
    pub depth: usize,
}

#[derive(Serialize, Clone)]
pub struct UsedZigzagConfig {
    pub length: usize,
    pub depth: usize,
}

fn default_zigzag_length() -> usize {
    5
}
fn default_zigzag_max_pivots() -> usize {
    55
}
fn default_true() -> bool {
    true
}
fn default_bar_ratio_limit() -> f64 {
    0.382
}
fn default_flat_ratio() -> f64 {
    0.2
}
fn default_number_of_pivots() -> usize {
    5
}
fn default_upper_dir() -> f64 {
    1.0
}
fn default_lower_dir() -> f64 {
    -1.0
}
fn default_pivot_tail_skip_max() -> usize {
    12
}
fn default_max_zigzag_levels() -> usize {
    0
}
fn default_error_score_ratio_max() -> f64 {
    0.2
}
fn default_pattern_line_width() -> u32 {
    2
}
fn default_zigzag_line_width() -> u32 {
    1
}

impl ChannelSixBody {
    fn scan_params(&self) -> SixPivotScanParams {
        let number_of_pivots = if self.number_of_pivots == 6 { 6 } else { 5 };
        SixPivotScanParams {
            number_of_pivots,
            bar_ratio_enabled: self.bar_ratio_enabled,
            bar_ratio_limit: self.bar_ratio_limit,
            flat_ratio: self.flat_ratio,
            error_score_ratio_max: self.error_score_ratio_max,
            upper_direction: self.upper_direction,
            lower_direction: self.lower_direction,
            size_filters: self.size_filters.clone(),
            ignore_if_entry_crossed: self.ignore_if_entry_crossed,
        }
    }
}

#[derive(Serialize, Clone)]
struct PatternMatchPayload {
    outcome: ChannelSixScanOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pattern_name: Option<&'static str>,
    pattern_drawing_batch: PatternDrawingBatch,
    #[serde(skip_serializing_if = "Option::is_none")]
    formation_trade_levels: Option<FormationTradeLevels>,
    #[serde(skip_serializing_if = "Option::is_none")]
    apex: Option<ApexResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_swing: Option<FailureSwingResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    breakout_volume: Option<BreakoutVolumeResult>,
}

#[derive(Serialize)]
struct ChannelSixResponse {
    matched: bool,
    bar_count: usize,
    zigzag_pivot_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    reject: Option<ChannelSixReject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<ChannelSixScanOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    drawing: Option<ChannelSixDrawingHints>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pattern_name: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pattern_drawing_batch: Option<PatternDrawingBatch>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pattern_matches: Vec<PatternMatchPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    used_zigzag: Option<UsedZigzagConfig>,
    /// İstek gövdesindeki `repaint` (Pine: açık mum). Tarama mumları istemcinin gönderdiği dilimdedir; kırpma tipik olarak web `acpOhlcWindowForScan`.
    repaint: bool,
    /// `pattern_matches` içinde canlı/robot adayı: `pivot_tail_skip == 0` ve `zigzag_level == 0` olan ilk eşleşme.
    #[serde(skip_serializing_if = "Option::is_none")]
    live_robot_match_index: Option<usize>,
    /// First match only: same as `pattern_matches[0].formation_trade_levels` when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    formation_trade_levels: Option<FormationTradeLevels>,
    /// Faz 2 formasyonları (Double Top/Bottom, H&S, Triple, Flag).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    formations: Vec<FormationMatch>,
    /// Faz 2 formasyonlarının çizim komutları (her formasyon için ayrı batch).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    formation_drawing_batches: Vec<PatternDrawingBatch>,
}

async fn channel_six_scan(
    Extension(_claims): Extension<AccessClaims>,
    Json(body): Json<ChannelSixBody>,
) -> impl IntoResponse {
    let repaint = body.repaint;

    if body.bars.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "bars en az bir mum içermeli" })),
        )
            .into_response();
    }

    let scan = body.scan_params();
    let mut map: BTreeMap<i64, OhlcBar> = BTreeMap::new();
    for b in &body.bars {
        map.insert(b.bar_index, *b);
    }

    let mut overlap_ranges: Vec<(i64, i64)> = body
        .existing_pattern_ranges
        .iter()
        .map(|r| (r.first_bar, r.last_bar))
        .collect();
    let dup_slice =
        (!body.duplicate_pivot_bars.is_empty()).then_some(body.duplicate_pivot_bars.as_slice());
    let allowed_last = (!body.allowed_last_pivot_directions.is_empty())
        .then_some(body.allowed_last_pivot_directions.as_slice());

    let mut configs: Vec<ZigzagConfigInput> = body
        .zigzag_configs
        .iter()
        .filter(|z| z.enabled)
        .cloned()
        .collect();
    if configs.is_empty() {
        configs.push(ZigzagConfigInput {
            enabled: true,
            length: body.zigzag_length,
            depth: body.zigzag_max_pivots,
        });
    }

    let max_m = body.max_matches.clamp(1, 32);

    // TV ACP: tüm `useZigzag*` açık yapılandırmalar sırayla dener; sonuçlar `max_matches`’e kadar birleştirilir.
    let mut all_outcomes: Vec<ChannelSixScanOutcome> = Vec::new();
    let mut bar_count = map.len();
    let mut max_zigzag_pivots: usize = 0;
    let mut last_reject_when_empty: Option<ChannelSixReject> = None;
    let mut first_zigzag_used: Option<UsedZigzagConfig> = None;
    let mut more_than_one_zigzag_matched = false;

    for z in &configs {
        if all_outcomes.len() >= max_m {
            break;
        }
        let window_filter = ChannelSixWindowFilter {
            avoid_overlap: body.avoid_overlap,
            existing_ranges: overlap_ranges.as_slice(),
            duplicate_pivot_bars: dup_slice.filter(|s| s.len() == 5),
            allowed_last_pivot_directions: allowed_last,
        };
        let remaining = max_m - all_outcomes.len();
        let a = analyze_channel_six_from_bars(
            &map,
            z.length,
            z.depth,
            body.zigzag_offset,
            &scan,
            body.pivot_tail_skip_max,
            body.max_zigzag_levels,
            if body.allowed_pattern_ids.is_empty() {
                None
            } else {
                Some(body.allowed_pattern_ids.as_slice())
            },
            &window_filter,
            remaining,
        );
        bar_count = a.bar_count;
        max_zigzag_pivots = max_zigzag_pivots.max(a.zigzag_pivot_count);

        if a.outcomes.is_empty() {
            if last_reject_when_empty.is_none() {
                last_reject_when_empty = a.reject;
            }
        } else {
            if first_zigzag_used.is_some() {
                more_than_one_zigzag_matched = true;
            } else {
                first_zigzag_used = Some(UsedZigzagConfig {
                    length: z.length,
                    depth: z.depth,
                });
            }
            if body.avoid_overlap {
                for o in &a.outcomes {
                    let mn = o.pivots.iter().map(|(b, _, _)| *b).min().unwrap_or(0);
                    let mx = o.pivots.iter().map(|(b, _, _)| *b).max().unwrap_or(0);
                    overlap_ranges.push((mn, mx));
                }
            }
            all_outcomes.extend(a.outcomes);
        }
    }

    let used_zigzag = if more_than_one_zigzag_matched {
        None
    } else {
        first_zigzag_used
    };
    let reject = if all_outcomes.is_empty() {
        last_reject_when_empty
    } else {
        None
    };

    let (ref_bar, ref_close) = map
        .keys()
        .next_back()
        .and_then(|k| map.get(k).map(|b| (*k, b.close)))
        .unwrap_or((0_i64, 0.0));

    let pattern_matches: Vec<PatternMatchPayload> = all_outcomes
        .iter()
        .map(|o| {
            let id = o.scan.pattern_type_id;
            let pattern_name = if (1..=21).contains(&id) {
                pattern_name_by_acp_id(id as u8)
            } else {
                None
            };
            let formation_trade_levels = compute_formation_trade_levels(o, ref_bar, ref_close);
            let apex = compute_apex_from_outcome(o, ref_bar, 0.75);
            let failure_swing = detect_failure_swing(o, &map, 0.85);
            let breakout_volume = check_breakout_volume(&map, ref_bar, 20, 1.5);
            PatternMatchPayload {
                outcome: o.clone(),
                pattern_name,
                pattern_drawing_batch: channel_six_pattern_drawing_batch(
                    o,
                    body.theme_dark,
                    body.pattern_line_width,
                    body.zigzag_line_width,
                ),
                formation_trade_levels,
                apex,
                failure_swing,
                breakout_volume,
            }
        })
        .collect();

    // Faz 2: Klasik formasyonlar (zigzag pivotlarından)
    let (formations, formation_drawing_batches) = {
        let zz = zigzag_from_ohlc_bars(&map, 8, 50, 0);
        let chrono = pivots_chronological(&zz);
        let pivot_triples: Vec<(i64, f64, i32)> = chrono
            .iter()
            .map(|p| (p.point.index, p.point.price, p.dir))
            .collect();
        let bars_vec: Vec<OhlcBar> = map.values().copied().collect();
        let fms = scan_formations(&pivot_triples, &bars_vec, &FormationParams::default());
        let batches: Vec<PatternDrawingBatch> = fms
            .iter()
            .map(|fm| {
                formation_to_drawing_batch(
                    fm,
                    body.theme_dark,
                    body.pattern_line_width,
                    body.zigzag_line_width,
                )
            })
            .collect();
        (fms, batches)
    };

    let live_robot_match_index = all_outcomes
        .iter()
        .position(|o| o.pivot_tail_skip == 0 && o.zigzag_level == 0);

    let first = all_outcomes.first();
    let outcome = first.cloned();
    let drawing = first.map(channel_six_drawing_hints);
    let pattern_name = first.and_then(|o| {
        let id = o.scan.pattern_type_id;
        if (1..=21).contains(&id) {
            pattern_name_by_acp_id(id as u8)
        } else {
            None
        }
    });
    let pattern_drawing_batch = pattern_matches
        .first()
        .map(|p| p.pattern_drawing_batch.clone());

    let formation_trade_levels = pattern_matches
        .first()
        .and_then(|p| p.formation_trade_levels.clone());

    let matched = !all_outcomes.is_empty() || !formations.is_empty();
    (
        StatusCode::OK,
        Json(ChannelSixResponse {
            matched,
            bar_count,
            zigzag_pivot_count: max_zigzag_pivots,
            reject,
            outcome,
            drawing,
            pattern_name,
            pattern_drawing_batch,
            pattern_matches,
            used_zigzag,
            repaint,
            live_robot_match_index,
            formation_trade_levels,
            formations,
            formation_drawing_batches,
        }),
    )
        .into_response()
}

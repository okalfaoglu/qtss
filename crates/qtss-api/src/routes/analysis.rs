//! Merkezi analiz — `qtss-chart-patterns` ile formasyon iskelesi.

use std::collections::BTreeMap;

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use uuid::Uuid;
use serde::{Deserialize, Serialize};
use serde_json::json;

use qtss_chart_patterns::{
    analyze_channel_six_from_bars, channel_six_drawing_hints, channel_six_pattern_drawing_batch,
    pattern_name_by_acp_id, ChannelSixDrawingHints, ChannelSixReject, ChannelSixScanOutcome,
    ChannelSixWindowFilter, OhlcBar, PatternDrawingBatch, SizeFilters, SixPivotScanParams,
};
use qtss_storage::{
    fetch_latest_nansen_setup_with_rows, fetch_nansen_snapshot, insert_engine_symbol,
    list_analysis_snapshots_with_symbols, list_engine_symbols_all,
    list_range_signal_events_joined, update_engine_symbol_patch, AnalysisSnapshotJoinedRow,
    EngineSymbolInsert, EngineSymbolRow, NansenSetupRowDetail, NansenSetupRunRow,
    NansenSnapshotRow, RangeSignalEventJoinedRow,
};

use crate::oauth::AccessClaims;
use crate::state::SharedState;

const ACP_CHART_PATTERNS_CONFIG_KEY: &str = "acp_chart_patterns";
const ELLIOTT_WAVE_CONFIG_KEY: &str = "elliott_wave";

/// Salt okunur / dashboard rolleri (`viewer`+).
pub fn analysis_read_router() -> Router<SharedState> {
    Router::new()
        .route("/analysis/health", get(analysis_health))
        .route(
            "/analysis/chart-patterns-config",
            get(get_chart_patterns_config),
        )
        .route("/analysis/elliott-wave-config", get(get_elliott_wave_config))
        .route("/analysis/patterns/channel-six", post(channel_six_scan))
        .route("/analysis/engine/symbols", get(list_engine_symbols_api))
        .route("/analysis/engine/snapshots", get(list_engine_snapshots_api))
        .route("/analysis/engine/range-signals", get(list_range_signals_api))
        .route("/analysis/nansen/snapshot", get(get_nansen_snapshot_api))
        .route("/analysis/nansen/setups/latest", get(get_nansen_setups_latest_api))
}

/// `engine_symbols` yazımı — `trader` / `admin` (`require_ops_roles`).
pub fn analysis_write_router() -> Router<SharedState> {
    Router::new()
        .route("/analysis/engine/symbols", post(post_engine_symbol_api))
        .route("/analysis/engine/symbols/{id}", patch(patch_engine_symbol_api))
}

async fn list_engine_symbols_api(State(st): State<SharedState>) -> Result<Json<Vec<EngineSymbolRow>>, String> {
    list_engine_symbols_all(&st.pool)
        .await
        .map(Json)
        .map_err(|e| e.to_string())
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
) -> Result<Json<EngineSymbolRow>, String> {
    let sym = body.symbol.trim().to_uppercase();
    if sym.is_empty() {
        return Err("symbol boş olamaz".to_string());
    }
    let iv = body.interval.trim().to_string();
    if iv.is_empty() {
        return Err("interval boş olamaz".to_string());
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
    insert_engine_symbol(&st.pool, &row)
        .await
        .map(Json)
        .map_err(|e| e.to_string())
}

async fn list_engine_snapshots_api(
    State(st): State<SharedState>,
) -> Result<Json<Vec<AnalysisSnapshotJoinedRow>>, String> {
    list_analysis_snapshots_with_symbols(&st.pool)
        .await
        .map(Json)
        .map_err(|e| e.to_string())
}

/// Son Nansen token screener snapshot’ı (`qtss-worker` + `NANSEN_API_KEY`). Satır yoksa `null`.
async fn get_nansen_snapshot_api(
    State(st): State<SharedState>,
) -> Result<Json<Option<NansenSnapshotRow>>, String> {
    fetch_nansen_snapshot(&st.pool, "token_screener")
        .await
        .map(Json)
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct NansenSetupsLatestResponse {
    pub run: Option<NansenSetupRunRow>,
    pub rows: Vec<NansenSetupRowDetail>,
}

/// Son başarılı `nansen_setup_scan` koşusu + en fazla 10 sıralı satır (`qtss-worker` + migration 0020).
async fn get_nansen_setups_latest_api(
    State(st): State<SharedState>,
) -> Result<Json<NansenSetupsLatestResponse>, String> {
    let out = fetch_latest_nansen_setup_with_rows(&st.pool)
        .await
        .map_err(|e| e.to_string())?;
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
) -> Result<Json<Vec<RangeSignalEventJoinedRow>>, String> {
    list_range_signal_events_joined(&st.pool, q.engine_symbol_id, q.limit)
        .await
        .map(Json)
        .map_err(|e| e.to_string())
}

#[derive(Deserialize, Default)]
struct PatchEngineSymbolBody {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub signal_direction_mode: Option<String>,
}

fn normalize_signal_direction_mode(raw: &str) -> Result<String, String> {
    match raw.trim().to_lowercase().as_str() {
        "both" | "bidirectional" | "long_short" | "long_and_short" => Ok("both".into()),
        "long_only" | "longonly" => Ok("long_only".into()),
        "short_only" | "shortonly" => Ok("short_only".into()),
        "auto_segment" | "auto" => Ok("auto_segment".into()),
        _ => Err(format!(
            "signal_direction_mode geçersiz: {raw} (both | long_only | short_only | auto_segment)"
        )),
    }
}

async fn patch_engine_symbol_api(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchEngineSymbolBody>,
) -> Result<StatusCode, String> {
    if body.enabled.is_none() && body.signal_direction_mode.is_none() {
        return Err("gövdede enabled veya signal_direction_mode gerekli".into());
    }
    let mode = body
        .signal_direction_mode
        .as_deref()
        .map(normalize_signal_direction_mode)
        .transpose()?;
    let n = update_engine_symbol_patch(&st.pool, id, body.enabled, mode.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("engine_symbol bulunamadı".into());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `app_config.acp_chart_patterns` — DB’de yoksa Pine ACP v6 fabrika varsayılanları (migrations 0007–0009).
async fn get_chart_patterns_config(State(st): State<SharedState>) -> Result<Json<serde_json::Value>, String> {
    let row = st
        .config
        .get_by_key(ACP_CHART_PATTERNS_CONFIG_KEY)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(
        row.map(|e| e.value)
            .unwrap_or_else(default_acp_chart_patterns_json),
    ))
}

/// `app_config.elliott_wave` — yoksa web ile uyumlu fabrika varsayılanları.
async fn get_elliott_wave_config(State(st): State<SharedState>) -> Result<Json<serde_json::Value>, String> {
    let row = st
        .config
        .get_by_key(ELLIOTT_WAVE_CONFIG_KEY)
        .await
        .map_err(|e| e.to_string())?;
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
        "strict_wave4_overlap": false,
        "show_projection_4h": false,
        "show_projection_1h": false,
        "show_projection_15m": false,
        "show_historical_waves": false,
        "show_nested_formations": true,
        "projection_bar_hop": 22,
        "projection_steps": 12,
        "use_acp_zigzag_swing": false,
        "acp_zigzag_row_index": 0,
        "pattern_menu": {
            "motive_impulse": true,
            "motive_diagonal": true,
            "corrective_zigzag": true,
            "corrective_flat": true,
            "corrective_triangle": true,
            "corrective_complex_wxy": true
        },
        "pattern_menu_by_tf": {
            "4h": {
                "motive_impulse": true,
                "motive_diagonal": true,
                "corrective_zigzag": true,
                "corrective_flat": true,
                "corrective_triangle": true,
                "corrective_complex_wxy": true
            },
            "1h": {
                "motive_impulse": true,
                "motive_diagonal": true,
                "corrective_zigzag": true,
                "corrective_flat": true,
                "corrective_triangle": true,
                "corrective_complex_wxy": true
            },
            "15m": {
                "motive_impulse": true,
                "motive_diagonal": true,
                "corrective_zigzag": true,
                "corrective_flat": true,
                "corrective_triangle": true,
                "corrective_complex_wxy": true
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
            "max_zigzag_levels": 2,
            "upper_direction": 1,
            "lower_direction": -1,
            "ignore_if_entry_crossed": false,
            "ratio_diff_enabled": false,
            "ratio_diff_max": 1.0,
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
    /// Pine `ratioDiffEnabled` — `getRatioDiff` (üçlü tepeler / üçlü dipler) üst sınır denetimi.
    #[serde(default)]
    pub ratio_diff_enabled: bool,
    /// Pine `ratioDiff` eşiği (`getRatioDiff` ≤ bu değer).
    #[serde(default = "default_ratio_diff_max")]
    pub ratio_diff_max: f64,
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
    pub length: usize,
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
fn default_ratio_diff_max() -> f64 {
    1.0
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
            ratio_diff_enabled: self.ratio_diff_enabled,
            ratio_diff_max: self.ratio_diff_max,
        }
    }
}

#[derive(Serialize)]
struct PatternMatchPayload {
    outcome: ChannelSixScanOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pattern_name: Option<&'static str>,
    pattern_drawing_batch: PatternDrawingBatch,
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
    let dup_slice = (!body.duplicate_pivot_bars.is_empty()).then_some(body.duplicate_pivot_bars.as_slice());
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

    let pattern_matches: Vec<PatternMatchPayload> = all_outcomes
        .iter()
        .map(|o| {
            let id = o.scan.pattern_type_id;
            let pattern_name = if (1..=13).contains(&id) {
                pattern_name_by_acp_id(id as u8)
            } else {
                None
            };
            PatternMatchPayload {
                outcome: o.clone(),
                pattern_name,
                pattern_drawing_batch: channel_six_pattern_drawing_batch(
                    o,
                    body.theme_dark,
                    body.pattern_line_width,
                    body.zigzag_line_width,
                ),
            }
        })
        .collect();

    let live_robot_match_index = all_outcomes
        .iter()
        .position(|o| o.pivot_tail_skip == 0 && o.zigzag_level == 0);

    let first = all_outcomes.first();
    let outcome = first.cloned();
    let drawing = first.map(channel_six_drawing_hints);
    let pattern_name = first.and_then(|o| {
        let id = o.scan.pattern_type_id;
        if (1..=13).contains(&id) {
            pattern_name_by_acp_id(id as u8)
        } else {
            None
        }
    });
    let pattern_drawing_batch = pattern_matches
        .first()
        .map(|p| p.pattern_drawing_batch.clone());

    let matched = !all_outcomes.is_empty();
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
        }),
    )
        .into_response()
}

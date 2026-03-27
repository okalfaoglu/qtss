//! Merkezi analiz — `qtss-chart-patterns` ile formasyon iskelesi.

use std::collections::BTreeMap;

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use qtss_chart_patterns::{
    analyze_channel_six_from_bars, channel_six_drawing_hints, channel_six_pattern_drawing_batch,
    pattern_name_by_acp_id, ChannelSixDrawingHints, ChannelSixReject, ChannelSixScanOutcome,
    ChannelSixWindowFilter, OhlcBar, PatternDrawingBatch, SizeFilters, SixPivotScanParams,
};

use crate::oauth::AccessClaims;
use crate::state::SharedState;

const ACP_CHART_PATTERNS_CONFIG_KEY: &str = "acp_chart_patterns";
const ELLIOTT_WAVE_CONFIG_KEY: &str = "elliott_wave";

pub fn analysis_router() -> Router<SharedState> {
    Router::new()
        .route("/analysis/health", get(analysis_health))
        .route(
            "/analysis/chart-patterns-config",
            get(get_chart_patterns_config),
        )
        .route("/analysis/elliott-wave-config", get(get_elliott_wave_config))
        .route("/analysis/patterns/channel-six", post(channel_six_scan))
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
        "projection_bar_hop": 22,
        "projection_steps": 12,
        "use_acp_zigzag_swing": false,
        "acp_zigzag_row_index": 0,
        "mtf_wave_color_4h": "#e53935",
        "mtf_wave_color_1h": "#43a047",
        "mtf_wave_color_15m": "#fb8c00"
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
        }),
    )
        .into_response()
}

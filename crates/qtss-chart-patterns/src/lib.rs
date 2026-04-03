//! Grafik formasyonları: Pine **Auto Chart Patterns** ile uyumlu **sayısal** parçalar, çizim JSON’u ve doğrusal fiyat.
//!
//! **Taşınan (Pine ile aynı kural):** `check_bar_ratio`, `get_ratio_diff` (Pine `Pattern.ratioDiff` alanı; `find` içinde eşik filtresi yok),
//! `trend_line_inspect`,
//! `inspect_pick_best_three_point_line`, `resolve_pattern_type_id`, tema RGB tabloları, `line_price_at_bar_index`,
//! `pattern_name_by_acp_id`, [`PatternDrawingBatch`](PatternDrawingBatch).
//!
//! **Taşınan:** [`zigzag`](zigzag) — `ZigzagLite::pivot_candle`, `calculate_bar`, `run_series`, `next_level_from_pivot_prices`.
//!
//! **Taşınan:** [`find`](find) — `zigzag_from_ohlc_bars`, 6 alterne pivot için `scan_six_alternating_pivots`.
//!
//! **Trendoscope/utils:** `Theme.getColors` → [`THEME_DARK_RGB`]/[`THEME_LIGHT_RGB`] (`theme` modülü); `Line.get_price` → [`line_price_at_bar_index`].
//! `check_overflow` / `get_trend_series` ayrı fonksiyon olarak yok; `trend_line_inspect` ve zigzag akışı ile örtüşür.

pub mod apex;
mod dashboard_v1;
mod dashboard_v2_envelope;
pub mod failure_swing;
mod volume_analysis;
pub mod formations;
mod formation_trade_levels;
mod find;
mod ohlc;
mod pattern_catalog;
mod resolve;
mod scan;
mod theme;
mod trading_range;
mod zigzag;

pub use apex::{compute_apex_bar, compute_apex_from_outcome, ApexResult};
pub use failure_swing::{
    check_breakout_volume, detect_failure_swing, BreakoutVolumeResult, FailureSwingResult,
};
pub use formations::{
    detect_bearish_flag, detect_bullish_flag, detect_double_bottom, detect_double_top,
    detect_head_and_shoulders, detect_inverse_head_and_shoulders, detect_triple_bottom,
    detect_triple_top, scan_formations, FormationMatch, FormationParams,
};
pub use pattern_catalog::{pattern_name_by_id, PatternId};

pub use dashboard_v1::{
    compute_signal_dashboard_v1, compute_signal_dashboard_v1_with_policy, SignalDashboardV1,
    SignalDirectionPolicy,
};
pub use dashboard_v2_envelope::{signal_dashboard_v2_envelope_from_v1, SignalDashboardV2Envelope};
pub use formation_trade_levels::{
    compute_formation_trade_levels, FormationTakeProfit, FormationTradeLevels, FormationTradeSide,
};
pub use find::{
    analyze_channel_six_from_bars, channel_six_drawing_hints, last_six_pivots_chrono,
    scan_six_alternating_pivots, six_pivots_chrono_tail_skip, try_scan_channel_six_from_bars,
    zigzag_from_ohlc_bars, ChannelLineEndpoint, ChannelSixAnalyzeResult, ChannelSixDrawingHints,
    ChannelSixReject, ChannelSixRejectCode, ChannelSixScanOutcome, ChannelSixWindowFilter,
    PivotTriple, SixPivotScanParams, SixPivotScanResult, SizeFilters,
};
pub use ohlc::OhlcBar;
pub use resolve::resolve_pattern_type_id;
pub use scan::{
    check_bar_ratio, get_ratio_diff, in_range, inspect_pick_best_three_point_line,
    inspect_two_point_line, trend_line_inspect,
};
pub use theme::{rgb_to_hex, THEME_DARK_RGB, THEME_LIGHT_RGB};
pub use trading_range::{analyze_trading_range, TradingRangeParams, TradingRangeResult};
pub use zigzag::{
    next_level_from_pivot_prices, next_level_from_zigzag, pivots_chronological, ChartPoint,
    ZigzagFlags, ZigzagLite, ZigzagPivot,
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Trendoscope Pine v6 **`Trendoscope/LineWrapper`** — `export method get_price(Line this, int bar)`.
///
/// Formül (Pine ile aynı):
/// `stepPerBar = (p2.price - p1.price) / (p2.index - p1.index)`,
/// `p1.price + (bar - p1.index) * stepPerBar`.
/// İki uç aynı `index` ise Pine’da bölüm sıfır → burada [`None`].
///
/// **Taşınmayan `Line` alanları:** `xloc`, `extend`, `color`, `style`, `width`, `obj` — TV çizim nesnesi;
/// QTSS’te [`DrawingCommand::TrendLine`](DrawingCommand::TrendLine) + web LWC `LineSeries`;
/// `extend` / `extend_bars` ile Pine `line.extend` benzeri sınırlı ışın (grafik mum aralığına kırpılır).
#[must_use]
pub fn line_price_at_bar_index(
    p1_bar: i64,
    p1_price: f64,
    p2_bar: i64,
    p2_price: f64,
    bar: i64,
) -> Option<f64> {
    let d = p2_bar - p1_bar;
    if d == 0 {
        return None;
    }
    let step = (p2_price - p1_price) / d as f64;
    Some(p1_price + (bar - p1_bar) as f64 * step)
}

/// `basechartpatterns.getPatternNameById` ile **kimlik eşlemesi** ([`pattern_name_by_id`] ile aynı tablo).
#[must_use]
pub fn pattern_name_by_acp_id(id: u8) -> Option<&'static str> {
    if !(1..=13).contains(&id) {
        return None;
    }
    Some(pattern_name_by_id(id as i32))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimePrice {
    pub time_ms: i64,
    pub price: f64,
    /// GUI: mum dizisi indeksine göre gerçek zaman eşlemesi (sunucu `time_ms` yedek).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bar_index: Option<i64>,
}

/// Pivot numara etiketi — grafikte tepe/dip tarafına yerleştirmek için (H/L ile çakışmayı azaltır).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PivotLabelAnchor {
    High,
    Low,
}

/// Pine `line.extend` — web tarafında `line_price_at_bar_index` ile uç mumlara kadar uzatılır.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendLineExtend {
    #[default]
    None,
    Left,
    Right,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DrawingCommand {
    TrendLine {
        p1: TimePrice,
        p2: TimePrice,
        line_width: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        color_hex: Option<String>,
        #[serde(default)]
        extend: TrendLineExtend,
        #[serde(default = "default_trend_extend_bars")]
        extend_bars: u32,
    },
    ZigzagPolyline {
        points: Vec<TimePrice>,
        line_width: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        color_hex: Option<String>,
    },
    PatternLabel {
        at: TimePrice,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        color_hex: Option<String>,
    },
    PivotLabel {
        at: TimePrice,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        color_hex: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<PivotLabelAnchor>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternDrawingBatch {
    pub batch_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern_type_id: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern_name: Option<String>,
    pub commands: Vec<DrawingCommand>,
}

fn default_trend_extend_bars() -> u32 {
    48
}

#[inline]
fn time_price_bar(bar: i64, price: f64) -> TimePrice {
    TimePrice {
        time_ms: bar.saturating_mul(60_000),
        price,
        bar_index: Some(bar),
    }
}

/// Pine `Pattern.draw()` ile aynı sıra: iki trend çizgisi, zigzag polyline, formasyon etiketi, pivot 1..6.
#[must_use]
pub fn channel_six_pattern_drawing_batch(
    outcome: &ChannelSixScanOutcome,
    theme_dark: bool,
    pattern_line_width: u32,
    zigzag_line_width: u32,
) -> PatternDrawingBatch {
    let hints = channel_six_drawing_hints(outcome);
    let id = outcome.scan.pattern_type_id;
    let theme: &[(u8, u8, u8)] = if theme_dark {
        THEME_DARK_RGB.as_slice()
    } else {
        THEME_LIGHT_RGB.as_slice()
    };
    let ci = if (1..=13).contains(&id) {
        (id as usize - 1) % theme.len()
    } else {
        0
    };
    let (r, g, b) = theme[ci];
    let color_hex = Some(rgb_to_hex(r, g, b));
    let zigzag_hex = color_hex.clone();
    let name = if (1..=13).contains(&id) {
        pattern_name_by_acp_id(id as u8).unwrap_or("Pattern")
    } else {
        "Pattern"
    }
    .to_string();

    let mut commands = Vec::with_capacity(11);
    // Pine `Pattern.draw()` → `Line.draw()` with default `extend = extend.none` (LineWrapper).
    commands.push(DrawingCommand::TrendLine {
        p1: time_price_bar(hints.upper[0].bar_index, hints.upper[0].price),
        p2: time_price_bar(hints.upper[1].bar_index, hints.upper[1].price),
        line_width: pattern_line_width,
        color_hex: color_hex.clone(),
        extend: TrendLineExtend::None,
        extend_bars: 0,
    });
    commands.push(DrawingCommand::TrendLine {
        p1: time_price_bar(hints.lower[0].bar_index, hints.lower[0].price),
        p2: time_price_bar(hints.lower[1].bar_index, hints.lower[1].price),
        line_width: pattern_line_width,
        color_hex: color_hex.clone(),
        extend: TrendLineExtend::None,
        extend_bars: 0,
    });
    let zz: Vec<TimePrice> = outcome
        .pivots
        .iter()
        .map(|(b, pr, _)| time_price_bar(*b, *pr))
        .collect();
    commands.push(DrawingCommand::ZigzagPolyline {
        points: zz,
        line_width: zigzag_line_width,
        color_hex: zigzag_hex,
    });
    let last = outcome.pivots.last().copied().unwrap_or((
        hints.upper[1].bar_index,
        hints.upper[1].price,
        0,
    ));
    commands.push(DrawingCommand::PatternLabel {
        at: time_price_bar(last.0, last.1),
        text: name.clone(),
        color_hex: color_hex.clone(),
    });
    for (i, (b, pr, dir)) in outcome.pivots.iter().enumerate() {
        let anchor = Some(if *dir > 0 {
            PivotLabelAnchor::High
        } else {
            PivotLabelAnchor::Low
        });
        commands.push(DrawingCommand::PivotLabel {
            at: time_price_bar(*b, *pr),
            text: (i + 1).to_string(),
            color_hex: color_hex.clone(),
            anchor,
        });
    }

    PatternDrawingBatch {
        batch_id: Uuid::new_v4(),
        pattern_type_id: (1..=13).contains(&id).then_some(id as u8),
        pattern_name: Some(name),
        commands,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_price_matches_linear() {
        let y = line_price_at_bar_index(0, 100.0, 10, 200.0, 5).unwrap();
        assert!((y - 150.0).abs() < 1e-9);
    }

    #[test]
    fn line_price_parallel_bars_none() {
        assert!(line_price_at_bar_index(5, 1.0, 5, 2.0, 5).is_none());
    }

    /// Pine `Line.get_price` adımlarının birebir kopyası ile sonuç aynı mı.
    #[test]
    fn line_price_matches_pine_line_wrapper_formula() {
        let p1_index: i64 = 3;
        let p1_price = 10.0;
        let p2_index: i64 = 13;
        let p2_price = 30.0;
        let bar: i64 = 8_i64;
        let step_per_bar = (p2_price - p1_price) / (p2_index - p1_index) as f64;
        let distance = bar - p1_index;
        let pine = p1_price + distance as f64 * step_per_bar;
        let rust = line_price_at_bar_index(p1_index, p1_price, p2_index, p2_price, bar).unwrap();
        assert!((pine - rust).abs() < 1e-12);
        assert!((rust - 20.0).abs() < 1e-9);
    }
}

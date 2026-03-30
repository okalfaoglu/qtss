use std::cmp::min;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use qtss_chart_patterns::{
    analyze_channel_six_from_bars, ChannelSixScanOutcome, ChannelSixWindowFilter, OhlcBar,
    SixPivotScanParams, SizeFilters,
};

// NOTE:
// - Bu test harness "Pine'dan alınan golden JSON" ile Rust çıktısını karşılaştırmak için iskelet sağlar.
// - Güncel olarak testdata içinde sample/gösterim amaçlı dosyalar bulunur; `expected` alanı null ise
//   sadece Rust motoru çalıştırılır (karşılaştırma atlanır).

const TESTDATA_GLOB_DIR: &str = "testdata/pine_parity/channel-six";

#[derive(Debug, serde::Deserialize)]
struct GoldenBar {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct GoldenZigzagConfig {
    enabled: Option<bool>,
    length: usize,
    depth: usize,
}

#[derive(Debug, serde::Deserialize)]
struct GoldenPatternStepExpected {
    matched: bool,
    outcomes: Option<Vec<ChannelSixScanOutcome>>,
}

#[derive(Debug, serde::Deserialize)]
struct GoldenStep {
    /// Pine'da her bar için `bar_index` ilerledikçe yaptığımız tarama "current bar".
    current_bar_index: usize,
    /// Bu step için `Pine` beklenen çıktısı.
    expected: Option<GoldenPatternStepExpected>,
}

#[derive(Debug, serde::Deserialize)]
struct GoldenRequest {
    calculated_bars: usize,
    repaint: bool,

    // Kanal tarama parametreleri (qtss-api /analysis/patterns/channel-six request-body parity).
    // Zigzag multi-set (useZigzag1..4 benzeri): boşsa zigzag_length + zigzag_max_pivots kullanılır.
    zigzag_configs: Option<Vec<GoldenZigzagConfig>>,
    zigzag_length: Option<usize>,
    zigzag_max_pivots: Option<usize>,
    zigzag_offset: Option<usize>,

    number_of_pivots: Option<usize>,

    bar_ratio_enabled: Option<bool>,
    bar_ratio_limit: Option<f64>,
    flat_ratio: Option<f64>,
    error_score_ratio_max: Option<f64>,
    upper_direction: Option<f64>,
    lower_direction: Option<f64>,

    pivot_tail_skip_max: Option<usize>,
    max_zigzag_levels: Option<usize>,

    allowed_pattern_ids: Option<Vec<i32>>,
    // Pine `avoidOverlap` ve pencere filtreleri:
    avoid_overlap: Option<bool>,
    existing_pattern_ranges: Option<Vec<[i64; 2]>>,
    duplicate_pivot_bars: Option<Vec<i64>>,
    allowed_last_pivot_directions: Option<Vec<i32>>,

    size_filters: Option<SizeFilters>,
    ignore_if_entry_crossed: Option<bool>,

    ratio_diff_enabled: Option<bool>,
    ratio_diff_max: Option<f64>,

    max_matches: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
struct GoldenChannelSixCase {
    name: String,
    ohlc: Vec<GoldenBar>,
    request: GoldenRequest,
    steps: Vec<GoldenStep>,
}

fn approx_eq(a: f64, b: f64) -> bool {
    // Kanal/zigzag hesaplarında tamamen deterministik olmayı hedefliyoruz; f64 eşitliği çok sık
    // yeterli ama golden dosyası dışarıdan üretildiğinde JSON parse yüzünden mikrosapmalar olabilir.
    (a - b).abs() < 1e-9
}

fn compare_outcomes(
    lhs: &[ChannelSixScanOutcome],
    rhs: &[ChannelSixScanOutcome],
) -> Result<(), String> {
    if lhs.len() != rhs.len() {
        return Err(format!(
            "outcomes length mismatch: {} != {}",
            lhs.len(),
            rhs.len()
        ));
    }
    for (i, (a, b)) in lhs.iter().zip(rhs.iter()).enumerate() {
        if a.pivots.len() != b.pivots.len() {
            return Err(format!(
                "step outcome[{i}].pivots len mismatch: {} != {}",
                a.pivots.len(),
                b.pivots.len()
            ));
        }
        if a.scan.pattern_type_id != b.scan.pattern_type_id {
            return Err(format!(
                "step outcome[{i}] scan.pattern_type_id mismatch: {} != {}",
                a.scan.pattern_type_id, b.scan.pattern_type_id
            ));
        }
        if a.scan.pick_upper != b.scan.pick_upper || a.scan.pick_lower != b.scan.pick_lower {
            return Err(format!(
                "step outcome[{i}] pick mismatch: ({}, {}) != ({}, {})",
                a.scan.pick_upper, a.scan.pick_lower, b.scan.pick_upper, b.scan.pick_lower
            ));
        }
        if a.scan.upper_ok != b.scan.upper_ok || a.scan.lower_ok != b.scan.lower_ok {
            return Err(format!("step outcome[{i}] ok flags mismatch"));
        }
        if a.scan.upper_score.to_bits() != b.scan.upper_score.to_bits()
            && !approx_eq(a.scan.upper_score, b.scan.upper_score)
        {
            return Err(format!(
                "step outcome[{i}] upper_score mismatch: {} != {}",
                a.scan.upper_score, b.scan.upper_score
            ));
        }
        if a.scan.lower_score.to_bits() != b.scan.lower_score.to_bits()
            && !approx_eq(a.scan.lower_score, b.scan.lower_score)
        {
            return Err(format!(
                "step outcome[{i}] lower_score mismatch: {} != {}",
                a.scan.lower_score, b.scan.lower_score
            ));
        }
        if a.zigzag_pivot_count != b.zigzag_pivot_count {
            return Err(format!(
                "step outcome[{i}] zigzag_pivot_count mismatch: {} != {}",
                a.zigzag_pivot_count, b.zigzag_pivot_count
            ));
        }
        if a.pivot_tail_skip != b.pivot_tail_skip {
            return Err(format!(
                "step outcome[{i}] pivot_tail_skip mismatch: {} != {}",
                a.pivot_tail_skip, b.pivot_tail_skip
            ));
        }
        if a.zigzag_level != b.zigzag_level {
            return Err(format!(
                "step outcome[{i}] zigzag_level mismatch: {} != {}",
                a.zigzag_level, b.zigzag_level
            ));
        }
        for (pidx, (pa, pb)) in a.pivots.iter().zip(b.pivots.iter()).enumerate() {
            let (aba, pra, dira) = *pa;
            let (abb, prb, dirb) = *pb;
            if aba != abb {
                return Err(format!(
                    "step outcome[{i}] pivot[{pidx}] bar_index mismatch: {} != {}",
                    aba, abb
                ));
            }
            if dira != dirb {
                return Err(format!(
                    "step outcome[{i}] pivot[{pidx}] dir mismatch: {} != {}",
                    dira, dirb
                ));
            }
            if !approx_eq(pra, prb) {
                return Err(format!(
                    "step outcome[{i}] pivot[{pidx}] price mismatch: {} != {}",
                    pra, prb
                ));
            }
        }
    }
    Ok(())
}

fn run_channel_six_at_step(
    full: &[GoldenBar],
    current_bar_index: usize,
    req: &GoldenRequest,
) -> Vec<ChannelSixScanOutcome> {
    let i = current_bar_index;
    let series_len = full.len();
    assert!(i < series_len, "current_bar_index out of range");

    let cap = min(req.calculated_bars.max(1), i + 1);
    let start = (i + 1).saturating_sub(cap);
    let mut window: Vec<&GoldenBar> = full[start..=i].iter().collect();

    if !req.repaint && window.len() > 1 {
        window.pop();
    }

    let mut map: BTreeMap<i64, OhlcBar> = BTreeMap::new();
    for (j, b) in window.iter().enumerate() {
        map.insert(
            j as i64,
            OhlcBar {
                open: b.open,
                high: b.high,
                low: b.low,
                close: b.close,
                bar_index: j as i64,
            },
        );
    }

    let bar_ratio_enabled = req.bar_ratio_enabled.unwrap_or(true);
    let bar_ratio_limit = req.bar_ratio_limit.unwrap_or(0.382);
    let flat_ratio = req.flat_ratio.unwrap_or(0.2);
    let number_of_pivots = req.number_of_pivots.unwrap_or(5);
    let upper_direction = req.upper_direction.unwrap_or(1.0);
    let lower_direction = req.lower_direction.unwrap_or(-1.0);
    let error_score_ratio_max = req.error_score_ratio_max.unwrap_or(0.2);
    let pivot_tail_skip_max = req.pivot_tail_skip_max.unwrap_or(12);
    let max_zigzag_levels = req.max_zigzag_levels.unwrap_or(0);
    let avoid_overlap = req.avoid_overlap.unwrap_or(true);
    let size_filters = req.size_filters.clone().unwrap_or_default();
    let ignore_if_entry_crossed = req.ignore_if_entry_crossed.unwrap_or(false);
    let ratio_diff_enabled = req.ratio_diff_enabled.unwrap_or(false);
    let ratio_diff_max = req.ratio_diff_max.unwrap_or(1.0);
    let max_matches = req.max_matches.unwrap_or(1).max(1);

    let scan_params = SixPivotScanParams {
        number_of_pivots: if number_of_pivots == 6 { 6 } else { 5 },
        bar_ratio_enabled,
        bar_ratio_limit,
        flat_ratio,
        error_score_ratio_max,
        upper_direction,
        lower_direction,
        size_filters,
        ignore_if_entry_crossed,
        ratio_diff_enabled,
        ratio_diff_max,
    };

    let allowed_pattern_ids = req.allowed_pattern_ids.clone().unwrap_or_default();
    let allowed_pattern_opt = if allowed_pattern_ids.is_empty() {
        None
    } else {
        Some(allowed_pattern_ids.as_slice())
    };

    let mut overlap_ranges: Vec<(i64, i64)> = req
        .existing_pattern_ranges
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r[0], r[1]))
        .collect();

    let dup_bars = req.duplicate_pivot_bars.clone().unwrap_or_default();
    let allowed_last = req
        .allowed_last_pivot_directions
        .clone()
        .unwrap_or_default();

    let zigzag_length = req.zigzag_length.unwrap_or(5);
    let zigzag_max_pivots = req.zigzag_max_pivots.unwrap_or(55);
    let zigzag_offset = req.zigzag_offset.unwrap_or(0);

    let enabled_zigzags: Vec<_> = req
        .zigzag_configs
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter(|z| z.enabled.unwrap_or(true))
        .collect();

    let configs: Vec<GoldenZigzagConfig> = if enabled_zigzags.is_empty() {
        vec![GoldenZigzagConfig {
            enabled: Some(true),
            length: zigzag_length,
            depth: zigzag_max_pivots,
        }]
    } else {
        enabled_zigzags
    };

    let max_m = max_matches.clamp(1, 32);
    let mut all_outcomes: Vec<ChannelSixScanOutcome> = Vec::new();

    for z in configs {
        if all_outcomes.len() >= max_m {
            break;
        }
        let remaining = max_m - all_outcomes.len();
        // qtss-api sadece `duplicate_pivot_bars.len()==5` ise duplicate penceresini verir.
        let window_filter = ChannelSixWindowFilter {
            avoid_overlap,
            existing_ranges: overlap_ranges.as_slice(),
            duplicate_pivot_bars: if dup_bars.len() == 5 {
                Some(dup_bars.as_slice())
            } else {
                None
            },
            allowed_last_pivot_directions: if allowed_last.is_empty() {
                None
            } else {
                Some(allowed_last.as_slice())
            },
        };

        let a = analyze_channel_six_from_bars(
            &map,
            z.length,
            z.depth,
            zigzag_offset,
            &scan_params,
            pivot_tail_skip_max,
            max_zigzag_levels,
            allowed_pattern_opt,
            &window_filter,
            remaining,
        );

        if !a.outcomes.is_empty() {
            if avoid_overlap {
                for o in &a.outcomes {
                    let mn = o.pivots.iter().map(|(b, _, _)| *b).min().unwrap_or(0);
                    let mx = o.pivots.iter().map(|(b, _, _)| *b).max().unwrap_or(0);
                    overlap_ranges.push((mn, mx));
                }
            }
            all_outcomes.extend(a.outcomes);
        }
    }

    all_outcomes
}

fn case_files() -> Vec<PathBuf> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(TESTDATA_GLOB_DIR);
    let mut out: Vec<PathBuf> = Vec::new();
    if !base.exists() {
        return out;
    }
    for ent in fs::read_dir(base).expect("testdata dir read_dir failed") {
        let ent = ent.expect("read_dir ent");
        let p = ent.path();
        if p.extension().is_some_and(|e| e == "json") {
            out.push(p);
        }
    }
    out.sort();
    out
}

#[test]
fn pine_golden_channel_six_runs_and_compares() {
    let files = case_files();
    assert!(
        !files.is_empty(),
        "No golden JSON files found. Add one under testdata/pine_parity/channel-six/*.json"
    );

    for path in files {
        let txt = fs::read_to_string(&path).expect("read golden json");
        let case: GoldenChannelSixCase = serde_json::from_str(&txt).expect("parse golden json");

        for step in &case.steps {
            let got = run_channel_six_at_step(&case.ohlc, step.current_bar_index, &case.request);
            match &step.expected {
                None => {
                    // Smoke-only: sadece çalıştığını doğrula.
                    let _ = got;
                }
                Some(exp) => {
                    if exp.matched == got.is_empty() {
                        panic!(
                            "{} step current_bar_index={} matched mismatch: exp={} got={}",
                            case.name,
                            step.current_bar_index,
                            exp.matched,
                            !got.is_empty()
                        );
                    }
                    if exp.matched {
                        let exp_outcomes = exp.outcomes.clone().unwrap_or_default();
                        let res = compare_outcomes(&got, &exp_outcomes);
                        if let Err(e) = res {
                            panic!(
                                "{} mismatch at step current_bar_index={}: {}",
                                case.name, step.current_bar_index, e
                            );
                        }
                    }
                }
            }
        }
    }
}

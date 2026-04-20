//! Aşama 5.B — centralized `(family, subkind, anchors)` → `render_geometry`
//! dispatch for the v2 detection pipeline.
//!
//! Instead of editing ~45 detectors to each emit their own geometry, we
//! derive it once at the orchestrator's insert boundary. The dispatch is
//! a prefix-matched rule table (CLAUDE.md #1 — look-up, not scattered
//! match). Each rule owns:
//!
//!   * a family check (`family == "classical"`)
//!   * a subkind predicate (prefix / suffix / explicit list)
//!   * a builder fn that consumes the raw anchor JSON array and returns
//!     `{ "kind": ..., "payload": ... }`
//!
//! Wyckoff and range already have bespoke chart overlays (Wyckoff box,
//! zone rectangles) so they return `None` here and the frontend falls
//! back to their legacy paths. TBM is a signal, not a pattern — also
//! no geometry.
//!
//! Anchors arrive as a `Vec<{bar_index, time, price, level, label}>`.
//! All prices are serialized as *strings* (Decimal lossless), so the
//! builder passes them through as strings — the frontend registry
//! calls `Number(...)` before rendering.

use serde_json::{json, Value};

/// Entry point: returns the `render_geometry` JSON blob the orchestrator
/// should attach to `NewDetection`, or `None` when the family already
/// owns a bespoke overlay or no archetype matches.
pub fn derive(family: &str, subkind: &str, anchors: &Value) -> Option<Value> {
    let pts = extract_points(anchors)?;
    for rule in RULES {
        if (rule.matches)(family, subkind) {
            return (rule.build)(subkind, &pts);
        }
    }
    None
}

#[derive(Clone)]
struct Anchor {
    time: String,
    price: String,
    label: Option<String>,
}

fn extract_points(anchors: &Value) -> Option<Vec<Anchor>> {
    let arr = anchors.as_array()?;
    let out: Vec<Anchor> = arr
        .iter()
        .filter_map(|v| {
            let time = v.get("time")?.as_str()?.to_string();
            let price = v
                .get("price")
                .map(|p| match p {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => String::new(),
                })
                .filter(|s| !s.is_empty())?;
            let label = v
                .get("label")
                .and_then(|l| l.as_str())
                .map(|s| s.to_string());
            Some(Anchor { time, price, label })
        })
        .collect();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn pt(a: &Anchor, label: Option<&str>) -> Value {
    match label.or(a.label.as_deref()) {
        Some(l) => json!({ "time": a.time, "price": a.price, "label": l }),
        None => json!({ "time": a.time, "price": a.price }),
    }
}

fn labelled_points(pts: &[Anchor], labels: &[&str]) -> Vec<Value> {
    pts.iter()
        .enumerate()
        .map(|(i, a)| pt(a, labels.get(i).copied()))
        .collect()
}

// ── Rule table ────────────────────────────────────────────────────────

struct Rule {
    matches: fn(&str, &str) -> bool,
    build: fn(&str, &[Anchor]) -> Option<Value>,
}

const RULES: &[Rule] = &[
    // Candle family → single arrow annotation on the close anchor.
    Rule {
        matches: |fam, _| fam == "candle",
        build: build_candle,
    },
    // Gap family → rectangle between pre/post gap prices.
    Rule {
        matches: |fam, _| fam == "gap",
        build: build_gap,
    },
    // Classical V tops/bottoms → v_spike kind.
    Rule {
        matches: |fam, sk| fam == "classical" && (sk.starts_with("v_top") || sk.starts_with("v_bottom")),
        build: build_v_spike,
    },
    // Classical double_top / double_bottom → double_pattern kind.
    Rule {
        matches: |fam, sk| {
            fam == "classical"
                && (sk.starts_with("double_top") || sk.starts_with("double_bottom"))
        },
        build: build_double_pattern,
    },
    // Classical H&S and inverse H&S → head_shoulders kind.
    Rule {
        matches: |fam, sk| {
            fam == "classical"
                && (sk.starts_with("head_and_shoulders")
                    || sk.starts_with("inverse_head_and_shoulders"))
        },
        build: build_head_shoulders,
    },
    // Classical diamond → diamond kind.
    Rule {
        matches: |fam, sk| fam == "classical" && sk.starts_with("diamond_"),
        build: build_diamond,
    },
    // Classical two-trendline patterns → two_lines kind. Alternating
    // anchors feed upper / lower trendlines.
    Rule {
        matches: |fam, sk| fam == "classical" && is_two_trendline(sk),
        build: build_two_lines,
    },
    // Classical triple_top / triple_bottom / measured_move → polyline.
    Rule {
        matches: |fam, sk| {
            fam == "classical"
                && (sk.starts_with("triple_top")
                    || sk.starts_with("triple_bottom")
                    || sk.starts_with("measured_move_abcd"))
        },
        build: build_plain_polyline,
    },
    // Harmonic XABCD — polyline + fib labels X,A,B,C,D.
    Rule {
        matches: |fam, _| fam == "harmonic",
        build: build_harmonic,
    },
    // Elliott nascent impulse (4 anchors, wave 3 in progress) — polyline
    // 0-1-2-3 with a projection hint toward the expected wave-5 target.
    Rule {
        matches: |fam, sk| fam == "elliott" && sk.starts_with("impulse_nascent"),
        build: build_elliott_nascent,
    },
    Rule {
        matches: |fam, sk| fam == "elliott" && sk.starts_with("impulse_forming"),
        build: build_elliott_forming,
    },
    // Elliott impulse (5 waves = 6 anchors) → polyline 0-1-2-3-4-5.
    Rule {
        matches: |fam, sk| {
            fam == "elliott"
                && (sk.starts_with("impulse_")
                    || sk.starts_with("leading_diagonal")
                    || sk.starts_with("ending_diagonal"))
        },
        build: build_elliott_impulse,
    },
    // Elliott zigzag / flat → polyline 0-A-B-C.
    Rule {
        matches: |fam, sk| {
            fam == "elliott" && (sk.starts_with("zigzag_") || sk.starts_with("flat_"))
        },
        build: build_elliott_correction,
    },
    // Elliott triangle → two_lines (converging / diverging trendlines).
    Rule {
        matches: |fam, sk| fam == "elliott" && sk.starts_with("triangle_"),
        build: build_two_lines,
    },
    // Elliott combination WXY → polyline 0-W-X-Y.
    Rule {
        matches: |fam, sk| fam == "elliott" && sk.starts_with("combination_wxy"),
        build: build_elliott_combination,
    },
    // wyckoff + range + tbm → no geometry (family has its own overlay
    // path or is a signal, not a visual pattern).
];

// ── Builders ──────────────────────────────────────────────────────────

fn build_candle(subkind: &str, pts: &[Anchor]) -> Option<Value> {
    let last = pts.last()?;
    let direction = if subkind.contains("bear") {
        "bear"
    } else {
        "bull"
    };
    Some(json!({
        "kind": "candle_annotation",
        "payload": {
            "time": last.time,
            "price": last.price,
            "direction": direction,
            "label": short_label(subkind),
        }
    }))
}

fn build_gap(_: &str, pts: &[Anchor]) -> Option<Value> {
    if pts.len() < 2 {
        return None;
    }
    let pre = &pts[0];
    let post = &pts[1];
    Some(json!({
        "kind": "gap_marker",
        "payload": {
            "time": pre.time,
            "time_end": post.time,
            "price_pre": pre.price,
            "price_post": post.price,
        }
    }))
}

fn build_v_spike(_: &str, pts: &[Anchor]) -> Option<Value> {
    if pts.len() < 3 {
        return None;
    }
    Some(json!({
        "kind": "v_spike",
        "payload": {
            "pre":   pt(&pts[0], None),
            "spike": pt(&pts[1], Some("V")),
            "post":  pt(&pts[2], None),
        }
    }))
}

fn build_double_pattern(_: &str, pts: &[Anchor]) -> Option<Value> {
    if pts.len() < 3 {
        return None;
    }
    // Anchors: peak1, trough, peak2 (or bottom1, peak, bottom2).
    let neck_price = pts[1].price.parse::<f64>().ok();
    Some(json!({
        "kind": "double_pattern",
        "payload": {
            "peaks": [pt(&pts[0], Some("1")), pt(&pts[2], Some("2"))],
            "trough": pt(&pts[1], Some("N")),
            "neck": neck_price,
        }
    }))
}

fn build_head_shoulders(_: &str, pts: &[Anchor]) -> Option<Value> {
    // Common layouts: [LS, H, RS] or [LS, neck1, H, neck2, RS].
    let (ls, head, rs, nl, nr) = match pts.len() {
        3 => (&pts[0], &pts[1], &pts[2], None, None),
        5 => (
            &pts[0],
            &pts[2],
            &pts[4],
            Some(&pts[1]),
            Some(&pts[3]),
        ),
        n if n >= 5 => (&pts[0], &pts[n / 2], &pts[n - 1], Some(&pts[1]), Some(&pts[n - 2])),
        _ => return None,
    };
    let mut payload = json!({
        "left_shoulder":  pt(ls, Some("LS")),
        "head":           pt(head, Some("H")),
        "right_shoulder": pt(rs, Some("RS")),
    });
    if let (Some(a), Some(b)) = (nl, nr) {
        payload["neck_left"] = pt(a, None);
        payload["neck_right"] = pt(b, None);
    }
    Some(json!({ "kind": "head_shoulders", "payload": payload }))
}

fn build_diamond(_: &str, pts: &[Anchor]) -> Option<Value> {
    if pts.len() < 4 {
        return None;
    }
    // Pick extremes by price for top/bottom; earliest/latest for left/right.
    let by_price = |lo_hi: bool| -> &Anchor {
        pts.iter()
            .min_by(|a, b| {
                let ap = a.price.parse::<f64>().unwrap_or(0.0);
                let bp = b.price.parse::<f64>().unwrap_or(0.0);
                if lo_hi {
                    ap.partial_cmp(&bp).unwrap()
                } else {
                    bp.partial_cmp(&ap).unwrap()
                }
            })
            .unwrap()
    };
    Some(json!({
        "kind": "diamond",
        "payload": {
            "top":    pt(by_price(false), None),
            "bottom": pt(by_price(true), None),
            "left":   pt(&pts[0], None),
            "right":  pt(pts.last().unwrap(), None),
        }
    }))
}

fn build_two_lines(subkind: &str, pts: &[Anchor]) -> Option<Value> {
    if pts.len() < 4 {
        return None;
    }
    // Rectangles / channels / wedges / triangles / broadenings all have
    // a bimodal price distribution — touches cluster around an upper
    // band and a lower band. Previous implementation paired anchors
    // step-by-2 and picked the higher of each pair as "upper". That
    // only works when the detector emits strictly alternating
    // upper/lower touches. Real detectors emit touches in time order,
    // so two consecutive upper touches (e.g. R1,R1,S1,S1,S2,S2,R2,R2)
    // would mis-classify one as lower, producing the cross-slanted
    // "rectangle" the user reported.
    //
    // Correct split: rank anchors by price; top half → upper band,
    // bottom half → lower band. This works uniformly for every
    // two-trendline family because all of them have a clear bimodal
    // price structure by construction, and it is immune to ordering /
    // duplicate-label / odd-count artefacts from the detector side.
    let prices: Vec<f64> = pts
        .iter()
        .map(|a| a.price.parse::<f64>().unwrap_or(0.0))
        .collect();
    let mut idx: Vec<usize> = (0..pts.len()).collect();
    idx.sort_by(|&i, &j| {
        prices[j]
            .partial_cmp(&prices[i])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let split = pts.len() / 2;
    let mut upper: Vec<Value> = Vec::new();
    let mut lower: Vec<Value> = Vec::new();
    for (rank, &orig) in idx.iter().enumerate() {
        if rank < split {
            upper.push(pt(&pts[orig], None));
        } else {
            lower.push(pt(&pts[orig], None));
        }
    }
    if upper.len() < 2 || lower.len() < 2 {
        return None;
    }
    // Sort each band by time so the resulting polyline traces left-to-
    // right. ISO-8601 lexical order = chronological order.
    let by_time = |a: &Value, b: &Value| {
        a.get("time")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("time").and_then(|v| v.as_str()).unwrap_or(""))
    };
    upper.sort_by(by_time);
    lower.sort_by(by_time);
    // Faz 14 — break-box on Elliott triangles (madde #2). Classical
    // two-line patterns (rectangle/wedge/flag/...) don't carry EW
    // semantics so they skip the break-box; only when the subkind
    // starts with "triangle_" (always Elliott-coded here since the
    // rule table routes classical triangles to the same builder but
    // without the EW prefix) do we project the envelope.
    let is_ew_triangle = subkind.starts_with("triangle_");
    let mut payload = json!({ "upper": upper, "lower": lower });
    if is_ew_triangle {
        if let Some(b) = build_break_box(pts, subkind.contains("bull")) {
            payload["break_box"] = b;
        }
    }
    Some(json!({ "kind": "two_lines", "payload": payload }))
}

/// Faz 14 — LuxAlgo-style forward-projecting invalidation box.
/// Returns `{ time_start, time_end, price_top, price_bot, side }`.
/// `bull=true` means the ABC/triangle is resolved upward; breakdown
/// through `price_bot` invalidates. `bull=false` → the mirror.
fn build_break_box(pts: &[Anchor], bull: bool) -> Option<Value> {
    if pts.len() < 2 {
        return None;
    }
    let first = pts.first()?;
    let last = pts.last()?;
    // Parse ISO-8601 into unix seconds. On failure skip rather than
    // emit a malformed box.
    let t_first = chrono::DateTime::parse_from_rfc3339(&first.time).ok()?;
    let t_last = chrono::DateTime::parse_from_rfc3339(&last.time).ok()?;
    let width = (t_last.timestamp() - t_first.timestamp()).max(60);
    let t_end = t_last.timestamp() + width;
    let prices: Vec<f64> = pts
        .iter()
        .filter_map(|a| a.price.parse::<f64>().ok())
        .collect();
    if prices.len() < 2 {
        return None;
    }
    let last_px: f64 = last.price.parse().ok()?;
    let hi = prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let lo = prices.iter().cloned().fold(f64::INFINITY, f64::min);
    // Bull corrective/triangle → invalidation below `last`; top = last,
    // bottom = lo of interior (A/B/… range). Bear → mirror.
    let (price_top, price_bot, side) = if bull {
        (last_px.max(hi), lo, "bull")
    } else {
        (hi, last_px.min(lo), "bear")
    };
    let t_end_iso = chrono::DateTime::from_timestamp(t_end, 0)?
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    Some(json!({
        "time_start": last.time.clone(),
        "time_end":   t_end_iso,
        "price_top":  price_top.to_string(),
        "price_bot":  price_bot.to_string(),
        "side":       side,
    }))
}

fn build_plain_polyline(_: &str, pts: &[Anchor]) -> Option<Value> {
    if pts.len() < 2 {
        return None;
    }
    Some(json!({
        "kind": "polyline",
        "payload": { "points": pts.iter().map(|a| pt(a, None)).collect::<Vec<_>>() }
    }))
}

fn build_harmonic(_: &str, pts: &[Anchor]) -> Option<Value> {
    if pts.len() < 5 {
        return None;
    }
    // Harmonic XABCD — emit dedicated `harmonic` kind so the frontend
    // can render two filled triangles (X-A-B and B-C-D) plus the
    // skeleton polyline and a dashed X-D chord, matching classical TA
    // charting software. See drawHarmonic in render-kind-registry.ts.
    let labels = ["X", "A", "B", "C", "D"];
    Some(json!({
        "kind": "harmonic",
        "payload": { "xabcd": labelled_points(&pts[..5], &labels) }
    }))
}

/// Faz 15 — nascent impulse: 4 realized anchors (0,1,2,3) + forward
/// projected wave-5 target so the chart signals the setup early.
fn build_elliott_nascent(_subkind: &str, pts: &[Anchor]) -> Option<Value> {
    let labels = ["0", "1", "2", "3"];
    let take = pts.len().min(labels.len());
    if take < 4 {
        return None;
    }
    let mut payload = json!({
        "points": labelled_points(&pts[..take], &labels[..take]),
    });
    // Fib overlay 0↔3 so operators see how deep wave-3 has extended.
    // `bear` flag follows the 0↔3 direction: descending = bear.
    let bear = pts[0].price.parse::<f64>().ok()
        .and_then(|a| pts[3].price.parse::<f64>().ok().map(|b| b < a))
        .unwrap_or(false);
    if let Some(fib) = build_fib_overlay(&pts[..take], bear) {
        payload["fib"] = fib;
    }
    Some(json!({ "kind": "polyline", "payload": payload }))
}

/// Faz 14.A13 — 5-anchor forming impulse (wave 5 in progress).
/// Same polyline treatment as the full impulse, minus the final leg.
fn build_elliott_forming(_subkind: &str, pts: &[Anchor]) -> Option<Value> {
    let labels = ["0", "1", "2", "3", "4"];
    let take = pts.len().min(labels.len());
    if take < 5 {
        return None;
    }
    let mut payload = json!({
        "points": labelled_points(&pts[..take], &labels[..take]),
    });
    let bear = pts[0].price.parse::<f64>().ok()
        .and_then(|a| pts[3].price.parse::<f64>().ok().map(|b| b < a))
        .unwrap_or(false);
    if let Some(fib) = build_fib_overlay(&pts[..take], bear) {
        payload["fib"] = fib;
    }
    Some(json!({ "kind": "polyline", "payload": payload }))
}

fn build_elliott_impulse(subkind: &str, pts: &[Anchor]) -> Option<Value> {
    let labels = ["0", "1", "2", "3", "4", "5"];
    let take = pts.len().min(labels.len());
    if take < 4 {
        return None;
    }
    // Faz 14.A10 — dynamic Fibonacci retracement overlay (madde #3).
    // After a 5-wave impulse (or diagonal) the post-pattern price action
    // typically retraces into a well-known fib zone. We emit the 0↔5
    // range + standard ratios and let the frontend draw dotted
    // horizontals forward. Frontend evaluates break-state against the
    // current candle. Pattern uses the same mechanism `break_box` does:
    // an optional sub-field inside the polyline payload.
    let mut payload = json!({
        "points": labelled_points(&pts[..take], &labels[..take]),
    });
    if let Some(fib) = build_fib_overlay(&pts[..take], subkind.contains("bear")) {
        payload["fib"] = fib;
    }
    Some(json!({ "kind": "polyline", "payload": payload }))
}

/// Faz 14.A10 — LuxAlgo-style fib retracement block carried inside the
/// polyline payload. `base` is wave 0, `target` is the last labelled
/// leg (wave 5 / C / Y). `ratios` are the canonical Elliott retracement
/// levels. Frontend draws them dotted from `target.time` and flips to
/// dashed once a level is crossed (break-state).
fn build_fib_overlay(pts: &[Anchor], bear: bool) -> Option<Value> {
    if pts.len() < 2 {
        return None;
    }
    let base = pts.first()?;
    let target = pts.last()?;
    // Skip degenerate patterns where 0 and 5 landed at the same price —
    // the ratios would collapse into one line and add noise, not value.
    let bp: f64 = base.price.parse().ok()?;
    let tp: f64 = target.price.parse().ok()?;
    if (tp - bp).abs() < f64::EPSILON {
        return None;
    }
    Some(json!({
        "base":   pt(base, Some("0")),
        "target": pt(target, None),
        "ratios": [0.236, 0.382, 0.5, 0.618, 0.786, 1.0],
        "bear":   bear,
    }))
}

fn build_elliott_correction(subkind: &str, pts: &[Anchor]) -> Option<Value> {
    let labels = ["0", "A", "B", "C"];
    let take = pts.len().min(labels.len());
    if take < 4 {
        return None;
    }
    // Faz 14 — LuxAlgo-style break-box (madde #2). Forward-projects the
    // invalidation envelope after (C). If the post-C price action re-
    // enters and crosses through this box, the corrective wave is void
    // → worker flips `state` to invalidated and the setup (if any) is
    // cancelled. Geometry: from C forward by (C - 0) time-width, price
    // band between C and the opposite extreme of the correction body.
    let bb = build_break_box(&pts[..take], subkind.contains("bull"));
    let mut payload = json!({
        "points": labelled_points(&pts[..take], &labels[..take])
    });
    if let Some(b) = bb {
        payload["break_box"] = b;
    }
    Some(json!({ "kind": "polyline", "payload": payload }))
}

fn build_elliott_combination(_: &str, pts: &[Anchor]) -> Option<Value> {
    let labels = ["0", "W", "X", "Y"];
    let take = pts.len().min(labels.len());
    if take < 4 {
        return None;
    }
    Some(json!({
        "kind": "polyline",
        "payload": { "points": labelled_points(&pts[..take], &labels[..take]) }
    }))
}

// ── Helpers ──

fn is_two_trendline(subkind: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "ascending_triangle",
        "descending_triangle",
        "symmetrical_triangle",
        "rectangle",
        "rising_wedge",
        "falling_wedge",
        "ascending_channel",
        "descending_channel",
        "broadening_top",
        "broadening_bottom",
        "broadening_triangle",
    ];
    PREFIXES.iter().any(|p| subkind.starts_with(p))
}

fn short_label(subkind: &str) -> String {
    // Trim the trailing `_bull` / `_bear` / `_neutral` suffix and replace
    // underscores with spaces so the chart label reads naturally.
    let trimmed = subkind
        .trim_end_matches("_bull")
        .trim_end_matches("_bear")
        .trim_end_matches("_neutral");
    trimmed.replace('_', " ").to_uppercase()
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn anchors(rows: &[(&str, &str)]) -> Value {
        Value::Array(
            rows.iter()
                .map(|(t, p)| json!({ "time": t, "price": p }))
                .collect(),
        )
    }

    #[test]
    fn candle_produces_annotation() {
        let a = anchors(&[("2024-01-01T00:00:00Z", "100"), ("2024-01-01T00:15:00Z", "101")]);
        let g = derive("candle", "hammer_bull", &a).unwrap();
        assert_eq!(g["kind"], "candle_annotation");
        assert_eq!(g["payload"]["direction"], "bull");
    }

    #[test]
    fn gap_produces_marker() {
        let a = anchors(&[("2024-01-01T00:00:00Z", "100"), ("2024-01-01T00:01:00Z", "105")]);
        let g = derive("gap", "breakaway_gap_bull", &a).unwrap();
        assert_eq!(g["kind"], "gap_marker");
    }

    #[test]
    fn harmonic_has_xabcd_labels() {
        let a = anchors(&[
            ("2024-01-01T00:00:00Z", "100"),
            ("2024-01-02T00:00:00Z", "110"),
            ("2024-01-03T00:00:00Z", "105"),
            ("2024-01-04T00:00:00Z", "115"),
            ("2024-01-05T00:00:00Z", "95"),
        ]);
        let g = derive("harmonic", "gartley_bull", &a).unwrap();
        assert_eq!(g["kind"], "harmonic");
        let pts = g["payload"]["xabcd"].as_array().unwrap();
        assert_eq!(pts[0]["label"], "X");
        assert_eq!(pts[4]["label"], "D");
    }

    #[test]
    fn classical_rectangle_produces_two_lines() {
        let a = anchors(&[
            ("2024-01-01T00:00:00Z", "110"),
            ("2024-01-02T00:00:00Z", "100"),
            ("2024-01-03T00:00:00Z", "110"),
            ("2024-01-04T00:00:00Z", "100"),
        ]);
        let g = derive("classical", "rectangle_bull", &a).unwrap();
        assert_eq!(g["kind"], "two_lines");
        assert_eq!(g["payload"]["upper"].as_array().unwrap().len(), 2);
        assert_eq!(g["payload"]["lower"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn wyckoff_returns_none() {
        let a = anchors(&[("2024-01-01T00:00:00Z", "100")]);
        assert!(derive("wyckoff", "spring_bull", &a).is_none());
    }

    #[test]
    fn v_spike_three_anchors() {
        let a = anchors(&[
            ("2024-01-01T00:00:00Z", "110"),
            ("2024-01-02T00:00:00Z", "90"),
            ("2024-01-03T00:00:00Z", "112"),
        ]);
        let g = derive("classical", "v_bottom_bull", &a).unwrap();
        assert_eq!(g["kind"], "v_spike");
        assert_eq!(g["payload"]["spike"]["label"], "V");
    }
}

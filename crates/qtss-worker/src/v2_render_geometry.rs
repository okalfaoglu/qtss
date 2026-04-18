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

fn build_two_lines(_: &str, pts: &[Anchor]) -> Option<Value> {
    if pts.len() < 4 {
        return None;
    }
    // Split alternating anchors into upper / lower. Within each pair of
    // adjacent anchors, the higher price is upper. For 4 pivots this
    // produces the canonical two-trendline form used by wedges,
    // channels, rectangles, triangles, broadenings.
    let mut upper: Vec<Value> = Vec::new();
    let mut lower: Vec<Value> = Vec::new();
    for i in (0..pts.len()).step_by(2) {
        if i + 1 >= pts.len() {
            break;
        }
        let a = &pts[i];
        let b = &pts[i + 1];
        let ap = a.price.parse::<f64>().unwrap_or(0.0);
        let bp = b.price.parse::<f64>().unwrap_or(0.0);
        if ap >= bp {
            upper.push(pt(a, None));
            lower.push(pt(b, None));
        } else {
            upper.push(pt(b, None));
            lower.push(pt(a, None));
        }
    }
    if upper.len() < 2 || lower.len() < 2 {
        return None;
    }
    Some(json!({
        "kind": "two_lines",
        "payload": { "upper": upper, "lower": lower }
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
    let labels = ["X", "A", "B", "C", "D"];
    Some(json!({
        "kind": "polyline",
        "payload": { "points": labelled_points(&pts[..5], &labels) }
    }))
}

fn build_elliott_impulse(_: &str, pts: &[Anchor]) -> Option<Value> {
    let labels = ["0", "1", "2", "3", "4", "5"];
    let take = pts.len().min(labels.len());
    if take < 4 {
        return None;
    }
    Some(json!({
        "kind": "polyline",
        "payload": { "points": labelled_points(&pts[..take], &labels[..take]) }
    }))
}

fn build_elliott_correction(_: &str, pts: &[Anchor]) -> Option<Value> {
    let labels = ["0", "A", "B", "C"];
    let take = pts.len().min(labels.len());
    if take < 4 {
        return None;
    }
    Some(json!({
        "kind": "polyline",
        "payload": { "points": labelled_points(&pts[..take], &labels[..take]) }
    }))
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
        assert_eq!(g["kind"], "polyline");
        let pts = g["payload"]["points"].as_array().unwrap();
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

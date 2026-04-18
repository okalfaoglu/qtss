//! Faz 9.8.AI-Yol2 — shared helpers for structural feature extractors.
//!
//! Pattern-agnostic utilities over the detection `raw_detection` JSON
//! envelope the orchestrator emits (`v2_detection_orchestrator.rs`).
//! Keeps per-family sources tight and uniform — no duplicated anchor
//! parsing or direction inference scattered across modules
//! (CLAUDE.md #1).

use serde_json::Value;

/// `+1` bull suffix, `-1` bear suffix, `0` otherwise.
pub fn direction_from_subkind(subkind: &str) -> f64 {
    if subkind.ends_with("_bull") || subkind.contains("_bull_") {
        1.0
    } else if subkind.ends_with("_bear") || subkind.contains("_bear_") {
        -1.0
    } else {
        0.0
    }
}

/// Data-driven subkind → ordinal dispatch. Returns `default_ord` if
/// none of the needles match. Callers keep the table small and data-only
/// (CLAUDE.md #1: dispatch table, not if/else).
pub fn subkind_ordinal(subkind: &str, table: &[(&str, f64)], default_ord: f64) -> f64 {
    table
        .iter()
        .find(|(needle, _)| subkind.contains(needle))
        .map(|(_, ord)| *ord)
        .unwrap_or(default_ord)
}

/// Summary geometry from the detection's `anchors` array.
///
/// Returns `(price_range_pct, span_bars, anchors_count)` where
/// `price_range_pct = (max − min)/min`. `None` when anchors are
/// missing/empty, prices unparseable, or min-price ≤ 0.
pub fn anchors_geometry(anchors: &Value) -> Option<(f64, i64, i64)> {
    let arr = anchors.as_array()?;
    if arr.is_empty() {
        return None;
    }
    let mut min_price = f64::INFINITY;
    let mut max_price = f64::NEG_INFINITY;
    let mut min_idx = i64::MAX;
    let mut max_idx = i64::MIN;
    let mut idx_seen = false;
    for a in arr {
        let price = a.get("price").and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| v.as_f64())
        });
        if let Some(p) = price {
            if p < min_price {
                min_price = p;
            }
            if p > max_price {
                max_price = p;
            }
        }
        if let Some(idx) = a.get("bar_index").and_then(|v| v.as_i64()) {
            idx_seen = true;
            if idx < min_idx {
                min_idx = idx;
            }
            if idx > max_idx {
                max_idx = idx;
            }
        }
    }
    if !min_price.is_finite() || !max_price.is_finite() || min_price <= 0.0 {
        return None;
    }
    let span = if idx_seen && max_idx > min_idx {
        max_idx - min_idx
    } else {
        0
    };
    let range_pct = (max_price - min_price) / min_price;
    Some((range_pct, span, arr.len() as i64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn direction_suffixes() {
        assert_eq!(direction_from_subkind("impulse_5_bull"), 1.0);
        assert_eq!(direction_from_subkind("flat_regular_bear"), -1.0);
        assert_eq!(direction_from_subkind("range_regime"), 0.0);
    }

    #[test]
    fn ordinal_table_lookup() {
        let table = &[("wedge", 2.0), ("triangle", 1.0)];
        assert_eq!(subkind_ordinal("rising_wedge_bear", table, 9.0), 2.0);
        assert_eq!(subkind_ordinal("symmetrical_triangle_neutral", table, 9.0), 1.0);
        assert_eq!(subkind_ordinal("unknown_shape", table, 9.0), 9.0);
    }

    #[test]
    fn geometry_parses_strings() {
        let a = json!([
            {"bar_index": 100, "price": "100.0"},
            {"bar_index": 140, "price": "150.0"}
        ]);
        let (pct, span, n) = anchors_geometry(&a).unwrap();
        assert!((pct - 0.5).abs() < 1e-9);
        assert_eq!(span, 40);
        assert_eq!(n, 2);
    }

    #[test]
    fn geometry_none_on_empty() {
        assert!(anchors_geometry(&json!([])).is_none());
        assert!(anchors_geometry(&json!(null)).is_none());
    }
}

//! Multi-level pivot tree — the central zigzag/pivot source for all
//! pattern detectors (Elliott, harmonic, classical, Wyckoff, range).
//!
//! See `docs/QTSS_V2_ARCHITECTURE_PLAN.md` §4 ("Merkezi Pivot Motoru").
//!
//! Levels are nested: every L3 pivot is also present in L2, every L2 in L1,
//! every L1 in L0. The `PivotTree` exposes that invariant via `at_level`.
//!
//! This module defines only the data shape. The detection algorithm
//! lives in the future `qtss-pivots` crate.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PivotKind {
    High,
    Low,
}

/// Swing classification relative to the previous same-kind pivot.
/// Derived after pivot detection; `None` for the first pivot of its kind.
///
/// Maps to PineScript's `dir = ±2` concept:
/// - `HH` / `LL` = trend confirmation (dir=±2)
/// - `LH` / `HL` = potential reversal / CHoCH
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwingType {
    /// Higher High — bullish continuation.
    HH,
    /// Lower High — potential bearish reversal (CHoCH if after HH sequence).
    LH,
    /// Higher Low — bullish continuation.
    HL,
    /// Lower Low — bearish continuation.
    LL,
}

/// Pivot detail level. Each level uses progressively coarser ATR thresholds:
/// L0 = tick / micro, L3 = macro (Elliott main wave / Wyckoff phase).
///
/// Detectors declare which level they consume via
/// `PatternDetector::required_pivot_level`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PivotLevel {
    L0,
    L1,
    L2,
    L3,
}

impl PivotLevel {
    pub const ALL: [PivotLevel; 4] = [PivotLevel::L0, PivotLevel::L1, PivotLevel::L2, PivotLevel::L3];

    pub fn as_index(self) -> usize {
        match self {
            PivotLevel::L0 => 0,
            PivotLevel::L1 => 1,
            PivotLevel::L2 => 2,
            PivotLevel::L3 => 3,
        }
    }

    /// Canonical DB/JSON string form. Matches migration 0192's
    /// `qtss_v2_detections.pivot_level` CHECK ('L0'..'L3') and the
    /// harmonic backtest sweep's level filter.
    pub fn as_str(self) -> &'static str {
        match self {
            PivotLevel::L0 => "L0",
            PivotLevel::L1 => "L1",
            PivotLevel::L2 => "L2",
            PivotLevel::L3 => "L3",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pivot {
    /// Bar index in the source series.
    pub bar_index: u64,
    pub time: DateTime<Utc>,
    pub price: Decimal,
    pub kind: PivotKind,
    pub level: PivotLevel,
    /// Distance to neighbors (used by validators / target engine).
    pub prominence: Decimal,
    pub volume_at_pivot: Decimal,
    /// HH/HL/LH/LL classification. `None` for first pivot of its kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swing_type: Option<SwingType>,
}

/// Immutable snapshot of all pivot levels for a bar series.
///
/// Invariant: `levels[i]` for higher `i` is a subset of `levels[i-1]`.
/// The `qtss-pivots` builder enforces this; consumers may rely on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PivotTree {
    levels: [Vec<Pivot>; 4],
}

impl PivotTree {
    pub fn new(l0: Vec<Pivot>, l1: Vec<Pivot>, l2: Vec<Pivot>, l3: Vec<Pivot>) -> Self {
        Self { levels: [l0, l1, l2, l3] }
    }

    pub fn empty() -> Self {
        Self {
            levels: [vec![], vec![], vec![], vec![]],
        }
    }

    pub fn at_level(&self, level: PivotLevel) -> &[Pivot] {
        &self.levels[level.as_index()]
    }

    pub fn last(&self, level: PivotLevel) -> Option<&Pivot> {
        self.at_level(level).last()
    }

    pub fn count(&self, level: PivotLevel) -> usize {
        self.at_level(level).len()
    }

    /// Detect Change of Character (CHoCH) at the given level.
    ///
    /// CHoCH = first LH after a sequence of HH (bearish CHoCH)
    ///       = first HL after a sequence of LL (bullish CHoCH)
    ///
    /// Returns `Some((pivot_index, SwingType))` of the CHoCH pivot, or None.
    pub fn detect_choch(&self, level: PivotLevel) -> Option<(usize, SwingType)> {
        let pivots = self.at_level(level);
        if pivots.len() < 4 {
            return None;
        }
        // Walk backwards looking for the most recent trend break.
        for i in (1..pivots.len()).rev() {
            let curr = &pivots[i];
            let Some(st) = curr.swing_type else { continue };
            match st {
                // Bearish CHoCH: LH after HH sequence.
                SwingType::LH => {
                    // Check if previous High was HH.
                    if let Some(prev_high) = pivots[..i].iter().rev().find(|p| p.kind == PivotKind::High) {
                        if prev_high.swing_type == Some(SwingType::HH) {
                            return Some((i, SwingType::LH));
                        }
                    }
                }
                // Bullish CHoCH: HL after LL sequence.
                SwingType::HL => {
                    if let Some(prev_low) = pivots[..i].iter().rev().find(|p| p.kind == PivotKind::Low) {
                        if prev_low.swing_type == Some(SwingType::LL) {
                            return Some((i, SwingType::HL));
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Detect Break of Structure (BOS) at the given level.
    ///
    /// BOS = HH continuing an existing uptrend (bullish BOS)
    ///     = LL continuing an existing downtrend (bearish BOS)
    ///
    /// Returns the most recent BOS pivot, or None.
    pub fn detect_bos(&self, level: PivotLevel) -> Option<(usize, SwingType)> {
        let pivots = self.at_level(level);
        for i in (1..pivots.len()).rev() {
            let curr = &pivots[i];
            let Some(st) = curr.swing_type else { continue };
            match st {
                SwingType::HH => {
                    if let Some(prev_high) = pivots[..i].iter().rev().find(|p| p.kind == PivotKind::High) {
                        if prev_high.swing_type == Some(SwingType::HH) {
                            return Some((i, SwingType::HH));
                        }
                    }
                }
                SwingType::LL => {
                    if let Some(prev_low) = pivots[..i].iter().rev().find(|p| p.kind == PivotKind::Low) {
                        if prev_low.swing_type == Some(SwingType::LL) {
                            return Some((i, SwingType::LL));
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Validate the subset invariant. Returns the first level pair that
    /// violates it, or `None` if everything is consistent.
    pub fn check_subset_invariant(&self) -> Option<(PivotLevel, PivotLevel)> {
        // Walk adjacent level pairs once, no nested branching per level.
        let pairs = [
            (PivotLevel::L0, PivotLevel::L1),
            (PivotLevel::L1, PivotLevel::L2),
            (PivotLevel::L2, PivotLevel::L3),
        ];
        pairs.into_iter().find(|(lo, hi)| {
            let lo_keys: std::collections::HashSet<u64> =
                self.at_level(*lo).iter().map(|p| p.bar_index).collect();
            self.at_level(*hi)
                .iter()
                .any(|p| !lo_keys.contains(&p.bar_index))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn p(idx: u64, level: PivotLevel) -> Pivot {
        Pivot {
            bar_index: idx,
            time: Utc::now(),
            price: dec!(100),
            kind: PivotKind::High,
            level,
            prominence: dec!(1),
            volume_at_pivot: dec!(10),
            swing_type: None,
        }
    }

    #[test]
    fn level_ordering_is_total() {
        assert!(PivotLevel::L0 < PivotLevel::L1);
        assert!(PivotLevel::L2 < PivotLevel::L3);
    }

    #[test]
    fn subset_invariant_holds_for_well_formed_tree() {
        let tree = PivotTree::new(
            vec![p(1, PivotLevel::L0), p(2, PivotLevel::L0), p(3, PivotLevel::L0), p(4, PivotLevel::L0)],
            vec![p(2, PivotLevel::L1), p(4, PivotLevel::L1)],
            vec![p(4, PivotLevel::L2)],
            vec![],
        );
        assert!(tree.check_subset_invariant().is_none());
    }

    #[test]
    fn subset_invariant_detects_violation() {
        // L1 contains bar 5 which is missing from L0.
        let tree = PivotTree::new(
            vec![p(1, PivotLevel::L0), p(2, PivotLevel::L0)],
            vec![p(5, PivotLevel::L1)],
            vec![],
            vec![],
        );
        assert_eq!(
            tree.check_subset_invariant(),
            Some((PivotLevel::L0, PivotLevel::L1))
        );
    }
}

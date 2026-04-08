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

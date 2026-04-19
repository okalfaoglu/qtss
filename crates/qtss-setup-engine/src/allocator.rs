//! Allocator — cross-setup gate. Decides whether a candidate setup
//! can be armed given the current open book. Three independent caps,
//! evaluated in **fail-fast order** (CLAUDE.md #1 — early return):
//!
//! 1. `max_total_open_risk_pct` — sum of `risk_pct` across all open
//!    setups in a venue class must stay under the cap.
//! 2. `max_concurrent_per_profile` — per-profile open count.
//! 3. Correlation group cap — at most N open setups per group,
//!    optionally only counting setups in the same direction.
//!
//! Pure function: no DB, no I/O. The worker hydrates `AllocatorContext`
//! from storage once per tick and calls `check_allocation` per
//! candidate.

use std::collections::HashMap;

use crate::types::{Direction, Profile, RejectReason};

/// Hard caps loaded from `system_config`. Populated by the worker;
/// no defaults baked into code (CLAUDE.md #2).
#[derive(Debug, Clone)]
pub struct AllocatorLimits {
    pub max_total_open_risk_pct: f64,
    pub max_concurrent_per_profile: HashMap<Profile, u32>,
    pub correlation_max_per_group: u32,
    pub correlation_same_direction_only: bool,
}

/// One row in the allocator's view of the open book.
#[derive(Debug, Clone)]
pub struct OpenSetupSummary {
    pub profile: Profile,
    pub direction: Direction,
    pub risk_pct: f64,
    /// Correlation group keys this setup's symbol belongs to.
    pub correlation_groups: Vec<String>,
    /// Faz 9C — runtime mode (`live` | `dry` | `backtest`). Caps are
    /// mode-scoped: a backtest setup never crowds out a live candidate
    /// and vice versa. Without this field, live setups saturating the
    /// per-profile slots starved the historical backfill pipeline
    /// (453k backtest detections → 0 backtest setups).
    pub mode: String,
}

/// Aggregated open-book context for one decision call. Filtered to a
/// single venue class by the worker before being passed in.
#[derive(Debug, Clone, Default)]
pub struct AllocatorContext {
    pub open_setups: Vec<OpenSetupSummary>,
}

/// Decide whether the candidate setup is allowed. `Ok(())` means
/// arm-it; `Err(RejectReason)` means refuse and record a rejection
/// row.
pub fn check_allocation(
    limits: &AllocatorLimits,
    ctx: &AllocatorContext,
    candidate: &OpenSetupSummary,
) -> Result<(), RejectReason> {
    // Faz 9C — all three caps are mode-scoped. Live setups saturating
    // the per-profile slots must not starve backtest candidates (and
    // vice versa). Filter the open book down to the candidate's own
    // mode before applying any guard.
    let same_mode = |s: &&OpenSetupSummary| s.mode == candidate.mode;

    // Guard 1 — total open risk
    let current_total: f64 = ctx
        .open_setups
        .iter()
        .filter(same_mode)
        .map(|s| s.risk_pct)
        .sum();
    if current_total + candidate.risk_pct > limits.max_total_open_risk_pct {
        return Err(RejectReason::TotalRiskCap);
    }

    // Guard 2 — per-profile concurrent count
    let profile_count = ctx
        .open_setups
        .iter()
        .filter(same_mode)
        .filter(|s| s.profile == candidate.profile)
        .count() as u32;
    let max_for_profile = limits
        .max_concurrent_per_profile
        .get(&candidate.profile)
        .copied()
        .unwrap_or(0);
    if profile_count + 1 > max_for_profile {
        return Err(RejectReason::MaxConcurrent);
    }

    // Guard 3 — correlation cap (per group)
    for group in &candidate.correlation_groups {
        let group_count = ctx
            .open_setups
            .iter()
            .filter(same_mode)
            .filter(|s| s.correlation_groups.iter().any(|g| g == group))
            .filter(|s| {
                !limits.correlation_same_direction_only || s.direction == candidate.direction
            })
            .count() as u32;
        if group_count + 1 > limits.correlation_max_per_group {
            return Err(RejectReason::CorrelationCap);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> AllocatorLimits {
        let mut m = HashMap::new();
        m.insert(Profile::T, 4);
        m.insert(Profile::Q, 3);
        m.insert(Profile::D, 2);
        AllocatorLimits {
            max_total_open_risk_pct: 6.0,
            max_concurrent_per_profile: m,
            correlation_max_per_group: 2,
            correlation_same_direction_only: true,
        }
    }

    fn s(profile: Profile, dir: Direction, risk: f64, groups: &[&str]) -> OpenSetupSummary {
        OpenSetupSummary {
            profile,
            direction: dir,
            risk_pct: risk,
            correlation_groups: groups.iter().map(|g| g.to_string()).collect(),
            mode: "live".to_string(),
        }
    }

    fn s_mode(
        profile: Profile,
        dir: Direction,
        risk: f64,
        groups: &[&str],
        mode: &str,
    ) -> OpenSetupSummary {
        let mut row = s(profile, dir, risk, groups);
        row.mode = mode.to_string();
        row
    }

    #[test]
    fn empty_book_allows_anything_within_caps() {
        let ctx = AllocatorContext::default();
        let cand = s(Profile::Q, Direction::Long, 0.5, &["btc_family"]);
        assert!(check_allocation(&limits(), &ctx, &cand).is_ok());
    }

    #[test]
    fn total_risk_cap_blocks() {
        let ctx = AllocatorContext {
            open_setups: vec![
                s(Profile::D, Direction::Long, 3.0, &[]),
                s(Profile::Q, Direction::Long, 2.5, &[]),
            ],
        };
        // 3.0 + 2.5 + 1.0 = 6.5 > 6.0
        let cand = s(Profile::T, Direction::Long, 1.0, &[]);
        assert_eq!(
            check_allocation(&limits(), &ctx, &cand),
            Err(RejectReason::TotalRiskCap)
        );
    }

    #[test]
    fn max_concurrent_per_profile_blocks() {
        let ctx = AllocatorContext {
            open_setups: vec![
                s(Profile::D, Direction::Long, 0.1, &[]),
                s(Profile::D, Direction::Short, 0.1, &[]),
            ],
        };
        let cand = s(Profile::D, Direction::Long, 0.1, &[]);
        assert_eq!(
            check_allocation(&limits(), &ctx, &cand),
            Err(RejectReason::MaxConcurrent)
        );
    }

    #[test]
    fn correlation_same_direction_blocks() {
        let ctx = AllocatorContext {
            open_setups: vec![
                s(Profile::Q, Direction::Long, 0.5, &["btc_family"]),
                s(Profile::T, Direction::Long, 0.25, &["btc_family"]),
            ],
        };
        let cand = s(Profile::D, Direction::Long, 1.0, &["btc_family"]);
        assert_eq!(
            check_allocation(&limits(), &ctx, &cand),
            Err(RejectReason::CorrelationCap)
        );
    }

    #[test]
    fn correlation_opposite_direction_allowed_when_same_only() {
        let ctx = AllocatorContext {
            open_setups: vec![
                s(Profile::Q, Direction::Long, 0.5, &["btc_family"]),
                s(Profile::T, Direction::Long, 0.25, &["btc_family"]),
            ],
        };
        // candidate is SHORT — same_direction_only=true → not counted
        let cand = s(Profile::D, Direction::Short, 1.0, &["btc_family"]);
        assert!(check_allocation(&limits(), &ctx, &cand).is_ok());
    }

    #[test]
    fn mode_scoping_isolates_caps() {
        // Live profile D saturated at cap=2; a backtest candidate in
        // the same profile must still be allowed — caps are per-mode.
        let ctx = AllocatorContext {
            open_setups: vec![
                s_mode(Profile::D, Direction::Long, 0.1, &[], "live"),
                s_mode(Profile::D, Direction::Short, 0.1, &[], "live"),
            ],
        };
        let cand = s_mode(Profile::D, Direction::Long, 0.1, &[], "backtest");
        assert!(check_allocation(&limits(), &ctx, &cand).is_ok());
        // Same candidate in live mode is still rejected.
        let cand_live = s_mode(Profile::D, Direction::Long, 0.1, &[], "live");
        assert_eq!(
            check_allocation(&limits(), &ctx, &cand_live),
            Err(RejectReason::MaxConcurrent)
        );
    }

    #[test]
    fn fail_fast_order_total_risk_first() {
        // Even though concurrent and correlation also fail, total
        // risk is checked first.
        let ctx = AllocatorContext {
            open_setups: vec![
                s(Profile::D, Direction::Long, 3.0, &["btc_family"]),
                s(Profile::D, Direction::Long, 3.0, &["btc_family"]),
            ],
        };
        let cand = s(Profile::D, Direction::Long, 1.0, &["btc_family"]);
        assert_eq!(
            check_allocation(&limits(), &ctx, &cand),
            Err(RejectReason::TotalRiskCap)
        );
    }
}

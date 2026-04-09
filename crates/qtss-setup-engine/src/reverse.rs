//! Reverse-signal evaluator. Closes an active setup early when the
//! latest `ConfluenceReading` flips direction with enough `guven`.
//!
//! The threshold is per-profile (T/Q/D) and lives in `system_config`
//! — nothing is hardcoded here (CLAUDE.md #2). The worker resolves
//! the threshold and passes it in.
//!
//! Sensitivity ordering: **Q most sensitive (lowest threshold)**,
//! then T, then D. The function itself doesn't enforce the ordering
//! — that's the worker's job; it just compares against the value
//! it was given.

use qtss_confluence::ConfluenceReading;

use crate::types::{Direction, Profile};

/// Returns `true` iff the setup should be force-closed due to a
/// reverse signal. Pure function — no DB, no side effects.
///
/// Rules:
/// 1. Setup direction must not be `Neutral` (nothing to reverse).
/// 2. Latest confluence direction must be the *opposite* of the
///    setup direction (a same-side or neutral confluence is not a
///    reverse signal).
/// 3. Confluence `guven` must reach the per-profile threshold.
pub fn should_reverse_close(
    setup_direction: Direction,
    _profile: Profile,
    latest_confluence: &ConfluenceReading,
    threshold: f64,
) -> bool {
    let opposite = match setup_direction {
        Direction::Long => Direction::Short,
        Direction::Short => Direction::Long,
        Direction::Neutral => return false,
    };
    if latest_confluence.direction != opposite {
        return false;
    }
    latest_confluence.guven >= threshold
}

#[cfg(test)]
mod tests {
    use super::*;
    use qtss_confluence::ConfluenceReading;

    fn reading(direction: Direction, guven: f64) -> ConfluenceReading {
        ConfluenceReading {
            erken_uyari: 0.0,
            guven,
            direction,
            layer_count: 5,
            details: vec![],
        }
    }

    #[test]
    fn long_setup_force_closes_on_strong_short_confluence() {
        let r = reading(Direction::Short, 0.60);
        assert!(should_reverse_close(Direction::Long, Profile::Q, &r, 0.55));
    }

    #[test]
    fn weak_opposite_does_not_close() {
        let r = reading(Direction::Short, 0.50);
        assert!(!should_reverse_close(Direction::Long, Profile::Q, &r, 0.55));
    }

    #[test]
    fn same_direction_never_closes() {
        let r = reading(Direction::Long, 0.99);
        assert!(!should_reverse_close(Direction::Long, Profile::Q, &r, 0.55));
    }

    #[test]
    fn neutral_confluence_does_not_close() {
        let r = reading(Direction::Neutral, 0.99);
        assert!(!should_reverse_close(Direction::Long, Profile::Q, &r, 0.55));
    }

    #[test]
    fn neutral_setup_is_immune() {
        let r = reading(Direction::Long, 0.99);
        assert!(!should_reverse_close(Direction::Neutral, Profile::Q, &r, 0.55));
    }

    #[test]
    fn d_profile_higher_threshold_filters_marginal_signals() {
        let r = reading(Direction::Short, 0.65);
        // T threshold (0.65) → trips
        assert!(should_reverse_close(Direction::Long, Profile::T, &r, 0.65));
        // D threshold (0.70) → does not
        assert!(!should_reverse_close(Direction::Long, Profile::D, &r, 0.70));
    }
}

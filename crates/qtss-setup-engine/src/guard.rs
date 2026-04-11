//! PositionGuard — per-setup entry/stop/target bookkeeping with a
//! monotonic ratchet trailing stop (`koruma`).
//!
//! Invariant: `koruma` only ever moves *toward* price (tighter).
//! It never loosens, even if the market pulls back. That guarantee
//! lets the Setup Engine emit a single `active_sl()` that both the
//! reporting layer and the execution adapter can trust.
//!
//! Ratchet formula (single expression, CLAUDE.md #1):
//!
//! ```text
//! long :  new_koruma = entry + (floor(unrealized_R) - 1) * R
//! short:  new_koruma = entry - (floor(unrealized_R) - 1) * R
//! ```
//!
//! where `R = |entry - entry_sl|`. The new value is only committed
//! if it is strictly tighter than the current `koruma`.
//!
//! Worked example (long, entry=100, entry_sl=98, R=2):
//! - price=101 → unrealized_R=0.5, floor=0 → new=98 (no change, looser)
//! - price=102 → unrealized_R=1.0, floor=1 → new=100 (BE) ✓
//! - price=104 → unrealized_R=2.0, floor=2 → new=102 (entry+R) ✓
//! - price=103 → unrealized_R=1.5, floor=1 → new=100 (older 102 wins, no loosen) ✓

use crate::types::Direction;

/// Per-profile knobs for the guard. Loaded from `system_config` by
/// the worker — nothing is hardcoded here.
#[derive(Debug, Clone, Copy)]
pub struct PositionGuardConfig {
    /// Initial stop distance in ATR multiples from entry.
    pub entry_sl_atr_mult: f64,
    /// Minimum time between ratchet tightenings (seconds). Prevents
    /// thrash on noisy fills.
    pub ratchet_interval_secs: i64,
    /// Target distance from entry in R multiples.
    pub target_ref_r: f64,
    /// Per-setup risk as percent of account equity.
    pub risk_pct: f64,
    /// Cap on concurrent setups for this profile.
    pub max_concurrent: u32,
    /// `guven` threshold above which a reverse signal force-closes
    /// the setup.
    pub reverse_guven_threshold: f64,
}

/// Structural target from a detection (measured move, fib extension, etc.).
#[derive(Debug, Clone, Copy)]
pub struct StructuralTarget {
    pub price: f64,
    pub weight: f64,
    pub label: &'static str,
}

/// Live state for a single setup. Owned by the engine; mutated in
/// place on each tick.
#[derive(Debug, Clone, Copy)]
pub struct PositionGuard {
    pub entry: f64,
    pub entry_sl: f64,
    /// Ratchet trailing stop — the only stop that actually moves.
    pub koruma: f64,
    pub target_ref: f64,
    /// Secondary target (e.g., 1.618× measured move).
    pub target_ref2: Option<f64>,
    pub direction: Direction,
    /// Whether entry/sl/tp came from structural detection vs ATR fallback.
    pub structural: bool,
}

impl PositionGuard {
    /// Construct a fresh guard from entry, ATR, profile config and
    /// direction. `koruma` starts at `entry_sl` (no ratchet yet).
    /// This is the **ATR fallback** — used when no structural detection
    /// provides invalidation/targets.
    pub fn new(entry: f64, atr: f64, cfg: &PositionGuardConfig, direction: Direction) -> Self {
        let stop_distance = atr * cfg.entry_sl_atr_mult;
        let sign = direction.sign();
        let entry_sl = entry - sign * stop_distance;
        let target_ref = entry + sign * stop_distance * cfg.target_ref_r;
        Self {
            entry,
            entry_sl,
            koruma: entry_sl,
            target_ref,
            target_ref2: None,
            direction,
            structural: false,
        }
    }

    /// Construct from **structural detection** data — invalidation
    /// price becomes SL, measured move / fib targets become TP.
    /// Falls back to ATR-based values if structural data is missing.
    pub fn new_structural(
        entry: f64,
        invalidation_price: f64,
        targets: &[StructuralTarget],
        atr: f64,
        cfg: &PositionGuardConfig,
        direction: Direction,
    ) -> Self {
        let sign = direction.sign();

        // SL = invalidation price (where the pattern breaks).
        let entry_sl = invalidation_price;

        // Validate SL is on the correct side of entry.
        let sl_valid = match direction {
            Direction::Long => entry_sl < entry,
            Direction::Short => entry_sl > entry,
            Direction::Neutral => false,
        };

        if !sl_valid || targets.is_empty() {
            // Fall back to ATR-based guard.
            return Self::new(entry, atr, cfg, direction);
        }

        // Sort targets by weight descending, pick best two.
        let mut sorted: Vec<StructuralTarget> = targets.to_vec();
        sorted.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));

        let target_ref = sorted[0].price;
        let target_ref2 = sorted.get(1).map(|t| t.price);

        // Validate target is on the correct side of entry.
        let tp_valid = match direction {
            Direction::Long => target_ref > entry,
            Direction::Short => target_ref < entry,
            Direction::Neutral => false,
        };

        if !tp_valid {
            return Self::new(entry, atr, cfg, direction);
        }

        Self {
            entry,
            entry_sl,
            koruma: entry_sl,
            target_ref,
            target_ref2,
            direction,
            structural: true,
        }
    }

    /// Effective stop right now. For a long it is the *higher* of
    /// `entry_sl` and `koruma`; for a short, the *lower*.
    pub fn active_sl(&self) -> f64 {
        match self.direction {
            Direction::Long => self.entry_sl.max(self.koruma),
            Direction::Short => self.entry_sl.min(self.koruma),
            Direction::Neutral => self.entry_sl,
        }
    }

    /// `R` — absolute risk unit = distance from entry to initial stop.
    pub fn r_value(&self) -> f64 {
        (self.entry - self.entry_sl).abs()
    }

    /// Unrealised gain expressed in R multiples. Sign flips with
    /// direction so a winning short is positive.
    pub fn unrealized_r(&self, price: f64) -> f64 {
        let r = self.r_value();
        if r == 0.0 {
            return 0.0;
        }
        ((price - self.entry) * self.direction.sign()) / r
    }

    /// Attempt to tighten `koruma` using the ratchet formula above.
    /// Returns `true` iff `koruma` was actually updated.
    pub fn try_ratchet(&mut self, price: f64) -> bool {
        if matches!(self.direction, Direction::Neutral) {
            return false;
        }
        let unrealized = self.unrealized_r(price).floor();
        if unrealized < 1.0 {
            return false; // need at least +1R before we move anything
        }
        let r = self.r_value();
        let sign = self.direction.sign();
        // First step (unrealized=1): koruma = entry (BE)
        // Second step (unrealized=2): koruma = entry + 1R   (long)
        // ...
        let candidate = self.entry + sign * (unrealized - 1.0) * r;
        let tighter = match self.direction {
            Direction::Long => candidate > self.koruma,
            Direction::Short => candidate < self.koruma,
            Direction::Neutral => false,
        };
        if tighter {
            self.koruma = candidate;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> PositionGuardConfig {
        PositionGuardConfig {
            entry_sl_atr_mult: 1.0,
            ratchet_interval_secs: 60,
            target_ref_r: 2.0,
            risk_pct: 0.5,
            max_concurrent: 3,
            reverse_guven_threshold: 0.55,
        }
    }

    #[test]
    fn long_construction() {
        let g = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Long);
        assert_eq!(g.entry, 100.0);
        assert_eq!(g.entry_sl, 98.0);
        assert_eq!(g.koruma, 98.0);
        assert_eq!(g.target_ref, 104.0);
        assert_eq!(g.r_value(), 2.0);
        assert_eq!(g.active_sl(), 98.0);
    }

    #[test]
    fn short_construction_inverts() {
        let g = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Short);
        assert_eq!(g.entry_sl, 102.0);
        assert_eq!(g.target_ref, 96.0);
        assert_eq!(g.r_value(), 2.0);
    }

    #[test]
    fn ratchet_long_progression() {
        let mut g = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Long);
        // +0.5R → no move
        assert!(!g.try_ratchet(101.0));
        assert_eq!(g.koruma, 98.0);
        // +1R → BE
        assert!(g.try_ratchet(102.0));
        assert_eq!(g.koruma, 100.0);
        assert_eq!(g.active_sl(), 100.0);
        // +2R → entry+1R
        assert!(g.try_ratchet(104.0));
        assert_eq!(g.koruma, 102.0);
        // pullback to +1.5R → no loosen
        assert!(!g.try_ratchet(103.0));
        assert_eq!(g.koruma, 102.0);
        // +3R → entry+2R
        assert!(g.try_ratchet(106.0));
        assert_eq!(g.koruma, 104.0);
    }

    #[test]
    fn ratchet_short_progression() {
        let mut g = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Short);
        assert_eq!(g.entry_sl, 102.0);
        // price drops to 98 → +1R
        assert!(g.try_ratchet(98.0));
        assert_eq!(g.koruma, 100.0);
        // price drops to 96 → +2R, koruma goes to 98
        assert!(g.try_ratchet(96.0));
        assert_eq!(g.koruma, 98.0);
        // price bounces to 99 → no loosen
        assert!(!g.try_ratchet(99.0));
        assert_eq!(g.koruma, 98.0);
    }

    #[test]
    fn unrealized_r_signs() {
        let g = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Long);
        assert_eq!(g.unrealized_r(102.0), 1.0);
        assert_eq!(g.unrealized_r(98.0), -1.0);

        let s = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Short);
        assert_eq!(s.unrealized_r(98.0), 1.0);
        assert_eq!(s.unrealized_r(102.0), -1.0);
    }

    #[test]
    fn neutral_never_ratchets() {
        let mut g = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Neutral);
        assert!(!g.try_ratchet(200.0));
    }

    #[test]
    fn structural_long_double_bottom() {
        // W pattern: neckline=105, double bottom at 95.
        // Invalidation = 95 (below double bottom).
        // Measured move target = 105 + (105-95) = 115.
        let targets = vec![
            StructuralTarget { price: 115.0, weight: 0.8, label: "MM 1.0x" },
            StructuralTarget { price: 121.18, weight: 0.5, label: "MM 1.618x" },
        ];
        let g = PositionGuard::new_structural(
            105.0, 95.0, &targets, 2.0, &cfg(), Direction::Long,
        );
        assert!(g.structural);
        assert_eq!(g.entry, 105.0);
        assert_eq!(g.entry_sl, 95.0);        // invalidation, not ATR
        assert_eq!(g.target_ref, 115.0);      // MM 1.0×, not ATR×R
        assert_eq!(g.target_ref2, Some(121.18)); // MM 1.618×
        assert_eq!(g.r_value(), 10.0);        // |105-95|
    }

    #[test]
    fn structural_fallback_on_invalid_sl() {
        // SL above entry for long → invalid → falls back to ATR.
        let targets = vec![
            StructuralTarget { price: 115.0, weight: 0.8, label: "MM 1.0x" },
        ];
        let g = PositionGuard::new_structural(
            100.0, 105.0, &targets, 2.0, &cfg(), Direction::Long,
        );
        assert!(!g.structural);
        assert_eq!(g.entry_sl, 98.0); // ATR fallback
    }

    #[test]
    fn structural_short_head_and_shoulders() {
        // H&S: neckline=100, head=110.
        // Invalidation = 110 (above head).
        // Target = 100 - (110-100) = 90.
        let targets = vec![
            StructuralTarget { price: 90.0, weight: 0.8, label: "MM 1.0x" },
            StructuralTarget { price: 83.82, weight: 0.5, label: "MM 1.618x" },
        ];
        let g = PositionGuard::new_structural(
            100.0, 110.0, &targets, 2.0, &cfg(), Direction::Short,
        );
        assert!(g.structural);
        assert_eq!(g.entry_sl, 110.0);
        assert_eq!(g.target_ref, 90.0);
        assert_eq!(g.target_ref2, Some(83.82));
    }
}

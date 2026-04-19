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
    /// Hard floor for target/SL prices as fraction of entry. Mirrors
    /// `wyckoff.plan.min_target_price_frac` — guards the ATR-fallback
    /// guard against producing sub-zero `target_ref` on wide-stop
    /// short setups (entry - stop_distance * target_ref_r). See
    /// docs/notes/bug_negative_target_price.md. Default 0.001.
    pub min_target_price_frac: f64,
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
    /// Faz A — TP1 (`target_ref`) daha önce değdi mi? True olduktan sonra
    /// aynı target_ref değimi kapanış tetiklemez; yalnızca `target_ref2`
    /// ya da SL/reverse kapanışı ilerletir. `on_tp1` çağrısında set edilir.
    pub tp1_hit: bool,
}

impl PositionGuard {
    /// Construct a fresh guard from entry, ATR, profile config and
    /// direction. `koruma` starts at `entry_sl` (no ratchet yet).
    /// This is the **ATR fallback** — used when no structural detection
    /// provides invalidation/targets.
    pub fn new(entry: f64, atr: f64, cfg: &PositionGuardConfig, direction: Direction) -> Self {
        let stop_distance = atr * cfg.entry_sl_atr_mult;
        let sign = direction.sign();
        let price_floor = entry.abs() * cfg.min_target_price_frac;
        // bug_negative_target_price.md — short-side entry_sl/target_ref
        // can dive below zero on wide ATR stops. Clamp to a positive
        // floor so downstream renderers never see an "impossible"
        // sub-zero price. Long side stays as-is (entry + positive).
        let entry_sl = (entry - sign * stop_distance).max(price_floor);
        let target_ref = (entry + sign * stop_distance * cfg.target_ref_r).max(price_floor);
        Self {
            entry,
            entry_sl,
            koruma: entry_sl,
            target_ref,
            target_ref2: None,
            direction,
            structural: false,
            tp1_hit: false,
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
        // SL = invalidation price (where the pattern breaks).
        let entry_sl = invalidation_price;
        let price_floor = entry.abs() * cfg.min_target_price_frac;

        // Validate SL is on the correct side of entry AND above the
        // price floor (bug_negative_target_price.md — detector-side
        // structural prices can also leak negative on wide patterns).
        let sl_valid = entry_sl > price_floor && match direction {
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
        // Drop a TP2 that fell below the price floor (e.g. measured-move
        // for short on a wide head-and-shoulders projecting sub-zero).
        let target_ref2 = sorted.get(1)
            .map(|t| t.price)
            .filter(|p| *p > price_floor);

        // Validate target is on the correct side of entry AND above
        // the price floor.
        let tp_valid = target_ref > price_floor && match direction {
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
            tp1_hit: false,
        }
    }

    /// Faz B — Structural trailing stop. Walks `highs`/`lows` back
    /// from newest to oldest, finds the most recent *confirmed* pivot
    /// on the "wrong side" of the trade (swing low for long, swing
    /// high for short), applies `buffer_atr_mult * atr` buffer, and
    /// returns the candidate koruma if it is strictly tighter than
    /// the current value.
    ///
    /// Only active once `tp1_hit == true` — Faz A's BE ratchet is the
    /// floor; structural trailing only *tightens* beyond that, never
    /// replaces BE before TP1.
    ///
    /// `left`/`right` — bars-before and bars-after that the pivot must
    /// dominate to be "confirmed" (a 3/2 fractal means 3 bars before
    /// and 2 after; right-side bars are the delay cost of confirmation).
    pub fn structural_trail_candidate(
        &self,
        highs: &[f64],
        lows: &[f64],
        atr: f64,
        left: usize,
        right: usize,
        buffer_atr_mult: f64,
    ) -> Option<f64> {
        if !self.tp1_hit {
            return None;
        }
        if highs.len() != lows.len() || atr <= 0.0 {
            return None;
        }
        let buffer = atr * buffer_atr_mult;
        let pivot = match self.direction {
            Direction::Long => recent_pivot_low(lows, left, right),
            Direction::Short => recent_pivot_high(highs, left, right),
            Direction::Neutral => return None,
        }?;
        let candidate = match self.direction {
            Direction::Long => pivot - buffer,
            Direction::Short => pivot + buffer,
            Direction::Neutral => return None,
        };
        let tighter = match self.direction {
            Direction::Long => candidate > self.koruma,
            Direction::Short => candidate < self.koruma,
            Direction::Neutral => false,
        };
        // Sanity: for a long, candidate must stay below entry-less-than-R
        // craziness is unlikely but clamp to >= entry (BE floor).
        // Post-TP1 the floor is always entry; never loosen below it.
        let respects_floor = match self.direction {
            Direction::Long => candidate >= self.entry,
            Direction::Short => candidate <= self.entry,
            Direction::Neutral => false,
        };
        if tighter && respects_floor {
            Some(candidate)
        } else {
            None
        }
    }

    /// Mutable counterpart to `structural_trail_candidate`. Returns
    /// `true` iff `koruma` was updated.
    pub fn try_structural_trail(
        &mut self,
        highs: &[f64],
        lows: &[f64],
        atr: f64,
        left: usize,
        right: usize,
        buffer_atr_mult: f64,
    ) -> bool {
        if let Some(c) =
            self.structural_trail_candidate(highs, lows, atr, left, right, buffer_atr_mult)
        {
            self.koruma = c;
            true
        } else {
            false
        }
    }

    /// Faz A — Called when `target_ref` (TP1) is hit for the first
    /// time. Sets `tp1_hit=true` and ratchets `koruma` up to `entry`
    /// (break-even). Idempotent: repeated calls are a no-op.
    ///
    /// Returns `true` if state actually changed. The caller is
    /// responsible for persisting `tp1_hit=true`, `tp1_price`, and the
    /// updated `koruma` to `qtss_setups`.
    ///
    /// Math guarantee: with 50 % realised at +1R and BE stop on the
    /// remaining half, the setup's minimum outcome is
    /// `0.5 * 1R + 0.5 * 0R = +0.5R` — "giving back from the profit
    /// zone" is impossible once this is called.
    pub fn on_tp1(&mut self) -> bool {
        if self.tp1_hit {
            return false;
        }
        self.tp1_hit = true;
        // BE ratchet — move koruma to entry (ignoring the monotonic
        // ratchet formula; koruma=entry is always tighter than
        // entry_sl by construction for a valid setup).
        let tighter = match self.direction {
            Direction::Long => self.entry > self.koruma,
            Direction::Short => self.entry < self.koruma,
            Direction::Neutral => false,
        };
        if tighter {
            self.koruma = self.entry;
        }
        true
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

/// Walk newest → oldest; return the price of the most recent
/// *confirmed* pivot low (a bar whose low is ≤ the `left` bars
/// before it and ≤ the `right` bars after it). `right > 0`
/// guarantees the pivot has had time to be "confirmed" by
/// subsequent bars — we do not nominate the latest bar itself.
fn recent_pivot_low(lows: &[f64], left: usize, right: usize) -> Option<f64> {
    let n = lows.len();
    if n < left + right + 1 {
        return None;
    }
    let newest_confirmed = n - right - 1;
    let mut i = newest_confirmed;
    while i >= left {
        let v = lows[i];
        let left_ok = (i - left..i).all(|j| lows[j] >= v);
        let right_ok = (i + 1..=i + right).all(|j| lows[j] >= v);
        if left_ok && right_ok {
            return Some(v);
        }
        if i == left {
            break;
        }
        i -= 1;
    }
    None
}

fn recent_pivot_high(highs: &[f64], left: usize, right: usize) -> Option<f64> {
    let n = highs.len();
    if n < left + right + 1 {
        return None;
    }
    let newest_confirmed = n - right - 1;
    let mut i = newest_confirmed;
    while i >= left {
        let v = highs[i];
        let left_ok = (i - left..i).all(|j| highs[j] <= v);
        let right_ok = (i + 1..=i + right).all(|j| highs[j] <= v);
        if left_ok && right_ok {
            return Some(v);
        }
        if i == left {
            break;
        }
        i -= 1;
    }
    None
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
            min_target_price_frac: 0.001,
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
    fn on_tp1_long_moves_koruma_to_entry() {
        let mut g = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Long);
        assert!(!g.tp1_hit);
        assert_eq!(g.koruma, 98.0);
        assert!(g.on_tp1());
        assert!(g.tp1_hit);
        assert_eq!(g.koruma, 100.0);   // BE
        assert_eq!(g.active_sl(), 100.0);
        // Second call is a no-op.
        assert!(!g.on_tp1());
        assert_eq!(g.koruma, 100.0);
    }

    #[test]
    fn on_tp1_short_moves_koruma_to_entry() {
        let mut g = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Short);
        assert_eq!(g.koruma, 102.0);
        assert!(g.on_tp1());
        assert_eq!(g.koruma, 100.0);
        assert_eq!(g.active_sl(), 100.0);
    }

    #[test]
    fn on_tp1_preserves_already_tighter_koruma() {
        // Ratchet ran past BE already, on_tp1 must not loosen.
        let mut g = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Long);
        g.try_ratchet(104.0);  // koruma → 102.0 (entry + 1R)
        assert_eq!(g.koruma, 102.0);
        assert!(g.on_tp1());
        assert_eq!(g.koruma, 102.0);   // kept tighter
        assert!(g.tp1_hit);
    }

    // ----- Faz B: structural trailing -----

    #[test]
    fn recent_pivot_low_picks_confirmed_swing() {
        // bars:        0   1   2   3   4   5   6   7   8
        // lows:      [10, 9,  8,  7,  8,  9,  8,  9, 10]
        // With left=2, right=2: newest_confirmed = 9-2-1 = 6.
        //   i=6: lows[6]=8, left[4..6]=[8,9]→8≤…, right[7..=8]=[9,10]→8≤… → pivot @8 ✓
        // Most-recent-wins: returns 8, not the older 7.
        let lows = vec![10.0, 9.0, 8.0, 7.0, 8.0, 9.0, 8.0, 9.0, 10.0];
        assert_eq!(recent_pivot_low(&lows, 2, 2), Some(8.0));
    }

    #[test]
    fn recent_pivot_high_picks_confirmed_swing() {
        // Symmetric: returns the most-recent confirmed pivot high (12), not 13.
        let highs = vec![10.0, 11.0, 12.0, 13.0, 12.0, 11.0, 12.0, 11.0, 10.0];
        assert_eq!(recent_pivot_high(&highs, 2, 2), Some(12.0));
    }

    #[test]
    fn recent_pivot_low_none_when_too_short() {
        let lows = vec![10.0, 9.0, 8.0];
        assert_eq!(recent_pivot_low(&lows, 2, 2), None);
    }

    #[test]
    fn structural_trail_inactive_before_tp1() {
        let mut g = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Long);
        // Build a long-dominated series where pivot_low = 99 exists.
        let lows = vec![101.0, 100.0, 99.0, 100.0, 101.0];
        let highs = vec![102.0; 5];
        assert!(!g.try_structural_trail(&highs, &lows, 1.0, 1, 1, 0.25));
        assert_eq!(g.koruma, 98.0); // unchanged — tp1 not hit
    }

    #[test]
    fn structural_trail_long_tightens_after_tp1() {
        let mut g = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Long);
        g.on_tp1();                  // koruma=100, tp1_hit=true
        // pivot_low at idx=2 value 102 (all others ≥ 102), buffer=0.25*1=0.25.
        let lows = vec![103.0, 102.5, 102.0, 102.5, 103.0];
        let highs = vec![105.0; 5];
        assert!(g.try_structural_trail(&highs, &lows, 1.0, 1, 1, 0.25));
        assert!((g.koruma - 101.75).abs() < 1e-9);   // 102 - 0.25
    }

    #[test]
    fn structural_trail_respects_be_floor() {
        // pivot below entry → would loosen; must be rejected.
        let mut g = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Long);
        g.on_tp1();                  // koruma=100 (BE)
        let lows = vec![101.0, 99.0, 98.0, 99.0, 101.0]; // pivot=98 < entry
        let highs = vec![105.0; 5];
        assert!(!g.try_structural_trail(&highs, &lows, 1.0, 1, 1, 0.25));
        assert_eq!(g.koruma, 100.0); // floor held
    }

    #[test]
    fn structural_trail_short_after_tp1() {
        let mut g = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Short);
        g.on_tp1();                  // koruma=100 (BE)
        // pivot_high at idx=2 value 98 (buffer 0.25 → koruma candidate 98.25).
        let highs = vec![97.0, 97.5, 98.0, 97.5, 97.0];
        let lows = vec![95.0; 5];
        assert!(g.try_structural_trail(&highs, &lows, 1.0, 1, 1, 0.25));
        assert!((g.koruma - 98.25).abs() < 1e-9);
    }

    #[test]
    fn structural_trail_monotonic() {
        let mut g = PositionGuard::new(100.0, 2.0, &cfg(), Direction::Long);
        g.on_tp1();
        let lows = vec![103.0, 102.5, 102.0, 102.5, 103.0];
        let highs = vec![105.0; 5];
        g.try_structural_trail(&highs, &lows, 1.0, 1, 1, 0.25);
        assert!((g.koruma - 101.75).abs() < 1e-9);
        // Older, looser pivot → no loosen.
        let lows2 = vec![101.5, 101.0, 100.5, 101.0, 101.5];
        assert!(!g.try_structural_trail(&highs, &lows2, 1.0, 1, 1, 0.25));
        assert!((g.koruma - 101.75).abs() < 1e-9);
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

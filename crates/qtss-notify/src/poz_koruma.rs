//! Faz 9.7.4 — Poz Koruma (Profit Ratchet, variant A1).
//!
//! Every end-of-day the watcher calls [`evaluate`] with the current
//! price + the setup's ratchet memory. If the daily gain clears
//! `min_gain_pct`, the SL is pushed up by `cumulative_pct + gain`
//! (minus a `buffer_pct` wick-safety cushion). The reference price
//! always advances so a sideways-after-gain day doesn't re-claim the
//! same move on the next ratchet.
//!
//! A1 = conservative: `new_SL = entry * (1 + cumulative_pct / 100)`
//! for LONG (inverted for SHORT). No scale-out, no partial trails —
//! the SL is the one knob this module moves.
//!
//! Pure module: all DB I/O happens in the watcher. Config loader
//! reads `notify.poz_koruma.*` keys (see migration 0136).

use chrono::{DateTime, Duration, Utc};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::card::builder::SetupDirection;

const MODULE: &str = "notify";

#[derive(Debug, Clone, Copy)]
pub struct PozKorumaConfig {
    pub enabled: bool,
    pub eod_hour_utc: u8,
    pub min_gain_pct: f64,
    pub buffer_pct: f64,
}

impl PozKorumaConfig {
    pub const FALLBACK: Self = Self {
        enabled: true,
        eod_hour_utc: 0,
        min_gain_pct: 0.5,
        buffer_pct: 0.05,
    };
}

/// Snapshot the watcher hands to [`evaluate`] each tick.
#[derive(Debug, Clone, Copy)]
pub struct RatchetInput {
    pub direction: SetupDirection,
    pub entry_price: Decimal,
    pub current_price: Decimal,
    /// Original SL at setup open. The ratchet never lowers below
    /// this (LONG) / above this (SHORT).
    pub original_sl: Decimal,
    /// Latest SL after prior ratchets.
    pub current_sl: Decimal,
    /// Price anchor for the *current* day's gain measurement. `None`
    /// on the very first call — we seed with `entry_price`.
    pub reference_price: Option<Decimal>,
    /// Ratchet history — sum of per-day gains already locked into SL.
    pub cumulative_pct: f64,
    pub last_update_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RatchetStep {
    pub new_sl: Decimal,
    pub new_reference_price: Decimal,
    pub new_cumulative_pct: f64,
    pub gained_pct: f64,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy)]
pub enum RatchetOutcome {
    NoChange,
    Ratcheted(RatchetStep),
    /// Reference advanced but SL did not move (daily gain too small).
    /// The watcher still persists `last_update_at` + `reference_price`.
    ReferenceOnly {
        new_reference_price: Decimal,
        gained_pct: f64,
        at: DateTime<Utc>,
    },
}

/// Pure evaluator. Call once per tick; returns [`RatchetOutcome::NoChange`]
/// unless we've crossed the configured end-of-day boundary.
pub fn evaluate(
    input: &RatchetInput,
    cfg: &PozKorumaConfig,
    now: DateTime<Utc>,
) -> RatchetOutcome {
    if !cfg.enabled {
        return RatchetOutcome::NoChange;
    }
    let reference = input
        .reference_price
        .unwrap_or(input.entry_price);

    // EOD gating — only ratchet once per UTC-day-past-eod_hour window.
    if !is_new_ratchet_day(input.last_update_at, now, cfg.eod_hour_utc) {
        return RatchetOutcome::NoChange;
    }

    let gained = daily_gain_pct(input.direction, reference, input.current_price);
    let new_reference = input.current_price;
    if gained < cfg.min_gain_pct {
        return RatchetOutcome::ReferenceOnly {
            new_reference_price: new_reference,
            gained_pct: gained,
            at: now,
        };
    }
    let new_cumulative = input.cumulative_pct + gained;
    let new_sl = compute_ratcheted_sl(
        input.direction,
        input.entry_price,
        input.current_sl,
        input.original_sl,
        new_cumulative,
        cfg.buffer_pct,
    );
    RatchetOutcome::Ratcheted(RatchetStep {
        new_sl,
        new_reference_price: new_reference,
        new_cumulative_pct: new_cumulative,
        gained_pct: gained,
        at: now,
    })
}

/// Was the last ratchet on a different "ratchet day" than `now`?
/// A ratchet day is the 24h window starting at `eod_hour_utc`. Using
/// floor-div arithmetic keeps this branchless.
fn is_new_ratchet_day(
    last: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    eod_hour_utc: u8,
) -> bool {
    let Some(last) = last else {
        // First ratchet check ever — gate to "only after the first
        // EOD boundary past opening" is too restrictive for testing;
        // allow it immediately. The `min_gain_pct` check still
        // prevents noise.
        return true;
    };
    let offset = Duration::hours(eod_hour_utc as i64);
    let last_bucket = (last - offset).date_naive();
    let now_bucket = (now - offset).date_naive();
    now_bucket > last_bucket
}

fn daily_gain_pct(direction: SetupDirection, reference: Decimal, current: Decimal) -> f64 {
    let Some(r) = reference.to_f64() else { return 0.0 };
    let Some(c) = current.to_f64() else { return 0.0 };
    if r.abs() < 1e-12 {
        return 0.0;
    }
    let raw = (c - r) / r * 100.0;
    match direction {
        SetupDirection::Long => raw,
        SetupDirection::Short => -raw,
    }
}

fn compute_ratcheted_sl(
    direction: SetupDirection,
    entry: Decimal,
    current_sl: Decimal,
    original_sl: Decimal,
    cumulative_pct: f64,
    buffer_pct: f64,
) -> Decimal {
    let Some(e) = entry.to_f64() else { return current_sl };
    // Net = cumulative gain minus the safety buffer; buffer never
    // pushes SL *below* the original SL (LONG) / above it (SHORT).
    let net_pct = (cumulative_pct - buffer_pct).max(0.0);
    let shifted = match direction {
        SetupDirection::Long => e * (1.0 + net_pct / 100.0),
        SetupDirection::Short => e * (1.0 - net_pct / 100.0),
    };
    let candidate = Decimal::from_f64(shifted).unwrap_or(current_sl);
    match direction {
        SetupDirection::Long => candidate.max(original_sl).max(current_sl),
        SetupDirection::Short => {
            // For SHORT, "tighter" = smaller number. Clamp against
            // original_sl (upper bound) and current_sl (already
            // ratcheted — don't loosen).
            let capped = candidate.min(original_sl);
            capped.min(current_sl)
        }
    }
}

pub async fn load_config(pool: &PgPool) -> PozKorumaConfig {
    let f = PozKorumaConfig::FALLBACK;
    let enabled_raw = qtss_storage::resolve_system_string(
        pool, MODULE, "poz_koruma.enabled",
        "QTSS_POZ_KORUMA_ENABLED",
        if f.enabled { "true" } else { "false" },
    )
    .await;
    let enabled = matches!(
        enabled_raw.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "on"
    );
    let eod_hour_utc = qtss_storage::resolve_system_u64(
        pool, MODULE, "poz_koruma.eod_hour_utc",
        "QTSS_POZ_KORUMA_EOD_HOUR_UTC",
        f.eod_hour_utc as u64, 0, 23,
    )
    .await as u8;
    let min_gain_pct = qtss_storage::resolve_system_f64(
        pool, MODULE, "poz_koruma.min_gain_pct",
        "QTSS_POZ_KORUMA_MIN_GAIN_PCT", f.min_gain_pct,
    )
    .await;
    let buffer_pct = qtss_storage::resolve_system_f64(
        pool, MODULE, "poz_koruma.buffer_pct",
        "QTSS_POZ_KORUMA_BUFFER_PCT", f.buffer_pct,
    )
    .await;
    PozKorumaConfig { enabled, eod_hour_utc, min_gain_pct, buffer_pct }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    fn cfg() -> PozKorumaConfig {
        PozKorumaConfig::FALLBACK
    }

    fn ts(y: i32, mo: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, 0, 0).unwrap()
    }

    fn long_input(
        current_price: Decimal,
        ref_price: Option<Decimal>,
        cum: f64,
        last: Option<DateTime<Utc>>,
    ) -> RatchetInput {
        RatchetInput {
            direction: SetupDirection::Long,
            entry_price: dec!(100),
            current_price,
            original_sl: dec!(95),
            current_sl: dec!(95),
            reference_price: ref_price,
            cumulative_pct: cum,
            last_update_at: last,
        }
    }

    #[test]
    fn disabled_config_always_no_change() {
        let mut c = cfg();
        c.enabled = false;
        let input = long_input(dec!(110), None, 0.0, None);
        assert!(matches!(evaluate(&input, &c, ts(2026, 4, 17, 12)), RatchetOutcome::NoChange));
    }

    #[test]
    fn first_day_gain_ratchets() {
        let input = long_input(dec!(102), None, 0.0, None);
        let out = evaluate(&input, &cfg(), ts(2026, 4, 17, 12));
        match out {
            RatchetOutcome::Ratcheted(s) => {
                assert!((s.gained_pct - 2.0).abs() < 0.01);
                // new_sl = 100 * (1 + (2 - 0.05)/100) = 101.95
                assert!((s.new_sl.to_f64().unwrap() - 101.95).abs() < 0.001);
                assert_eq!(s.new_reference_price, dec!(102));
            }
            other => panic!("expected Ratcheted, got {other:?}"),
        }
    }

    #[test]
    fn same_day_second_call_no_change() {
        let input = long_input(
            dec!(103),
            Some(dec!(100)),
            2.0,
            Some(ts(2026, 4, 17, 1)),
        );
        assert!(matches!(
            evaluate(&input, &cfg(), ts(2026, 4, 17, 12)),
            RatchetOutcome::NoChange
        ));
    }

    #[test]
    fn noise_day_advances_reference_only() {
        let mut c = cfg();
        c.min_gain_pct = 1.0;
        // 0.3% gain — below min_gain_pct (1.0). Reference advances.
        let input = long_input(
            dec!(100.3),
            Some(dec!(100)),
            5.0, // already-locked prior gains
            Some(ts(2026, 4, 16, 1)),
        );
        let out = evaluate(&input, &c, ts(2026, 4, 17, 12));
        match out {
            RatchetOutcome::ReferenceOnly { new_reference_price, gained_pct, .. } => {
                assert_eq!(new_reference_price, dec!(100.3));
                assert!((gained_pct - 0.3).abs() < 0.01);
            }
            other => panic!("expected ReferenceOnly, got {other:?}"),
        }
    }

    #[test]
    fn short_direction_inverts_sl_movement() {
        // SHORT from 100 → price drops to 98 (2% gain for short).
        let mut input = long_input(dec!(98), None, 0.0, None);
        input.direction = SetupDirection::Short;
        input.original_sl = dec!(105);
        input.current_sl = dec!(105);
        let out = evaluate(&input, &cfg(), ts(2026, 4, 17, 12));
        match out {
            RatchetOutcome::Ratcheted(s) => {
                // new_sl = 100 * (1 - (2 - 0.05)/100) = 98.05
                assert!((s.new_sl.to_f64().unwrap() - 98.05).abs() < 0.001);
            }
            other => panic!("expected Ratcheted, got {other:?}"),
        }
    }

    #[test]
    fn new_sl_never_below_original_for_long() {
        // Start with a big cumulative but a loss on the day → net_pct
        // floored at 0 by the buffer subtraction, so new_sl = entry.
        let input = long_input(
            dec!(100.01),
            Some(dec!(100)),
            0.03, // next cumulative = 0.04, buffer 0.05 → net 0 → SL=entry=100
            Some(ts(2026, 4, 16, 1)),
        );
        let mut c = cfg();
        c.min_gain_pct = 0.0; // force ratchet path
        let out = evaluate(&input, &c, ts(2026, 4, 17, 12));
        if let RatchetOutcome::Ratcheted(s) = out {
            // new_sl clamped to max(original_sl=95, current_sl=95, candidate=100) = 100
            assert!((s.new_sl.to_f64().unwrap() - 100.0).abs() < 0.001);
        } else {
            panic!("expected Ratcheted");
        }
    }
}

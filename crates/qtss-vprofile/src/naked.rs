//! Naked VPOC tracking.
//!
//! A "naked" VPOC is the VPOC of a previous range that price has not
//! revisited since it formed. Naked VPOCs are strong magnet targets per
//! Villahermosa *Wyckoff 2.0* §7.4.3 — the market tends to return to
//! mean-volume reference points eventually.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::profile::VolumeProfile;
use qtss_domain::v2::bar::Bar;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NakedVpoc {
    /// Price level.
    pub price: Decimal,
    /// When the source range ended (profile snapshot time).
    pub formed_at: DateTime<Utc>,
    /// Bar index at formation (in the caller's bar window).
    pub formed_bar_index: u64,
    /// True until price revisits the level (low <= price <= high).
    pub is_naked: bool,
}

/// Wrapper list — kept simple so callers can serialize/persist easily.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct NakedVpocList {
    pub levels: Vec<NakedVpoc>,
}

/// Given prior profile snapshots and the bars that came AFTER them,
/// mark which VPOCs are still naked. A profile's VPOC is "tested" once
/// any subsequent bar's `[low, high]` envelope contains it.
///
/// Caller responsibilities:
/// - `snapshots` ordered oldest → newest.
/// - `subsequent_bars` chronologically after each snapshot's
///   `formed_at`. We don't filter by time here; pass the relevant
///   slice.
pub fn detect_naked_vpocs(
    snapshots: &[(VolumeProfile, DateTime<Utc>, u64)],
    subsequent_bars: &[Bar],
) -> NakedVpocList {
    let levels = snapshots
        .iter()
        .map(|(prof, t, idx)| {
            let tested = subsequent_bars
                .iter()
                .any(|b| b.low <= prof.vpoc && b.high >= prof.vpoc);
            NakedVpoc {
                price: prof.vpoc,
                formed_at: *t,
                formed_bar_index: *idx,
                is_naked: !tested,
            }
        })
        .collect();
    NakedVpocList { levels }
}

impl NakedVpocList {
    /// Naked levels only.
    pub fn naked(&self) -> impl Iterator<Item = &NakedVpoc> {
        self.levels.iter().filter(|n| n.is_naked)
    }

    /// Closest naked VPOC to `price` in the given direction. `up = true`
    /// → smallest naked above; `up = false` → largest naked below.
    pub fn nearest_naked(&self, price: Decimal, up: bool) -> Option<Decimal> {
        self.naked()
            .filter(|n| if up { n.price > price } else { n.price < price })
            .map(|n| n.price)
            .min_by(|a, b| {
                let da = (*a - price).abs();
                let db = (*b - price).abs();
                da.cmp(&db)
            })
    }
}

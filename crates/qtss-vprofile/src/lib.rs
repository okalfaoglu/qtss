//! qtss-vprofile — Volume Profile primitives.
//!
//! Foundation crate for P7 Wyckoff upgrades (Villahermosa *Wyckoff 2.0*
//! ch. 7.4). Builds a price-binned volume distribution from a bar (or,
//! later, tick) sequence and exposes the canonical Wyckoff/Auction-
//! Theory references:
//!
//! - **VPOC**  — Volume Point of Control (price bin with the highest
//!   traded volume).
//! - **VAH / VAL** — Value Area High/Low; the smallest contiguous band
//!   around VPOC that contains `value_area_pct` of total volume (default
//!   70%).
//! - **HVN**  — High Volume Nodes; local volume maxima (peaks).
//! - **LVN**  — Low Volume Nodes; local volume minima (valleys / fast-
//!   transit "low-resistance" zones).
//! - **DVPOC** — Developing VPOC, recomputed as the range/profile grows.
//! - **Naked VPOC** — historical VPOCs that price has not yet revisited.
//!
//! ## Design
//!
//! Pure data crate — no IO, no DB, no async. Builders implement a single
//! [`VolumeProfileBuilder`] trait so adding a new source (tick stream,
//! aggregated trades, footprint) is one impl + one registration in the
//! caller (CLAUDE.md rule #1). All thresholds live in [`VProfileConfig`]
//! and validate up-front (CLAUDE.md rule #2).
//!
//! Asset-class agnostic: only `Bar` / `Decimal` in the public surface,
//! no venue or asset-specific assumptions (CLAUDE.md rule #4).

mod builder;
mod config;
mod error;
mod profile;
mod naked;

#[cfg(test)]
mod tests;

pub use builder::{BarBasedBuilder, VolumeProfileBuilder};
pub use config::VProfileConfig;
pub use error::{VProfileError, VProfileResult};
pub use naked::{NakedVpoc, NakedVpocList, detect_naked_vpocs};
pub use profile::{VPBin, VolumeProfile};

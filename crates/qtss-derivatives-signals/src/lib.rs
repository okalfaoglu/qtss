//! qtss-derivatives-signals — crypto derivatives event detectors.
//!
//! Turns the raw feeds `qtss-onchain` already pulls (funding rate,
//! open interest, mark-index premium, taker long/short, global long/
//! short ratio) into first-class detection events the Confluence
//! engine can weigh alongside chart patterns. Five families:
//!
//!   * **FundingSpike** — latest |funding_rate| > Nσ of rolling window
//!   * **OIImbalance** — ΔOI% over window diverges from ΔPrice%
//!   * **BasisDislocation** — |mark - index| / index > threshold
//!   * **LongShortRatioExtreme** — LSR outside [min, max] band
//!   * **TakerFlowImbalance** — taker_buy/taker_sell ratio extreme
//!
//! Each detector is a pure function reading a JSON payload (the
//! `data_snapshots.response_json` column format `qtss-onchain` writes)
//! plus a [`DerivConfig`]. Engine writer wires them up.

mod config;
mod event;
mod events;

pub use config::DerivConfig;
pub use event::{DerivEvent, DerivEventKind};
pub use events::{
    detect_basis_dislocation, detect_funding_spike, detect_long_short_extreme,
    detect_oi_imbalance, detect_taker_flow_imbalance,
};

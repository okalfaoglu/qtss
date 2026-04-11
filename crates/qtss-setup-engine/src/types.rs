//! Shared enums for the Setup Engine. Every enum carries a stable
//! `as_str()` slug — that slug is what ends up in PostgreSQL
//! `CHECK (... IN (...))` constraints and JSON payloads, so DO NOT
//! change the strings without a migration.

use serde::{Deserialize, Serialize};

// Direction is reused verbatim from the confluence crate so the
// engine and the scorer never disagree on what "long"/"short" mean.
pub use qtss_confluence::ConfluenceDirection as Direction;

/// Setup profile — how long the setup is expected to live and how
/// aggressively the PositionGuard should ratchet.
///
/// * `T` — short-term (minutes/hours). Tight stop, fast ratchet,
///   smallest per-setup risk.
/// * `Q` — short-mid (hours/days). The most market-sensitive profile
///   and the default for the Q-RADAR workflow.
/// * `D` — mid/long (days/weeks). Wide stop, slow ratchet, largest
///   per-setup risk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    T,
    Q,
    D,
}

impl Profile {
    pub fn as_str(self) -> &'static str {
        match self {
            Profile::T => "t",
            Profile::Q => "q",
            Profile::D => "d",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "t" | "T" => Some(Profile::T),
            "q" | "Q" => Some(Profile::Q),
            "d" | "D" => Some(Profile::D),
            _ => None,
        }
    }
}

/// Wave-context classification attached to a setup. Populated by the
/// engine from whichever detector arm triggered the setup; used by
/// the reporting layer and the chart renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AltType {
    ReactionLow,
    TrendLow,
    ReversalHigh,
    SellingHigh,
}

impl AltType {
    pub fn as_str(self) -> &'static str {
        match self {
            AltType::ReactionLow => "reaction_low",
            AltType::TrendLow => "trend_low",
            AltType::ReversalHigh => "reversal_high",
            AltType::SellingHigh => "selling_high",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "reaction_low" => Some(AltType::ReactionLow),
            "trend_low" => Some(AltType::TrendLow),
            "reversal_high" => Some(AltType::ReversalHigh),
            "selling_high" => Some(AltType::SellingHigh),
            _ => None,
        }
    }
}

/// Setup lifecycle state. English slugs are the on-disk format —
/// localisation lives in `web/locales`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SetupState {
    Flat,
    Armed,
    Active,
    Closed,
}

impl SetupState {
    pub fn as_str(self) -> &'static str {
        match self {
            SetupState::Flat => "flat",
            SetupState::Armed => "armed",
            SetupState::Active => "active",
            SetupState::Closed => "closed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "flat" => Some(SetupState::Flat),
            "armed" => Some(SetupState::Armed),
            "active" => Some(SetupState::Active),
            "closed" => Some(SetupState::Closed),
            _ => None,
        }
    }
}

/// Venue class — BIST + Crypto are live in Faz 8.0; the rest are
/// schema-only placeholders so migrations do not need editing when
/// the adapters land.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VenueClass {
    Crypto,
    Bist,
    UsEquities,
    Commodities,
    Fx,
}

impl VenueClass {
    pub fn as_str(self) -> &'static str {
        match self {
            VenueClass::Crypto => "crypto",
            VenueClass::Bist => "bist",
            VenueClass::UsEquities => "us_equities",
            VenueClass::Commodities => "commodities",
            VenueClass::Fx => "fx",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "crypto" => Some(VenueClass::Crypto),
            "bist" => Some(VenueClass::Bist),
            "us_equities" => Some(VenueClass::UsEquities),
            "commodities" => Some(VenueClass::Commodities),
            "fx" => Some(VenueClass::Fx),
            _ => None,
        }
    }
}

/// Reason a setup transitions to `Closed`. Written into
/// `setup_events.payload` and the `close_reason` column on
/// `qtss_v2_setups`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    TargetHit,
    StopHit,
    ReverseSignal,
    Manual,
}

impl CloseReason {
    pub fn as_str(self) -> &'static str {
        match self {
            CloseReason::TargetHit => "target_hit",
            CloseReason::StopHit => "stop_hit",
            CloseReason::ReverseSignal => "reverse_signal",
            CloseReason::Manual => "manual",
        }
    }
}

/// Reason the allocator refused to arm a candidate setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    TotalRiskCap,
    MaxConcurrent,
    CorrelationCap,
    CommissionGate,
}

impl RejectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            RejectReason::TotalRiskCap => "total_risk_cap",
            RejectReason::MaxConcurrent => "max_concurrent",
            RejectReason::CorrelationCap => "correlation_cap",
            RejectReason::CommissionGate => "commission_gate",
        }
    }
}

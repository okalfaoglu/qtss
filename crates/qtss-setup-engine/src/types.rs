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

/// Market risk mode derived from the dominant regime.
/// Determines per-profile behavior: which profiles can arm new
/// setups, what guven threshold to use, whether to stop entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskMode {
    RiskOn,
    RiskNeutral,
    RiskOff,
}

impl RiskMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RiskMode::RiskOn => "risk_on",
            RiskMode::RiskNeutral => "risk_neutral",
            RiskMode::RiskOff => "risk_off",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "risk_on" => Some(RiskMode::RiskOn),
            "risk_neutral" => Some(RiskMode::RiskNeutral),
            "risk_off" => Some(RiskMode::RiskOff),
            _ => None,
        }
    }
}

/// Per-profile behavior in a given risk mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskModeBehavior {
    /// Normal setup arming.
    Active,
    /// Raise guven threshold, only strong structures.
    Selective,
    /// Keep existing setups, no new ones.
    Continue,
    /// Stop all new setups, minimize exposure.
    Stopped,
}

impl RiskModeBehavior {
    pub fn from_str(s: &str) -> Self {
        match s {
            "active" => Self::Active,
            "selective" => Self::Selective,
            "continue" => Self::Continue,
            "stopped" => Self::Stopped,
            _ => Self::Selective,
        }
    }
}

/// Setup lifecycle state. English slugs are the on-disk format —
/// localisation lives in `web/locales`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupState {
    Flat,
    Armed,
    Active,
    Closed,
    ClosedWin,
    ClosedLoss,
    ClosedManual,
}

impl SetupState {
    pub fn as_str(self) -> &'static str {
        match self {
            SetupState::Flat => "flat",
            SetupState::Armed => "armed",
            SetupState::Active => "active",
            SetupState::Closed => "closed",
            SetupState::ClosedWin => "closed_win",
            SetupState::ClosedLoss => "closed_loss",
            SetupState::ClosedManual => "closed_manual",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "flat" => Some(SetupState::Flat),
            "armed" => Some(SetupState::Armed),
            "active" => Some(SetupState::Active),
            "closed" => Some(SetupState::Closed),
            "closed_win" => Some(SetupState::ClosedWin),
            "closed_loss" => Some(SetupState::ClosedLoss),
            "closed_manual" => Some(SetupState::ClosedManual),
            _ => None,
        }
    }

    pub fn is_closed(self) -> bool {
        matches!(self, Self::Closed | Self::ClosedWin | Self::ClosedLoss | Self::ClosedManual)
    }

    /// Derive the granular close state from a CloseReason.
    pub fn from_close_reason(reason: CloseReason) -> Self {
        match reason {
            CloseReason::TargetHit => SetupState::ClosedWin,
            CloseReason::StopHit => SetupState::ClosedLoss,
            CloseReason::ReverseSignal => SetupState::ClosedLoss,
            CloseReason::Manual => SetupState::ClosedManual,
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
    // Faz 9.1 — classic confluence gate vetoes.
    GateKillSwitch,
    GateStaleData,
    GateNewsBlackout,
    GateRegimeOpposite,
    GateDirectionConsensus,
    GateBelowMinScore,
    GateNoDirection,
    // Faz 9.3.3 — LightGBM inference sidecar veto (gate_enabled=true).
    AiGate,
}

impl RejectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            RejectReason::TotalRiskCap => "total_risk_cap",
            RejectReason::MaxConcurrent => "max_concurrent",
            RejectReason::CorrelationCap => "correlation_cap",
            RejectReason::CommissionGate => "commission_gate",
            RejectReason::GateKillSwitch => "gate_kill_switch",
            RejectReason::GateStaleData => "gate_stale_data",
            RejectReason::GateNewsBlackout => "gate_news_blackout",
            RejectReason::GateRegimeOpposite => "gate_regime_opposite",
            RejectReason::GateDirectionConsensus => "gate_direction_consensus",
            RejectReason::GateBelowMinScore => "gate_below_min_score",
            RejectReason::GateNoDirection => "gate_no_direction",
            RejectReason::AiGate => "ai_gate",
        }
    }

    /// Map a Faz 9.1 `VetoKind` onto the persisted rejection slug.
    pub fn from_veto_kind(kind: crate::confluence_gate::VetoKind) -> Self {
        use crate::confluence_gate::VetoKind;
        match kind {
            VetoKind::KillSwitch => RejectReason::GateKillSwitch,
            VetoKind::StaleData => RejectReason::GateStaleData,
            VetoKind::NewsBlackout => RejectReason::GateNewsBlackout,
            VetoKind::RegimeOpposite => RejectReason::GateRegimeOpposite,
            VetoKind::DirectionConsensusFail => RejectReason::GateDirectionConsensus,
            VetoKind::BelowMinScore => RejectReason::GateBelowMinScore,
            VetoKind::NoDirection => RejectReason::GateNoDirection,
        }
    }
}

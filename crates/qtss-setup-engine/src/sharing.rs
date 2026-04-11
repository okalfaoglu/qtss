//! D/T/Q Setup Sharing Layer.
//!
//! Determines whether a setup should be shared on a subscription
//! channel and how. This is NOT the setup creation logic — setup
//! engine creates setups independently; this layer decides what
//! gets published to subscribers.
//!
//! ## Channels
//!
//! - **D-Analiz** — mid-term, BIST/COIN/NASDAQ, blue chip + growth + value.
//!   Risk-off → only strong structures shared.
//! - **T-Analiz** — short-term (hours-15d), index-heavy.
//!   Risk-off → sharing stopped/minimal.
//! - **Q-Analiz (RADAR)** — capital-tracked virtual portfolio (≈1.5M TL).
//!   Each share includes capital + quantity. Add-on, partial sell.
//!   Weekly/monthly reports.
//!
//! ## Market Mode (Risk Mode)
//!
//! Derived from the dominant regime. Affects sharing behavior per channel:
//!
//! | Channel | Risk-on       | Risk-nötr        | Risk-off             |
//! |---------|---------------|------------------|----------------------|
//! | D       | Active        | Continue         | Selective (strong)   |
//! | T       | Active (LONG) | Selective (LONG) | Stopped/minimal      |
//! | Q       | Active        | Selective        | Selective (very)     |

use crate::types::{Profile, RiskMode, RiskModeBehavior};

/// Sharing channel — one-to-one with the subscription tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SharingChannel {
    D,
    T,
    Q,
}

impl SharingChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            SharingChannel::D => "d",
            SharingChannel::T => "t",
            SharingChannel::Q => "q",
        }
    }

    pub fn from_profile(profile: Profile) -> Self {
        match profile {
            Profile::D => SharingChannel::D,
            Profile::T => SharingChannel::T,
            Profile::Q => SharingChannel::Q,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SharingChannel::D => "D-Analiz",
            SharingChannel::T => "T-Analiz",
            SharingChannel::Q => "Q-Analiz (RADAR)",
        }
    }
}

/// Per-channel sharing configuration. Loaded from `system_config`.
#[derive(Debug, Clone)]
pub struct SharingConfig {
    /// Regime → risk mode mapping.
    pub regime_risk_map: std::collections::HashMap<String, RiskMode>,
    /// Per-channel risk mode behavior.
    pub channel_behavior: std::collections::HashMap<(SharingChannel, RiskMode), RiskModeBehavior>,
    /// Guven multiplier in selective mode.
    pub selective_guven_mult: f64,
    /// Minimum guven for sharing (base threshold).
    pub min_share_guven: f64,
}

/// Decision output from the sharing evaluator.
#[derive(Debug, Clone)]
pub struct SharingDecision {
    pub channel: SharingChannel,
    pub should_share: bool,
    pub risk_mode: RiskMode,
    pub behavior: RiskModeBehavior,
    /// For Q: capital and quantity info.
    pub q_radar_capital: Option<QRadarShareInfo>,
}

/// Extra info attached to Q-RADAR shares.
#[derive(Debug, Clone)]
pub struct QRadarShareInfo {
    pub allocated_capital: f64,
    pub quantity: f64,
    pub avg_entry_price: f64,
}

/// Evaluate whether a setup should be shared on its channel.
///
/// Pure function — no DB, no I/O. The worker populates inputs.
pub fn evaluate_sharing(
    cfg: &SharingConfig,
    channel: SharingChannel,
    regime: &str,
    guven: f64,
    direction: &str,
    q_info: Option<QRadarShareInfo>,
) -> SharingDecision {
    let risk_mode = cfg
        .regime_risk_map
        .get(regime)
        .copied()
        .unwrap_or(RiskMode::RiskNeutral);

    let behavior = cfg
        .channel_behavior
        .get(&(channel, risk_mode))
        .copied()
        .unwrap_or(RiskModeBehavior::Selective);

    let should_share = match behavior {
        RiskModeBehavior::Active => guven >= cfg.min_share_guven,
        RiskModeBehavior::Selective => {
            let threshold = cfg.min_share_guven * cfg.selective_guven_mult;
            guven >= threshold
        }
        RiskModeBehavior::Continue => guven >= cfg.min_share_guven,
        RiskModeBehavior::Stopped => false,
    };

    // T-channel in risk-off: only long direction, and minimal.
    let should_share = if channel == SharingChannel::T
        && risk_mode == RiskMode::RiskOff
    {
        false
    } else if channel == SharingChannel::T
        && risk_mode == RiskMode::RiskNeutral
        && direction != "long"
    {
        false // T-nötr: only LONG
    } else {
        should_share
    };

    SharingDecision {
        channel,
        should_share,
        risk_mode,
        behavior,
        q_radar_capital: if channel == SharingChannel::Q { q_info } else { None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn test_cfg() -> SharingConfig {
        let mut regime_risk = HashMap::new();
        regime_risk.insert("trending_up".to_string(), RiskMode::RiskOn);
        regime_risk.insert("trending_down".to_string(), RiskMode::RiskOn);
        regime_risk.insert("ranging".to_string(), RiskMode::RiskNeutral);
        regime_risk.insert("volatile".to_string(), RiskMode::RiskOff);
        regime_risk.insert("uncertain".to_string(), RiskMode::RiskOff);

        let mut cb = HashMap::new();
        // D
        cb.insert((SharingChannel::D, RiskMode::RiskOn), RiskModeBehavior::Active);
        cb.insert((SharingChannel::D, RiskMode::RiskNeutral), RiskModeBehavior::Continue);
        cb.insert((SharingChannel::D, RiskMode::RiskOff), RiskModeBehavior::Selective);
        // T
        cb.insert((SharingChannel::T, RiskMode::RiskOn), RiskModeBehavior::Active);
        cb.insert((SharingChannel::T, RiskMode::RiskNeutral), RiskModeBehavior::Selective);
        cb.insert((SharingChannel::T, RiskMode::RiskOff), RiskModeBehavior::Stopped);
        // Q
        cb.insert((SharingChannel::Q, RiskMode::RiskOn), RiskModeBehavior::Active);
        cb.insert((SharingChannel::Q, RiskMode::RiskNeutral), RiskModeBehavior::Selective);
        cb.insert((SharingChannel::Q, RiskMode::RiskOff), RiskModeBehavior::Selective);

        SharingConfig {
            regime_risk_map: regime_risk,
            channel_behavior: cb,
            selective_guven_mult: 1.3,
            min_share_guven: 0.50,
        }
    }

    #[test]
    fn d_risk_on_shares() {
        let d = evaluate_sharing(&test_cfg(), SharingChannel::D, "trending_up", 0.55, "long", None);
        assert!(d.should_share);
        assert_eq!(d.risk_mode, RiskMode::RiskOn);
    }

    #[test]
    fn d_risk_off_selective_filters_weak() {
        // guven 0.55 < 0.50 * 1.3 = 0.65 → filtered
        let d = evaluate_sharing(&test_cfg(), SharingChannel::D, "volatile", 0.55, "long", None);
        assert!(!d.should_share);
    }

    #[test]
    fn d_risk_off_selective_passes_strong() {
        let d = evaluate_sharing(&test_cfg(), SharingChannel::D, "volatile", 0.70, "long", None);
        assert!(d.should_share);
    }

    #[test]
    fn t_risk_off_stopped() {
        let d = evaluate_sharing(&test_cfg(), SharingChannel::T, "volatile", 0.90, "long", None);
        assert!(!d.should_share);
    }

    #[test]
    fn t_risk_neutral_only_long() {
        let long = evaluate_sharing(&test_cfg(), SharingChannel::T, "ranging", 0.70, "long", None);
        assert!(long.should_share);
        let short = evaluate_sharing(&test_cfg(), SharingChannel::T, "ranging", 0.70, "short", None);
        assert!(!short.should_share);
    }

    #[test]
    fn q_risk_on_with_capital_info() {
        let info = QRadarShareInfo {
            allocated_capital: 150_000.0,
            quantity: 500.0,
            avg_entry_price: 300.0,
        };
        let d = evaluate_sharing(&test_cfg(), SharingChannel::Q, "trending_up", 0.60, "long", Some(info));
        assert!(d.should_share);
        assert!(d.q_radar_capital.is_some());
    }
}

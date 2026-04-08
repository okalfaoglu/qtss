//! `/v2/risk` wire types -- Faz 5 Adim (f).
//!
//! The Risk HUD shows the operator how close the account is to each
//! configured cap (drawdown, daily loss, leverage, open positions,
//! kill-switch). Each gauge carries the raw value, the cap, and a
//! 0..=1 utilization fraction so the React side just maps utilization
//! to colour without re-deriving math from raw fields.

use chrono::{DateTime, Utc};
use qtss_risk::{AccountState, RiskConfig};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// One cap-vs-current gauge. `utilization` is `value / cap` clamped to
/// `[0, 1]`. `breached` mirrors `utilization >= 1` so the frontend
/// does not re-implement the comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskGauge {
    pub label: String,
    pub value: Decimal,
    pub cap: Decimal,
    pub utilization: Decimal,
    pub breached: bool,
}

/// Whole `/v2/risk` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskHud {
    pub generated_at: DateTime<Utc>,
    pub kill_switch_manual: bool,
    /// True if any soft cap is breached (drawdown, daily loss,
    /// leverage, open positions). Renders the global red banner.
    pub any_breached: bool,
    /// True if the hard kill-switch drawdown is breached.
    pub kill_switch_armed: bool,
    pub gauges: Vec<RiskGauge>,
}

/// Pure builder. Pulls every gauge from the account snapshot + risk
/// config so the route handler stays a one-liner.
pub fn build_risk_hud(account: &AccountState, cfg: &RiskConfig) -> RiskHud {
    let drawdown = account.drawdown();
    let day_loss = account.day_loss_pct();
    let open = Decimal::from(account.open_positions);
    let max_open = Decimal::from(cfg.max_open_positions);

    let gauges = vec![
        gauge("drawdown", drawdown, cfg.max_drawdown),
        gauge("daily_loss", day_loss, cfg.max_daily_loss),
        gauge("leverage", account.current_leverage, cfg.max_leverage),
        gauge("open_positions", open, max_open),
        gauge("killswitch_drawdown", drawdown, cfg.killswitch_drawdown),
    ];

    let any_breached = gauges.iter().any(|g| g.breached && g.label != "killswitch_drawdown");
    let kill_switch_armed = gauges
        .iter()
        .find(|g| g.label == "killswitch_drawdown")
        .map(|g| g.breached)
        .unwrap_or(false);

    RiskHud {
        generated_at: Utc::now(),
        kill_switch_manual: account.kill_switch_manual,
        any_breached,
        kill_switch_armed,
        gauges,
    }
}

fn gauge(label: &str, value: Decimal, cap: Decimal) -> RiskGauge {
    let utilization = if cap > Decimal::ZERO {
        let raw = value / cap;
        raw.max(Decimal::ZERO).min(Decimal::ONE)
    } else {
        Decimal::ZERO
    };
    RiskGauge {
        label: label.into(),
        value,
        cap,
        utilization,
        breached: cap > Decimal::ZERO && value >= cap,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn cfg() -> RiskConfig {
        RiskConfig::defaults()
    }

    fn acc(equity: Decimal, peak: Decimal, day_pnl: Decimal, open: u32, lev: Decimal) -> AccountState {
        AccountState {
            equity,
            peak_equity: peak,
            day_pnl,
            open_positions: open,
            current_leverage: lev,
            kill_switch_manual: false,
        }
    }

    #[test]
    fn fresh_account_has_no_breaches() {
        let hud = build_risk_hud(&acc(dec!(10000), dec!(10000), dec!(0), 0, dec!(0)), &cfg());
        assert!(!hud.any_breached);
        assert!(!hud.kill_switch_armed);
        for g in &hud.gauges {
            assert!(!g.breached, "{} should be fine", g.label);
        }
    }

    #[test]
    fn drawdown_breach_flips_global_flag() {
        // Peak 10k, equity 9.4k -> 6% drawdown, max_drawdown is 5%.
        let hud = build_risk_hud(&acc(dec!(9400), dec!(10000), dec!(0), 0, dec!(0)), &cfg());
        assert!(hud.any_breached);
        assert!(!hud.kill_switch_armed);
        let dd = hud.gauges.iter().find(|g| g.label == "drawdown").unwrap();
        assert!(dd.breached);
    }

    #[test]
    fn killswitch_arms_only_on_hard_threshold() {
        // 9% drawdown -> killswitch_drawdown (8%) breached.
        let hud = build_risk_hud(&acc(dec!(9100), dec!(10000), dec!(0), 0, dec!(0)), &cfg());
        assert!(hud.kill_switch_armed);
        // Hard breach also implies the soft drawdown gauge is at 100%.
        let dd = hud.gauges.iter().find(|g| g.label == "drawdown").unwrap();
        assert_eq!(dd.utilization, Decimal::ONE);
    }

    #[test]
    fn open_positions_gauge_uses_count() {
        let mut a = acc(dec!(10000), dec!(10000), dec!(0), 8, dec!(1));
        a.kill_switch_manual = true;
        let hud = build_risk_hud(&a, &cfg());
        assert!(hud.kill_switch_manual);
        let g = hud.gauges.iter().find(|g| g.label == "open_positions").unwrap();
        assert!(g.breached);
    }

    #[test]
    fn json_round_trip() {
        let hud = build_risk_hud(&acc(dec!(10000), dec!(10000), dec!(-50), 1, dec!(0.5)), &cfg());
        let j = serde_json::to_string(&hud).unwrap();
        let back: RiskHud = serde_json::from_str(&j).unwrap();
        assert_eq!(back.gauges.len(), 5);
    }
}

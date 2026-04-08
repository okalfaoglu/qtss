//! Position sizing.
//!
//! `TradeIntent.sizing` is a `SizingHint` enum from `qtss-domain` —
//! the strategy declares *what* it wants, the risk layer turns that
//! into a concrete quantity. We dispatch one [`Sizer`] per hint variant
//! through a `HashMap` keyed by the variant tag string, so adding a new
//! sizing flavour is one register call (CLAUDE.md rule #1).

use crate::config::RiskConfig;
use crate::state::AccountState;
use qtss_domain::v2::intent::{RiskRejection, SizingHint, TradeIntent};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SizerOutput {
    /// Quantity in base-asset units, after caps applied by the sizer
    /// (per-trade risk, leverage, …). The engine still re-runs the
    /// post-sizing checks before approval.
    pub quantity: Decimal,
    /// Notes on any adjustments the sizer made (so the audit log can
    /// surface "sizer trimmed from 2.0 to 0.85 to honour leverage cap").
    pub adjustments: Vec<String>,
}

pub trait Sizer: Send + Sync {
    fn size(
        &self,
        intent: &TradeIntent,
        state: &AccountState,
        cfg: &RiskConfig,
    ) -> Result<SizerOutput, RiskRejection>;
}

/// Stable string tag for a `SizingHint` variant. Used as the dispatch
/// key. Centralised so the registry and the strategies agree.
pub fn hint_tag(hint: &SizingHint) -> &'static str {
    match hint {
        SizingHint::RiskPct { .. } => "risk_pct",
        SizingHint::Kelly => "kelly",
        SizingHint::FixedNotional { .. } => "fixed_notional",
        SizingHint::VolTarget => "vol_target",
    }
}

#[derive(Default)]
pub struct SizerRegistry {
    sizers: HashMap<&'static str, Arc<dyn Sizer>>,
}

impl SizerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tag: &'static str, sizer: Arc<dyn Sizer>) {
        self.sizers.insert(tag, sizer);
    }

    pub fn get(&self, tag: &str) -> Option<Arc<dyn Sizer>> {
        self.sizers.get(tag).cloned()
    }

    pub fn len(&self) -> usize {
        self.sizers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sizers.is_empty()
    }

    /// Convenience: register the built-in sizer set.
    pub fn with_defaults() -> Self {
        let mut r = Self::new();
        r.register("risk_pct", Arc::new(RiskPctSizer));
        r.register("fixed_notional", Arc::new(FixedNotionalSizer));
        r.register("vol_target", Arc::new(VolTargetSizer));
        r.register("kelly", Arc::new(KellySizer));
        r
    }
}

// ---------------------------------------------------------------------------
// RiskPct: risk N% of equity over the stop distance
// ---------------------------------------------------------------------------

pub struct RiskPctSizer;

impl Sizer for RiskPctSizer {
    fn size(
        &self,
        intent: &TradeIntent,
        state: &AccountState,
        cfg: &RiskConfig,
    ) -> Result<SizerOutput, RiskRejection> {
        let pct = match intent.sizing {
            SizingHint::RiskPct { pct } => pct,
            _ => return Err(RiskRejection::InvalidIntent("expected RiskPct".into())),
        };
        let entry = intent
            .entry_price
            .ok_or_else(|| RiskRejection::InvalidIntent("entry_price required".into()))?;
        let distance = (entry - intent.stop_loss).abs();
        if distance <= Decimal::ZERO {
            return Err(RiskRejection::StopDistanceTooSmall);
        }
        let mut adjustments = Vec::new();
        // Cap by per-trade risk regardless of what the strategy asked for.
        let effective_pct = if pct > cfg.max_risk_per_trade {
            adjustments.push(format!(
                "risk_pct trimmed from {pct} to {}",
                cfg.max_risk_per_trade
            ));
            cfg.max_risk_per_trade
        } else {
            pct
        };
        let risk_quote = state.equity * effective_pct;
        let quantity = risk_quote / distance;
        Ok(SizerOutput {
            quantity,
            adjustments,
        })
    }
}

// ---------------------------------------------------------------------------
// FixedNotional
// ---------------------------------------------------------------------------

pub struct FixedNotionalSizer;

impl Sizer for FixedNotionalSizer {
    fn size(
        &self,
        intent: &TradeIntent,
        _state: &AccountState,
        _cfg: &RiskConfig,
    ) -> Result<SizerOutput, RiskRejection> {
        let notional = match intent.sizing {
            SizingHint::FixedNotional { notional } => notional,
            _ => return Err(RiskRejection::InvalidIntent("expected FixedNotional".into())),
        };
        let entry = intent
            .entry_price
            .ok_or_else(|| RiskRejection::InvalidIntent("entry_price required".into()))?;
        if entry <= Decimal::ZERO {
            return Err(RiskRejection::InvalidIntent("entry_price must be > 0".into()));
        }
        Ok(SizerOutput {
            quantity: notional / entry,
            adjustments: Vec::new(),
        })
    }
}

// ---------------------------------------------------------------------------
// VolTarget — placeholder routing through the per-trade-risk cap.
// A full ATR-based implementation lands when the indicators crate
// produces realised vol on the same envelope.
// ---------------------------------------------------------------------------

pub struct VolTargetSizer;

impl Sizer for VolTargetSizer {
    fn size(
        &self,
        intent: &TradeIntent,
        state: &AccountState,
        cfg: &RiskConfig,
    ) -> Result<SizerOutput, RiskRejection> {
        // Until ATR is plumbed in, behave like RiskPct using the cap.
        let synthetic = TradeIntent {
            sizing: SizingHint::RiskPct {
                pct: cfg.max_risk_per_trade,
            },
            ..intent.clone()
        };
        let mut out = RiskPctSizer.size(&synthetic, state, cfg)?;
        out.adjustments
            .push("vol_target fell back to risk_pct cap".into());
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Kelly — same fallback shape; concrete implementation needs hit-rate
// stats from qtss-reporting.
// ---------------------------------------------------------------------------

pub struct KellySizer;

impl Sizer for KellySizer {
    fn size(
        &self,
        intent: &TradeIntent,
        state: &AccountState,
        cfg: &RiskConfig,
    ) -> Result<SizerOutput, RiskRejection> {
        let synthetic = TradeIntent {
            sizing: SizingHint::RiskPct {
                pct: cfg.max_risk_per_trade * dec!(0.25),
            },
            ..intent.clone()
        };
        let mut out = RiskPctSizer.size(&synthetic, state, cfg)?;
        out.adjustments
            .push("kelly fell back to quarter-kelly of cap".into());
        Ok(out)
    }
}

//! Risk engine. Walks every registered check, then dispatches the
//! intent to the right sizer, then re-applies the leverage cap on the
//! resulting notional. Returns `Result<ApprovedIntent, RiskRejection>`
//! so the caller can audit-log every rejection alongside approvals.

use crate::checks::RiskCheck;
use crate::config::RiskConfig;
use crate::error::RiskResult;
use crate::sizing::{hint_tag, SizerRegistry};
use crate::state::AccountState;
use chrono::Utc;
use qtss_domain::v2::intent::{ApprovedIntent, RiskRejection, TradeIntent};
use rust_decimal::Decimal;
use std::sync::Arc;
use uuid::Uuid;

pub struct RiskEngine {
    config: RiskConfig,
    checks: Vec<Arc<dyn RiskCheck>>,
    sizers: SizerRegistry,
}

impl RiskEngine {
    pub fn new(config: RiskConfig, sizers: SizerRegistry) -> RiskResult<Self> {
        config.validate()?;
        Ok(Self {
            config,
            checks: Vec::new(),
            sizers,
        })
    }

    pub fn config(&self) -> &RiskConfig {
        &self.config
    }

    pub fn register_check(&mut self, c: Arc<dyn RiskCheck>) {
        self.checks.push(c);
    }

    pub fn check_count(&self) -> usize {
        self.checks.len()
    }

    /// Walk checks → size → re-apply leverage cap → emit ApprovedIntent.
    pub fn approve(
        &self,
        intent: TradeIntent,
        state: &AccountState,
    ) -> Result<ApprovedIntent, RiskRejection> {
        let mut passed = Vec::with_capacity(self.checks.len());
        for c in &self.checks {
            c.evaluate(&intent, state, &self.config)?;
            passed.push(c.name().to_string());
        }

        let sizer = self
            .sizers
            .get(hint_tag(&intent.sizing))
            .ok_or_else(|| {
                RiskRejection::InvalidIntent(format!(
                    "no sizer registered for {}",
                    hint_tag(&intent.sizing)
                ))
            })?;
        let mut sized = sizer.size(&intent, state, &self.config)?;

        // Recompute notional and trim if leverage cap would be breached.
        let entry = intent
            .entry_price
            .ok_or_else(|| RiskRejection::InvalidIntent("entry_price required".into()))?;
        let notional = sized.quantity * entry;
        let max_notional = state.equity * self.config.max_leverage;
        let final_qty = if notional > max_notional && entry > Decimal::ZERO {
            sized
                .adjustments
                .push("quantity trimmed to honour max_leverage".into());
            max_notional / entry
        } else {
            sized.quantity
        };
        let final_notional = final_qty * entry;
        if final_qty <= Decimal::ZERO {
            return Err(RiskRejection::InvalidIntent(
                "sized quantity is zero or negative".into(),
            ));
        }

        Ok(ApprovedIntent {
            id: Uuid::new_v4(),
            approved_at: Utc::now(),
            intent,
            quantity: final_qty,
            notional: final_notional,
            checks_passed: passed,
            adjustments: sized.adjustments,
        })
    }
}

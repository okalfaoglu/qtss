//! Portfolio engine.
//!
//! Tracks per-instrument positions, aggregate equity, day pnl and peak
//! equity. Consumes fills from the execution layer and mark prices from
//! the market-data layer; produces an [`AccountState`] snapshot the
//! risk engine can gate on.
//!
//! Stays free of any IO — callers feed in fills/marks and pull the
//! snapshot when they need it.

use crate::position::Position;
use chrono::{DateTime, Datelike, Utc};
use qtss_domain::v2::intent::Side;
use qtss_risk::AccountState;
use rust_decimal::Decimal;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PortfolioConfig {
    /// Starting equity in quote currency. Defines the day's reference
    /// for `day_pnl` book-keeping until the next rollover.
    pub starting_equity: Decimal,
}

pub struct PortfolioEngine {
    config: PortfolioConfig,
    positions: HashMap<String, Position>,
    cash: Decimal,
    realised_today: Decimal,
    peak_equity: Decimal,
    /// Equity at the start of the current trading day, used to compute
    /// `day_pnl` independently of intra-day fills.
    day_anchor: Decimal,
    /// Day boundary the anchor was last set at.
    day_anchor_at: DateTime<Utc>,
    kill_switch_manual: bool,
}

impl PortfolioEngine {
    pub fn new(config: PortfolioConfig) -> Self {
        let now = Utc::now();
        Self {
            cash: config.starting_equity,
            realised_today: Decimal::ZERO,
            peak_equity: config.starting_equity,
            day_anchor: config.starting_equity,
            day_anchor_at: now,
            positions: HashMap::new(),
            kill_switch_manual: false,
            config,
        }
    }

    pub fn config(&self) -> &PortfolioConfig {
        &self.config
    }

    pub fn position(&self, symbol: &str) -> Option<&Position> {
        self.positions.get(symbol)
    }

    pub fn positions(&self) -> impl Iterator<Item = &Position> {
        self.positions.values()
    }

    pub fn open_count(&self) -> u32 {
        self.positions.values().filter(|p| !p.is_flat()).count() as u32
    }

    pub fn set_kill_switch(&mut self, on: bool) {
        self.kill_switch_manual = on;
    }

    /// Apply a single fill. The caller has already authenticated the
    /// fill against the originating order — we just net it.
    pub fn apply_fill(&mut self, symbol: &str, side: Side, qty: Decimal, price: Decimal, fee: Decimal) {
        let pos = self
            .positions
            .entry(symbol.to_string())
            .or_insert_with(|| Position::flat(symbol));
        let realised = pos.apply_fill(side, qty, price, fee);
        self.realised_today += realised;
        // Cash moves opposite to the trade direction (buy debits cash,
        // sell credits cash). Fees always debit cash.
        let notional = qty * price;
        self.cash += match side {
            Side::Long => -notional,
            Side::Short => notional,
        } - fee;
        self.refresh_peak();
    }

    /// Push a mark price into a single instrument; refreshes its
    /// unrealised pnl and the engine's peak equity.
    pub fn mark(&mut self, symbol: &str, price: Decimal) {
        if let Some(pos) = self.positions.get_mut(symbol) {
            pos.mark(price);
        }
        self.refresh_peak();
    }

    /// Bulk mark — convenience for the market-data feed.
    pub fn mark_all(&mut self, prices: &HashMap<String, Decimal>) {
        for (sym, p) in prices {
            if let Some(pos) = self.positions.get_mut(sym) {
                pos.mark(*p);
            }
        }
        self.refresh_peak();
    }

    /// Total equity = cash + unrealised pnl across all positions.
    /// (Realised pnl is already baked into cash.)
    pub fn equity(&self) -> Decimal {
        let unreal: Decimal = self.positions.values().map(|p| p.unrealised_pnl).sum();
        self.cash + unreal
    }

    /// Day PnL = current equity − anchor at start of trading day.
    pub fn day_pnl(&self) -> Decimal {
        self.equity() - self.day_anchor
    }

    fn refresh_peak(&mut self) {
        let eq = self.equity();
        if eq > self.peak_equity {
            self.peak_equity = eq;
        }
    }

    /// Reset the day anchor. Called by the scheduler at UTC midnight,
    /// or by tests directly. Idempotent within the same UTC date.
    pub fn rollover_day(&mut self, now: DateTime<Utc>) {
        if now.date_naive() == self.day_anchor_at.date_naive() {
            return;
        }
        self.day_anchor = self.equity();
        self.day_anchor_at = now;
        self.realised_today = Decimal::ZERO;
    }

    /// Build the AccountState that the risk engine consumes.
    /// Aggregate gross notional / equity gives the leverage figure.
    pub fn snapshot(&self) -> AccountState {
        let equity = self.equity();
        let gross_notional: Decimal = self
            .positions
            .values()
            .filter(|p| !p.is_flat())
            .map(|p| p.last_mark.max(p.avg_entry) * p.quantity)
            .sum();
        let leverage = if equity > Decimal::ZERO {
            gross_notional / equity
        } else {
            Decimal::ZERO
        };
        AccountState {
            equity,
            peak_equity: self.peak_equity,
            day_pnl: self.day_pnl(),
            open_positions: self.open_count(),
            current_leverage: leverage,
            kill_switch_manual: self.kill_switch_manual,
        }
    }
}

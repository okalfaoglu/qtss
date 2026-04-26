//! Trade lifecycle — the canonical state machine for an IQ-D / IQ-T
//! position from entry to exit.
//!
//! Lifecycle:
//!
//!   Pending → Open → (TP1 hit) → ScalingOut → (TP2 hit) → ScalingOut
//!     │              │
//!     │              └→ (SL hit / timeout / trailing) → Closed
//!     └→ (entry slippage > config) → Aborted
//!
//! Each state transition is captured with timestamp + bar_index +
//! price so post-hoc attribution can replay the exact path.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::config::IqPolarity;
use super::cost::FillCost;

/// Stage of a trade. `Open` and `ScalingOut` are the only states
/// that consume bars from the replay engine; the others are terminal
/// (terminal states get a single timestamp recorded then never
/// updated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeState {
    Pending,
    Open,
    ScalingOut,
    Closed,
    Aborted,
}

/// Why did the trade close? Used both as a state transition cause
/// and as an attribution category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeOutcome {
    /// Hit the final TP level (TP3 or last specified TP).
    TakeProfitFull,
    /// Hit at least one TP but stopped out before the final.
    TakeProfitPartial,
    /// Hit the stop loss without hitting any TP.
    StopLoss,
    /// Hit the trailing stop after a favourable excursion.
    TrailingStop,
    /// Held longer than `max_holding_bars` without resolution.
    Timeout,
    /// External invalidation (Wyckoff event flip / Elliott
    /// invalidation / regime change). v1 not wired yet — placeholder.
    Invalidated,
    /// Trade was aborted before opening (slippage too high, etc.).
    Aborted,
}

/// Snapshot of how the trade was doing at a particular bar — used
/// for path analysis (max favourable / max adverse excursion, time
/// to first TP, etc.).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathSnapshot {
    pub bar_index: usize,
    pub time: DateTime<Utc>,
    pub price: Decimal,
    pub unrealised_pnl_pct: f64,
    pub bars_held: u32,
}

/// Single trade record — output by the runner, consumed by the
/// attribution + report passes. JSON-serialisable so the trade log
/// writes one Trade per line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IqTrade {
    pub trade_id: Uuid,
    pub run_tag: String,
    pub polarity: IqPolarity,

    pub symbol: String,
    pub timeframe: String,
    pub exchange: String,
    pub segment: String,

    pub state: TradeState,

    // ── Entry context ─────────────────────────────────────────────
    pub entry_bar: usize,
    pub entry_time: DateTime<Utc>,
    pub entry_price: Decimal,
    /// All 10 component scores at the moment the setup fired —
    /// post-hoc attribution can correlate "weak fib" with "loss".
    pub entry_components: serde_json::Value,
    pub entry_composite_score: f64,
    pub wyckoff_event_at_entry: Option<String>,
    pub elliott_wave_at_entry: Option<String>,
    pub cycle_phase_at_entry: Option<String>,
    pub cycle_source_at_entry: Option<String>,
    pub regime_at_entry: Option<String>,

    // ── Targets ───────────────────────────────────────────────────
    pub stop_loss: Decimal,
    pub take_profits: Vec<Decimal>,
    pub initial_qty: Decimal,
    pub remaining_qty: Decimal,

    // ── Path tracking ─────────────────────────────────────────────
    pub max_favorable_pct: f64,
    pub max_adverse_pct: f64,
    pub bars_held: u32,
    pub snapshots: Vec<PathSnapshot>,

    // ── Exit context ──────────────────────────────────────────────
    pub exit_bar: Option<usize>,
    pub exit_time: Option<DateTime<Utc>>,
    pub exit_price: Option<Decimal>,
    pub outcome: Option<TradeOutcome>,
    pub regime_at_exit: Option<String>,

    // ── PnL + cost ────────────────────────────────────────────────
    pub gross_pnl: Decimal,
    pub net_pnl: Decimal,
    pub net_pnl_pct: f64,
    pub costs: FillCost,
    /// Tier-by-tier realised PnL (TP1 partial fill, TP2 partial,
    /// final exit). `tp_realised[i]` = PnL realised on
    /// `take_profits[i]`. Last entry covers the SL / trailing /
    /// timeout fill.
    pub tier_pnls: Vec<Decimal>,
}

impl IqTrade {
    /// Construct a new trade in the `Pending` state. The runner
    /// transitions it to `Open` once it confirms entry (next bar in
    /// most modes, immediate at close in others).
    pub fn pending(
        run_tag: impl Into<String>,
        polarity: IqPolarity,
        symbol: impl Into<String>,
        timeframe: impl Into<String>,
        exchange: impl Into<String>,
        segment: impl Into<String>,
        entry_bar: usize,
        entry_time: DateTime<Utc>,
        entry_price: Decimal,
        stop_loss: Decimal,
        take_profits: Vec<Decimal>,
        initial_qty: Decimal,
        entry_components: serde_json::Value,
        entry_composite_score: f64,
    ) -> Self {
        Self {
            trade_id: Uuid::new_v4(),
            run_tag: run_tag.into(),
            polarity,
            symbol: symbol.into(),
            timeframe: timeframe.into(),
            exchange: exchange.into(),
            segment: segment.into(),
            state: TradeState::Pending,
            entry_bar,
            entry_time,
            entry_price,
            entry_components,
            entry_composite_score,
            wyckoff_event_at_entry: None,
            elliott_wave_at_entry: None,
            cycle_phase_at_entry: None,
            cycle_source_at_entry: None,
            regime_at_entry: None,
            stop_loss,
            take_profits: take_profits.clone(),
            initial_qty,
            remaining_qty: initial_qty,
            max_favorable_pct: 0.0,
            max_adverse_pct: 0.0,
            bars_held: 0,
            snapshots: Vec::new(),
            exit_bar: None,
            exit_time: None,
            exit_price: None,
            outcome: None,
            regime_at_exit: None,
            gross_pnl: Decimal::ZERO,
            net_pnl: Decimal::ZERO,
            net_pnl_pct: 0.0,
            costs: FillCost::zero(),
            tier_pnls: vec![Decimal::ZERO; take_profits.len() + 1],
        }
    }

    /// Did this trade lose money on a NET basis (after costs)?
    pub fn is_loss(&self) -> bool {
        matches!(self.state, TradeState::Closed) && self.net_pnl < Decimal::ZERO
    }

    /// Update path stats given the latest mark-to-market price.
    pub fn observe_path(
        &mut self,
        bar_index: usize,
        time: DateTime<Utc>,
        price: Decimal,
        long: bool,
    ) {
        use rust_decimal::prelude::ToPrimitive;
        let entry = self
            .entry_price
            .to_f64()
            .unwrap_or(0.0)
            .max(f64::MIN_POSITIVE);
        let p = price.to_f64().unwrap_or(0.0);
        let unrealised = if long {
            (p - entry) / entry * 100.0
        } else {
            (entry - p) / entry * 100.0
        };
        if unrealised > self.max_favorable_pct {
            self.max_favorable_pct = unrealised;
        }
        if unrealised < self.max_adverse_pct {
            self.max_adverse_pct = unrealised;
        }
        self.bars_held = self.bars_held.saturating_add(1);
        // Caller decides whether to push a snapshot (depends on
        // `path_snapshot_every_bars`); we expose `current_unrealised`
        // for them but don't push automatically.
        let _ = (bar_index, time);
    }

    pub fn push_snapshot(
        &mut self,
        bar_index: usize,
        time: DateTime<Utc>,
        price: Decimal,
    ) {
        use rust_decimal::prelude::ToPrimitive;
        let entry = self
            .entry_price
            .to_f64()
            .unwrap_or(0.0)
            .max(f64::MIN_POSITIVE);
        let p = price.to_f64().unwrap_or(0.0);
        let long = matches!(self.polarity, IqPolarity::Dip);
        let pct = if long {
            (p - entry) / entry * 100.0
        } else {
            (entry - p) / entry * 100.0
        };
        self.snapshots.push(PathSnapshot {
            bar_index,
            time,
            price,
            unrealised_pnl_pct: pct,
            bars_held: self.bars_held,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn fixture() -> IqTrade {
        IqTrade::pending(
            "test",
            IqPolarity::Dip,
            "BTCUSDT",
            "4h",
            "binance",
            "futures",
            100,
            Utc::now(),
            dec!(50000),
            dec!(48000),
            vec![dec!(52000), dec!(54000), dec!(56000)],
            dec!(0.1),
            json!({}),
            0.7,
        )
    }

    #[test]
    fn pending_trade_has_remaining_qty_equal_initial() {
        let t = fixture();
        assert_eq!(t.remaining_qty, t.initial_qty);
        assert_eq!(t.tier_pnls.len(), 4); // 3 TPs + final
    }

    #[test]
    fn observe_path_tracks_mfe_mae() {
        let mut t = fixture();
        t.observe_path(101, Utc::now(), dec!(52000), true); // +4%
        t.observe_path(102, Utc::now(), dec!(49000), true); // -2%
        t.observe_path(103, Utc::now(), dec!(53000), true); // +6%
        assert!((t.max_favorable_pct - 6.0).abs() < 0.01);
        assert!((t.max_adverse_pct - (-2.0)).abs() < 0.01);
    }

    #[test]
    fn short_trade_inverts_pnl_direction() {
        let mut t = fixture();
        t.polarity = IqPolarity::Top;
        t.observe_path(101, Utc::now(), dec!(48000), false); // +4%
        t.observe_path(102, Utc::now(), dec!(51000), false); // -2%
        assert!((t.max_favorable_pct - 4.0).abs() < 0.01);
        assert!((t.max_adverse_pct - (-2.0)).abs() < 0.01);
    }

    #[test]
    fn snapshot_records_unrealised_pct() {
        let mut t = fixture();
        t.push_snapshot(105, Utc::now(), dec!(53000));
        let snap = &t.snapshots[0];
        assert!((snap.unrealised_pnl_pct - 6.0).abs() < 0.01);
        assert_eq!(snap.bar_index, 105);
    }
}

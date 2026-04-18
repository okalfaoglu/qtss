//! Faz 9.7.3 — Setup lifecycle: event kinds, transition detector,
//! and dispatch router. Called every tick by the setup watcher.
//!
//! CLAUDE.md #1 — event handling uses a dispatch table (`Vec<Box<dyn
//! LifecycleHandler>>`) rather than an if/else chain. New side-effects
//! (Telegram, X, webhook, audit, metrics) are plugged in by adding a
//! handler impl, not by editing a switch.
//!
//! CLAUDE.md #3 — the detector is a pure function over
//! `(SetupState, PriceTick)`; it does not know about DB, Telegram, or
//! the outbox. The handlers do the I/O.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::card::builder::SetupDirection;
use crate::health::{HealthBand, HealthScore};
use crate::price_tick::PriceTick;

// ---------------------------------------------------------------------------
// Event kinds
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleEventKind {
    EntryTouched,
    TpHit,
    TpPartial,
    TpFinal,
    SlHit,
    SlRatcheted,
    Invalidated,
    Cancelled,
    HealthWarn,
    HealthDanger,
}

impl LifecycleEventKind {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::EntryTouched => "entry_touched",
            Self::TpHit => "tp_hit",
            Self::TpPartial => "tp_partial",
            Self::TpFinal => "tp_final",
            Self::SlHit => "sl_hit",
            Self::SlRatcheted => "sl_ratcheted",
            Self::Invalidated => "invalidated",
            Self::Cancelled => "cancelled",
            Self::HealthWarn => "health_warn",
            Self::HealthDanger => "health_danger",
        }
    }

    /// Terminal events end the setup and trigger close accounting.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::TpFinal | Self::SlHit | Self::Invalidated | Self::Cancelled)
    }

    /// The `close_reason` to stamp on `qtss_setups` when terminal.
    /// Non-terminal kinds return `None`.
    pub fn close_reason(self) -> Option<&'static str> {
        match self {
            Self::TpFinal => Some("tp_final"),
            Self::SlHit => Some("sl_hit"),
            Self::Invalidated => Some("invalidated"),
            Self::Cancelled => Some("cancelled"),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Setup state (watcher view)
// ---------------------------------------------------------------------------

/// Minimal view the detector needs. Built from `qtss_setups` row +
/// the last-seen price tick. `current_sl` reflects Poz Koruma ratchet
/// when 9.7.4 lands; for 9.7.3 it's just the original SL.
#[derive(Debug, Clone)]
pub struct WatcherSetupState {
    pub setup_id: Uuid,
    pub exchange: String,
    pub symbol: String,
    pub direction: SetupDirection,
    pub entry_price: Decimal,
    pub current_sl: Decimal,
    pub tp_prices: [Option<Decimal>; 3], // TP1, TP2, TP3
    pub tp_hits_bitmap: u8,              // bit0=TP1, bit1=TP2, bit2=TP3
    pub entry_touched: bool,
    pub opened_at: DateTime<Utc>,
}

impl WatcherSetupState {
    fn tp_hit(&self, idx_zero_based: usize) -> bool {
        (self.tp_hits_bitmap >> idx_zero_based) & 1 == 1
    }

    /// Direction-aware "did the price reach/exceed `target`?".
    fn reached(&self, price: Decimal, target: Decimal) -> bool {
        match self.direction {
            SetupDirection::Long => price >= target,
            SetupDirection::Short => price <= target,
        }
    }

    fn pnl_pct(&self, price: Decimal) -> Option<f64> {
        let entry = self.entry_price.to_f64()?;
        if entry.abs() < 1e-12 {
            return Some(0.0);
        }
        let px = price.to_f64()?;
        let raw = (px - entry) / entry * 100.0;
        Some(match self.direction {
            SetupDirection::Long => raw,
            SetupDirection::Short => -raw,
        })
    }
}

// ---------------------------------------------------------------------------
// Transition detector (pure)
// ---------------------------------------------------------------------------

/// Output of the detector — a decision + ancillary data needed by the
/// router. The setup watcher applies them in order; the router
/// collapses multi-TP frames into a single terminal event when
/// appropriate.
#[derive(Debug, Clone, PartialEq)]
pub struct LifecycleDecision {
    pub kind: LifecycleEventKind,
    /// For TP events: which level (1..=3). `None` otherwise.
    pub tp_index: Option<u8>,
    pub price: Decimal,
}

/// Detect the set of transitions triggered by `tick`. The detector is
/// order-preserving — events come back in the order: entry → TP1 →
/// TP2 → TP3 → SL (but SL short-circuits once detected, since it's
/// terminal for the untraded-through side). Health-band transitions
/// are emitted by the watcher, not here.
pub fn detect_transitions(
    state: &WatcherSetupState,
    tick: &PriceTick,
) -> Vec<LifecycleDecision> {
    let mut out: Vec<LifecycleDecision> = Vec::new();
    // Use mid for fairness; caller can decide to use bid/ask instead.
    let price = tick.mid();

    // 1. Entry touch.
    if !state.entry_touched && state.reached(price, state.entry_price) {
        out.push(LifecycleDecision {
            kind: LifecycleEventKind::EntryTouched,
            tp_index: None,
            price,
        });
    }

    // 2. SL — terminal. We emit it alone if fired (no mixing with TP
    //    in the same frame: if the tick crossed both, the stop-first
    //    convention is safer for the user).
    if state.reached_sl(price) {
        out.push(LifecycleDecision {
            kind: LifecycleEventKind::SlHit,
            tp_index: None,
            price,
        });
        return out;
    }

    // 3. TP levels — emit `TpHit` for each newly-reached level. The
    //    watcher decides tp_partial vs tp_final via Smart Target AI
    //    (Faz 9.7.4). In 9.7.3 the router defaults the *final* TP
    //    slot (highest-indexed TP present) to `TpFinal`.
    for (i, maybe_tp) in state.tp_prices.iter().enumerate() {
        let Some(tp) = maybe_tp else { continue };
        if state.tp_hit(i) {
            continue;
        }
        if state.reached(price, *tp) {
            out.push(LifecycleDecision {
                kind: LifecycleEventKind::TpHit,
                tp_index: Some((i as u8) + 1),
                price,
            });
        }
    }

    out
}

impl WatcherSetupState {
    fn reached_sl(&self, price: Decimal) -> bool {
        match self.direction {
            SetupDirection::Long => price <= self.current_sl,
            SetupDirection::Short => price >= self.current_sl,
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch router
// ---------------------------------------------------------------------------

/// Context passed to every handler — the full picture of a single
/// lifecycle event at emission time.
#[derive(Debug, Clone)]
pub struct LifecycleContext {
    pub setup_id: Uuid,
    pub kind: LifecycleEventKind,
    pub price: Decimal,
    pub tp_index: Option<u8>,
    pub pnl_pct: Option<f64>,
    pub pnl_r: Option<f64>,
    pub health: Option<HealthScore>,
    pub prev_health_band: Option<HealthBand>,
    pub duration_ms: Option<i64>,
    pub emitted_at: DateTime<Utc>,
    // Setup metadata snapshot — threaded so renderers avoid DB reads.
    pub exchange: String,
    pub symbol: String,
    pub direction: SetupDirection,
    pub entry_price: Decimal,
    pub current_sl: Decimal,
    /// Smart Target AI decision captured when this event was emitted
    /// (Faz 9.7.4). Populated only on `TpHit`/`TpPartial`/`TpFinal`
    /// branches that went through the evaluator; `None` otherwise.
    pub ai_action: Option<String>,
    pub ai_reasoning: Option<String>,
    pub ai_confidence: Option<f64>,
}

/// A side-effect receiver for lifecycle events. Implementations:
/// DB-persist (9.7.3), Telegram renderer (9.7.5), X outbox (9.7.6),
/// metrics counter (future), LLM-judge replay (future).
#[async_trait]
pub trait LifecycleHandler: Send + Sync {
    async fn on_event(&self, ctx: &LifecycleContext);
    fn name(&self) -> &'static str;
}

/// Holds the handler registry. Cheap to clone (inner Arc). Event
/// handlers are invoked sequentially per event; use concurrent
/// broadcast inside a handler if you need fan-out.
#[derive(Clone, Default)]
pub struct LifecycleRouter {
    handlers: Arc<Vec<Arc<dyn LifecycleHandler>>>,
}

impl LifecycleRouter {
    pub fn new(handlers: Vec<Arc<dyn LifecycleHandler>>) -> Self {
        Self { handlers: Arc::new(handlers) }
    }

    pub fn handler_names(&self) -> Vec<&'static str> {
        self.handlers.iter().map(|h| h.name()).collect()
    }

    pub async fn dispatch(&self, ctx: &LifecycleContext) {
        for h in self.handlers.iter() {
            h.on_event(ctx).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers the watcher uses to build contexts.
// ---------------------------------------------------------------------------

/// Build a context given a detector decision + state. Computes PnL.
pub fn make_context(
    state: &WatcherSetupState,
    decision: &LifecycleDecision,
    health: Option<HealthScore>,
    prev_health_band: Option<HealthBand>,
    now: DateTime<Utc>,
) -> LifecycleContext {
    let pnl_pct = state.pnl_pct(decision.price);
    let duration_ms = Some((now - state.opened_at).num_milliseconds().max(0));
    LifecycleContext {
        setup_id: state.setup_id,
        kind: decision.kind,
        price: decision.price,
        tp_index: decision.tp_index,
        pnl_pct,
        pnl_r: None, // filled when risk is known (9.7.4)
        health,
        prev_health_band,
        duration_ms,
        emitted_at: now,
        ai_action: None,
        ai_reasoning: None,
        ai_confidence: None,
        exchange: state.exchange.clone(),
        symbol: state.symbol.clone(),
        direction: state.direction,
        entry_price: state.entry_price,
        current_sl: state.current_sl,
    }
}

/// Promote a `TpHit` to `TpFinal` when it's the last TP slot present
/// OR to `TpPartial` otherwise. Faz 9.7.4 will replace this default
/// with a Smart Target decision. Idempotent / pure.
pub fn promote_tp_hit(
    state: &WatcherSetupState,
    decision: LifecycleDecision,
) -> LifecycleDecision {
    if decision.kind != LifecycleEventKind::TpHit {
        return decision;
    }
    let Some(idx) = decision.tp_index else {
        return decision;
    };
    // Is there a higher-index TP defined? If not, this is the final.
    let has_higher = state.tp_prices[(idx as usize)..]
        .iter()
        .any(|t| t.is_some());
    let kind = if has_higher {
        LifecycleEventKind::TpPartial
    } else {
        LifecycleEventKind::TpFinal
    };
    LifecycleDecision { kind, ..decision }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn state_long() -> WatcherSetupState {
        WatcherSetupState {
            setup_id: Uuid::new_v4(),
            exchange: "binance".into(),
            symbol: "BTCUSDT".into(),
            direction: SetupDirection::Long,
            entry_price: dec!(100),
            current_sl: dec!(95),
            tp_prices: [Some(dec!(105)), Some(dec!(110)), Some(dec!(120))],
            tp_hits_bitmap: 0,
            entry_touched: false,
            opened_at: Utc::now(),
        }
    }

    fn tick_at(px_bid: Decimal, px_ask: Decimal) -> PriceTick {
        PriceTick { bid: px_bid, ask: px_ask, update_id: 1, received_at: Utc::now() }
    }

    #[test]
    fn long_entry_touch_and_tp1() {
        let mut s = state_long();
        let t = tick_at(dec!(106), dec!(106)); // mid=106 ≥ 105
        let out = detect_transitions(&s, &t);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].kind, LifecycleEventKind::EntryTouched);
        assert_eq!(out[1].kind, LifecycleEventKind::TpHit);
        assert_eq!(out[1].tp_index, Some(1));
        // Already-touched entry should not repeat.
        s.entry_touched = true;
        let out2 = detect_transitions(&s, &t);
        assert_eq!(out2.len(), 1);
        assert_eq!(out2[0].kind, LifecycleEventKind::TpHit);
    }

    #[test]
    fn long_sl_is_terminal_and_short_circuits_tp() {
        let s = state_long();
        // Tick that would technically also be below TP but crashes under SL.
        let t = tick_at(dec!(90), dec!(90));
        let out = detect_transitions(&s, &t);
        // EntryTouched fires (since 90 passed 100 going down? No — LONG entry
        // requires price >= entry. So only SL fires.)
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, LifecycleEventKind::SlHit);
    }

    #[test]
    fn short_direction_inverts_comparisons() {
        let mut s = state_long();
        s.direction = SetupDirection::Short;
        s.entry_price = dec!(100);
        s.current_sl = dec!(105);
        s.tp_prices = [Some(dec!(95)), Some(dec!(90)), None];
        s.entry_touched = true;
        // price falls to 92 → TP1 (95) hit, TP2 (90) not yet.
        let t = tick_at(dec!(92), dec!(92));
        let out = detect_transitions(&s, &t);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, LifecycleEventKind::TpHit);
        assert_eq!(out[0].tp_index, Some(1));
    }

    #[test]
    fn promote_tp_to_final_when_last_slot() {
        let mut s = state_long();
        // TP3 is defined; TP1 hit → still partial.
        let d = LifecycleDecision {
            kind: LifecycleEventKind::TpHit,
            tp_index: Some(1),
            price: dec!(105),
        };
        assert_eq!(promote_tp_hit(&s, d.clone()).kind, LifecycleEventKind::TpPartial);

        // Only TP1 defined → it IS the final.
        s.tp_prices = [Some(dec!(105)), None, None];
        assert_eq!(promote_tp_hit(&s, d).kind, LifecycleEventKind::TpFinal);
    }

    #[test]
    fn terminal_and_close_reason_mapping() {
        assert_eq!(LifecycleEventKind::TpFinal.close_reason(), Some("tp_final"));
        assert_eq!(LifecycleEventKind::SlHit.close_reason(), Some("sl_hit"));
        assert_eq!(LifecycleEventKind::TpHit.close_reason(), None);
        assert!(LifecycleEventKind::TpFinal.is_terminal());
        assert!(!LifecycleEventKind::TpPartial.is_terminal());
    }
}

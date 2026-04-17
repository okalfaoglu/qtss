//! Faz 9.8.0 — In-memory tick-driven store of broker-filled positions.
//!
//! Sibling of `PriceTickStore` (which tracks setup-level entry/SL/TP)
//! but for positions that have actually been filled at a venue. Ticks
//! from bookTicker/markPrice/userData streams drive evaluation of
//! liquidation guard, scale manager, ratchet, and tp engine.
//!
//! Fully in-memory; persistence lives in `live_positions` /
//! `position_scale_events` / `liquidation_guard_events` tables. The
//! store is rehydrated on worker start from open rows.

use std::collections::HashMap;
use std::sync::RwLock;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifies a position uniquely inside the store.
pub type PositionId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExecutionMode {
    Dry,
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PositionSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TpLeg {
    pub price: Decimal,
    pub qty: Decimal,
    pub filled_qty: Decimal,
}

/// Per-position mutable state kept in memory and synced to DB on change.
#[derive(Debug, Clone)]
pub struct LivePositionState {
    pub id: PositionId,
    pub setup_id: Option<Uuid>,
    pub mode: ExecutionMode,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub side: PositionSide,
    pub leverage: u8,
    pub entry_avg: Decimal,
    pub qty_filled: Decimal,
    pub qty_remaining: Decimal,
    pub current_sl: Option<Decimal>,
    pub tp_ladder: Vec<TpLeg>,
    pub liquidation_price: Option<Decimal>,
    pub maint_margin_ratio: Option<Decimal>,
    pub funding_rate_next: Option<Decimal>,
    pub last_mark: Option<Decimal>,
    pub last_tick_at: Option<DateTime<Utc>>,
    pub opened_at: DateTime<Utc>,
}

/// Key under which positions are indexed for tick fan-out. A single
/// (mode, exchange, symbol) may host multiple positions (e.g. split
/// between several setups sharing the same venue symbol).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TickKey {
    pub mode: ExecutionMode,
    pub exchange: String,
    pub symbol: String,
}

/// In-memory store. Reads are cheap (RwLock read); writes happen on
/// fill / scale / ratchet / close.
#[derive(Default)]
pub struct LivePositionStore {
    by_id: RwLock<HashMap<PositionId, LivePositionState>>,
    by_tick: RwLock<HashMap<TickKey, Vec<PositionId>>>,
}

impl LivePositionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a position (used on hydrate + fill).
    pub fn upsert(&self, state: LivePositionState) {
        let key = TickKey {
            mode: state.mode,
            exchange: state.exchange.clone(),
            symbol: state.symbol.clone(),
        };
        let id = state.id;
        {
            let mut m = self.by_id.write().expect("live_position_store poisoned");
            m.insert(id, state);
        }
        let mut idx = self.by_tick.write().expect("live_position_store poisoned");
        let entry = idx.entry(key).or_default();
        if !entry.contains(&id) {
            entry.push(id);
        }
    }

    /// Update the last mark from a bookTicker / markPrice tick.
    /// Returns the list of position ids that should be re-evaluated.
    pub fn update_mark(&self, key: &TickKey, price: Decimal, at: DateTime<Utc>) -> Vec<PositionId> {
        let ids = {
            let idx = self.by_tick.read().expect("live_position_store poisoned");
            idx.get(key).cloned().unwrap_or_default()
        };
        if ids.is_empty() {
            return ids;
        }
        let mut m = self.by_id.write().expect("live_position_store poisoned");
        for id in &ids {
            if let Some(state) = m.get_mut(id) {
                state.last_mark = Some(price);
                state.last_tick_at = Some(at);
            }
        }
        ids
    }

    pub fn get(&self, id: PositionId) -> Option<LivePositionState> {
        self.by_id
            .read()
            .expect("live_position_store poisoned")
            .get(&id)
            .cloned()
    }

    pub fn remove(&self, id: PositionId) {
        let state = {
            let mut m = self.by_id.write().expect("live_position_store poisoned");
            m.remove(&id)
        };
        if let Some(s) = state {
            let key = TickKey {
                mode: s.mode,
                exchange: s.exchange,
                symbol: s.symbol,
            };
            let mut idx = self.by_tick.write().expect("live_position_store poisoned");
            if let Some(v) = idx.get_mut(&key) {
                v.retain(|x| *x != id);
                if v.is_empty() {
                    idx.remove(&key);
                }
            }
        }
    }

    pub fn open_count(&self) -> usize {
        self.by_id
            .read()
            .expect("live_position_store poisoned")
            .len()
    }
}

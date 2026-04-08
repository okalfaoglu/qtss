//! Canonical topic names. Adding a new topic = adding one constant here.
//!
//! Keeping every topic name in one file makes it trivial to grep for
//! producers and consumers and prevents typos in distant call sites.

// ---------- Market data ----------
pub const BAR_CLOSED: &str = "bar.closed";
pub const BAR_LIVE: &str = "bar.live";
pub const TICK_TRADE: &str = "tick.trade";
pub const BOOK_L2_UPDATE: &str = "book.l2.update";

// ---------- Foundation layer ----------
pub const PIVOT_UPDATED: &str = "pivot.updated";
pub const REGIME_CHANGED: &str = "regime.changed";

// ---------- Pattern layer ----------
pub const PATTERN_DETECTED: &str = "pattern.detected";
pub const PATTERN_VALIDATED: &str = "pattern.validated";
pub const PATTERN_INVALIDATED: &str = "pattern.invalidated";
pub const TARGET_COMPUTED: &str = "target.computed";

// ---------- Forecast / scenario ----------
pub const SCENARIO_UPDATED: &str = "scenario.updated";
pub const FORECAST_UPDATED: &str = "forecast.updated";

// ---------- Strategy / risk / execution ----------
pub const SIGNAL_ENVELOPE: &str = "signal.envelope";
pub const INTENT_CREATED: &str = "intent.created";
pub const INTENT_APPROVED: &str = "intent.approved";
pub const INTENT_REJECTED: &str = "intent.rejected";
pub const ORDER_SUBMITTED: &str = "order.submitted";
pub const ORDER_FILLED: &str = "order.filled";
pub const ORDER_CANCELED: &str = "order.canceled";
pub const POSITION_OPENED: &str = "position.opened";
pub const POSITION_UPDATED: &str = "position.updated";
pub const POSITION_CLOSED: &str = "position.closed";
pub const RISK_BREACH: &str = "risk.breach";
pub const KILLSWITCH_TRIPPED: &str = "killswitch.tripped";

// ---------- Cross-cutting ----------
pub const ONCHAIN_SIGNAL: &str = "onchain.signal";
/// Bridged from Postgres NOTIFY emitted by migration 0014's trigger.
pub const CONFIG_CHANGED: &str = "config_changed";

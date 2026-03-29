//! Global trading halt flag — worker `kill_switch_loop` sets it; strategies must check [`is_trading_halted`] each tick.

use std::sync::atomic::{AtomicBool, Ordering};

static TRADING_HALTED: AtomicBool = AtomicBool::new(false);

#[must_use]
pub fn is_trading_halted() -> bool {
    TRADING_HALTED.load(Ordering::SeqCst)
}

pub fn set_trading_halted(halted: bool) {
    TRADING_HALTED.store(halted, Ordering::SeqCst);
}

/// Marks trading as halted (idempotent semantics for callers).
pub fn halt_trading() {
    TRADING_HALTED.store(true, Ordering::SeqCst);
    tracing::error!("TRADING_HALTED: new orders should be blocked until cleared");
}

pub fn clear_trading_halt() {
    TRADING_HALTED.store(false, Ordering::SeqCst);
    tracing::warn!("TRADING_HALTED cleared");
}

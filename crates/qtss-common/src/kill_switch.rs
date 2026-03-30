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

/// [`clear_trading_halt`] ile aynı — master rehber / API uç adı uyumu.
#[inline]
pub fn resume_trading() {
    clear_trading_halt();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static HALT_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn halt_and_clear_roundtrip() {
        let _g = HALT_TEST_LOCK.lock().expect("halt test lock");
        clear_trading_halt();
        assert!(!is_trading_halted());
        set_trading_halted(true);
        assert!(is_trading_halted());
        clear_trading_halt();
        assert!(!is_trading_halted());
    }

    #[test]
    fn resume_trading_clears_halt() {
        let _g = HALT_TEST_LOCK.lock().expect("halt test lock");
        set_trading_halted(true);
        assert!(is_trading_halted());
        resume_trading();
        assert!(!is_trading_halted());
    }
}

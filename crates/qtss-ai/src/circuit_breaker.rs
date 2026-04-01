//! Simple circuit breaker for AI providers (FAZ P1).
//!
//! After `failure_threshold` consecutive failures the breaker opens and rejects
//! calls for `recovery_secs`.  A single success resets the counter (half-open → closed).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub struct CircuitBreaker {
    consecutive_failures: AtomicU32,
    failure_threshold: u32,
    recovery_secs: u64,
    /// UNIX epoch secs when breaker opened (0 = closed).
    opened_at: AtomicU64,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, recovery_secs: u64) -> Self {
        Self {
            consecutive_failures: AtomicU32::new(0),
            failure_threshold: failure_threshold.max(1),
            recovery_secs: recovery_secs.max(5),
            opened_at: AtomicU64::new(0),
        }
    }

    fn now_epoch() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Returns `true` if the breaker allows a call (closed or half-open after recovery period).
    pub fn allow(&self) -> bool {
        let opened = self.opened_at.load(Ordering::Relaxed);
        if opened == 0 {
            return true;
        }
        // recovery period elapsed → half-open, allow one probe
        Self::now_epoch().saturating_sub(opened) >= self.recovery_secs
    }

    /// Record a successful call — resets breaker to closed.
    pub fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.opened_at.store(0, Ordering::Relaxed);
    }

    /// Record a failed call — may trip the breaker.
    pub fn record_failure(&self) {
        let prev = self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
        if prev + 1 >= self.failure_threshold {
            self.opened_at.store(Self::now_epoch(), Ordering::Relaxed);
        }
    }

    pub fn is_open(&self) -> bool {
        !self.allow()
    }

    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_closed() {
        let cb = CircuitBreaker::new(3, 30);
        assert!(cb.allow());
        assert!(!cb.is_open());
    }

    #[test]
    fn opens_after_threshold() {
        let cb = CircuitBreaker::new(3, 60);
        cb.record_failure();
        cb.record_failure();
        assert!(cb.allow());
        cb.record_failure(); // 3rd
        assert!(cb.is_open());
    }

    #[test]
    fn success_resets() {
        let cb = CircuitBreaker::new(2, 60);
        cb.record_failure();
        cb.record_failure();
        assert!(cb.is_open());
        cb.record_success();
        assert!(cb.allow());
        assert_eq!(cb.consecutive_failures(), 0);
    }

    #[test]
    fn half_open_after_recovery() {
        let cb = CircuitBreaker::new(1, 0); // 0 → min 5s, but we can test by setting opened_at far in past
        cb.record_failure();
        // force opened_at to 100 seconds ago
        cb.opened_at.store(CircuitBreaker::now_epoch() - 100, Ordering::Relaxed);
        assert!(cb.allow()); // recovery elapsed
    }
}

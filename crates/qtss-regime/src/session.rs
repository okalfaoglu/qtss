//! Trading-session classifier.
//!
//! Crypto is a 24/7 market but liquidity and volatility cluster around
//! the three major equity/FX sessions. Strategy sizing, ORB entries,
//! and news-blackout gates all need to know which session owns each
//! bar. This module is a pure helper — no DB, no I/O — so detectors
//! and the API layer can both call it cheaply per bar.
//!
//! Times are UTC. Session calendars are asset-class specific; we keep
//! the crypto default here and expose helpers for BIST / NASDAQ so when
//! those markets come online the caller picks the right one.

use chrono::{DateTime, Datelike, Timelike, Utc, Weekday};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingSession {
    /// Asia-Pacific: ~00:00–09:00 UTC (Tokyo 9-18 JST).
    Asia,
    /// London: ~07:00–16:00 UTC (8-17 BST / 7-16 GMT).
    London,
    /// New York: ~13:00–22:00 UTC (9-18 ET, stocks 9:30-16:00 ET).
    NewYork,
    /// No major overlap (weekend for equities, thin liquidity for
    /// crypto — 22:00-00:00 UTC bridge to Asia).
    Off,
}

impl TradingSession {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asia => "asia",
            Self::London => "london",
            Self::NewYork => "new_york",
            Self::Off => "off",
        }
    }
}

/// Sessions a bar belongs to. A bar can straddle two sessions (the
/// London–NY overlap 13:00–16:00 UTC is the canonical "power hour"
/// window). The primary is the session whose midpoint is closer to the
/// bar's open time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionWindow {
    pub primary: TradingSession,
    pub overlap: Option<TradingSession>,
}

/// Classify a crypto bar. Hours are UTC.
///
/// Hours taken from the "24/7 crypto with equity-session halos"
/// convention: Asia 00–08, London 08–12, Overlap 13–15, NY 16–21, Off
/// 22–23. Exact boundaries configurable via `system_config.session.*`
/// — the default picks a boundary most detectors accept.
pub fn classify_crypto(t: DateTime<Utc>) -> SessionWindow {
    let h = t.hour();
    // London–NY overlap: 13–15 UTC. Both sessions active simultaneously.
    if (13..=15).contains(&h) {
        return SessionWindow {
            primary: TradingSession::London,
            overlap: Some(TradingSession::NewYork),
        };
    }
    let primary = match h {
        0..=7 => TradingSession::Asia,
        8..=12 => TradingSession::London,
        16..=21 => TradingSession::NewYork,
        _ => TradingSession::Off,
    };
    SessionWindow { primary, overlap: None }
}

/// Return true when `t` is the first bar of its session (the bar whose
/// open time crosses the session-open boundary). Caller supplies the
/// bar's **open** time — not the close — so the check is boundary-exact.
pub fn is_session_open_bar(t: DateTime<Utc>, tf_seconds: i64) -> Option<TradingSession> {
    let h = t.hour() as i64;
    let mins_into_hour = t.minute() as i64 * 60 + t.second() as i64;
    let tf_mins = tf_seconds / 60;
    let boundary_hours = [0i64, 8, 13, 16, 22]; // Asia, London, Overlap, NY, Off
    for &bh in &boundary_hours {
        if h == bh && mins_into_hour < tf_mins * 60 {
            return Some(session_at_hour(bh));
        }
    }
    None
}

fn session_at_hour(h: i64) -> TradingSession {
    match h {
        0 => TradingSession::Asia,
        8 | 13 => TradingSession::London,
        16 => TradingSession::NewYork,
        _ => TradingSession::Off,
    }
}

/// Is this a weekend in the equity sense (Sat/Sun)? Crypto ignores
/// weekends for core venues but BIST/NASDAQ calendar pipelines gate
/// on this flag.
pub fn is_equity_weekend(t: DateTime<Utc>) -> bool {
    matches!(t.weekday(), Weekday::Sat | Weekday::Sun)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 1, 8, h, 0, 0).unwrap()
    }

    #[test]
    fn asia_morning() {
        assert_eq!(classify_crypto(utc(3)).primary, TradingSession::Asia);
    }
    #[test]
    fn london_morning() {
        assert_eq!(classify_crypto(utc(9)).primary, TradingSession::London);
    }
    #[test]
    fn overlap_has_both() {
        let w = classify_crypto(utc(14));
        assert_eq!(w.primary, TradingSession::London);
        assert_eq!(w.overlap, Some(TradingSession::NewYork));
    }
    #[test]
    fn ny_afternoon() {
        assert_eq!(classify_crypto(utc(19)).primary, TradingSession::NewYork);
    }
    #[test]
    fn session_open_at_13_utc() {
        assert_eq!(
            is_session_open_bar(utc(13), 60 * 15),
            Some(TradingSession::London)
        );
    }
}

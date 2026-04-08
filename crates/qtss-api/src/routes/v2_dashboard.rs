#![allow(dead_code)]
//! `GET /v2/dashboard` -- Faz 5 (a) wire endpoint.
//!
//! Returns a [`DashboardSnapshot`] built from the in-memory v2 portfolio
//! engine plus a bounded ring buffer of equity-curve samples. The route
//! itself owns no business logic -- it dispatches to
//! [`V2DashboardHandle`], which is the single source of truth for what
//! the dashboard panel sees.
//!
//! The handle's ring-buffer capacity is *not* hardcoded -- it comes from
//! `system_config (api.v2_dashboard_equity_capacity)` per CLAUDE.md
//! rule #2. The default is seeded by [`AppState::new`] when the row is
//! missing.

use std::collections::VecDeque;
use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use tokio::sync::RwLock;

use qtss_gui_api::{DashboardSnapshot, EquityPoint};
use qtss_portfolio::{PortfolioConfig, PortfolioEngine};

use crate::error::ApiError;
use crate::state::SharedState;

/// Shared state slot exposed via [`SharedState`]. Holds the v2 portfolio
/// engine alongside the equity-curve ring buffer the dashboard renders.
pub struct V2DashboardHandle {
    inner: RwLock<Inner>,
}

struct Inner {
    engine: PortfolioEngine,
    equity_curve: VecDeque<EquityPoint>,
    #[allow(dead_code)]
    capacity: usize,
}

impl V2DashboardHandle {
    pub fn new(starting_equity: rust_decimal::Decimal, capacity: usize) -> Arc<Self> {
        let engine = PortfolioEngine::new(PortfolioConfig { starting_equity });
        Arc::new(Self {
            inner: RwLock::new(Inner {
                engine,
                equity_curve: VecDeque::with_capacity(capacity.max(1)),
                capacity: capacity.max(1),
            }),
        })
    }

    /// Snapshot the current state into the wire DTO.
    pub async fn snapshot(&self) -> DashboardSnapshot {
        let g = self.inner.read().await;
        let account = g.engine.snapshot();
        let curve: Vec<EquityPoint> = g.equity_curve.iter().cloned().collect();
        DashboardSnapshot::build(&g.engine, &account, curve)
    }

    /// Append the current equity to the ring buffer. Called by the
    /// portfolio worker (or by tests) -- the route handler stays
    /// read-only.
    pub async fn record_equity_sample(&self) {
        let mut g = self.inner.write().await;
        let equity = g.engine.snapshot().equity;
        let cap = g.capacity;
        let point = EquityPoint { at: Utc::now(), equity };
        if g.equity_curve.len() == cap {
            g.equity_curve.pop_front();
        }
        g.equity_curve.push_back(point);
    }

    /// Mutable access for the writer side (fills, marks). Kept on the
    /// handle so the engine instance is never duplicated.
    pub async fn with_engine<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut PortfolioEngine) -> R,
    {
        let mut g = self.inner.write().await;
        f(&mut g.engine)
    }
}

pub fn v2_dashboard_router() -> Router<SharedState> {
    Router::new().route("/v2/dashboard", get(get_dashboard))
}

async fn get_dashboard(
    State(st): State<SharedState>,
) -> Result<Json<DashboardSnapshot>, ApiError> {
    Ok(Json(st.v2_dashboard.snapshot().await))
}

#[cfg(test)]
mod tests {
    use super::*;
    use qtss_domain::v2::intent::Side;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn snapshot_is_empty_for_fresh_handle() {
        let h = V2DashboardHandle::new(dec!(10000), 8);
        let snap = h.snapshot().await;
        assert_eq!(snap.portfolio.equity, dec!(10000));
        assert!(snap.open_positions.is_empty());
        assert!(snap.equity_curve.is_empty());
    }

    #[tokio::test]
    async fn ring_buffer_evicts_oldest() {
        let h = V2DashboardHandle::new(dec!(10000), 3);
        for _ in 0..5 {
            h.record_equity_sample().await;
        }
        let snap = h.snapshot().await;
        assert_eq!(snap.equity_curve.len(), 3);
    }

    #[tokio::test]
    async fn snapshot_reflects_fills() {
        let h = V2DashboardHandle::new(dec!(10000), 8);
        h.with_engine(|e| {
            e.apply_fill("BTCUSDT", Side::Long, dec!(0.1), dec!(50000), dec!(2));
            e.mark("BTCUSDT", dec!(51000));
        })
        .await;
        let snap = h.snapshot().await;
        assert_eq!(snap.open_positions.len(), 1);
        assert_eq!(snap.open_positions[0].symbol, "BTCUSDT");
    }
}

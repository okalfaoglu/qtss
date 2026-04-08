//! qtss-exec-binance — Binance spot adapter for the v2 execution layer.
//!
//! Wraps the low-level [`qtss_binance::BinanceClient`] (REST signing,
//! exchange-info, raw `POST /api/v3/order`) behind the v2
//! [`ExecutionAdapter`] trait so the router treats it the same as the
//! sim adapter or any other venue.
//!
//! ## Layering (CLAUDE.md rule #3)
//!
//! - This crate knows about Binance order-type strings.
//! - It does **not** know about strategies, validators, risk gates, or
//!   the portfolio engine. The router calls `place(OrderRequest)`; we
//!   translate, sign, post, and lift the response back into an
//!   `OrderAck`. Nothing else.
//! - No hardcoded fees / slippage / retry counts (CLAUDE.md rule #2).
//!   Everything that varies per environment lives in
//!   [`BinanceExecConfig`].
//!
//! Realised fees come from [`qtss_fees::FeeModel`] — the live broker
//! response will of course also carry a `commission` field, but until
//! we wire user-data-stream fills back into the engine the fee model
//! gives the portfolio a deterministic cost number that matches what
//! the sim adapter charges.

mod adapter;
mod error;
mod translate;

pub use adapter::{BinanceExecAdapter, BinanceExecConfig};
pub use error::{BinanceExecError, BinanceExecResult};

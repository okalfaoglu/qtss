//! Resolver implementations. One module per resolver so the dispatch
//! registry can list them in preference order without any per-resolver
//! glue code (CLAUDE.md #1 — table-driven).

pub mod atr_band;
pub mod fib_extension;
pub mod harmonic_prz;
pub mod structural;
pub mod vprofile;

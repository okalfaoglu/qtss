//! Tactical cadence — see `client::run_tactical_sweep`.

use crate::client::{run_tactical_sweep, AiRuntime};
use crate::error::AiResult;

/// Executes one tactical sweep (all enabled engine symbols).
pub async fn run(rt: &AiRuntime) -> AiResult<()> {
    run_tactical_sweep(rt).await
}

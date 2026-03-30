//! Operational cadence (open position management).

use crate::client::{run_operational_sweep, AiRuntime};
use crate::error::AiResult;

pub async fn run(rt: &AiRuntime) -> AiResult<()> {
    run_operational_sweep(rt).await
}

//! Strategic cadence (portfolio directives).

use crate::client::{run_strategic_sweep, AiRuntime};
use crate::error::AiResult;

pub async fn run(rt: &AiRuntime) -> AiResult<()> {
    run_strategic_sweep(rt).await
}

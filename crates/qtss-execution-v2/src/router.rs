//! ExecutionRouter — picks an adapter by `ExecutionMode` and dispatches
//! the bracket built from the ApprovedIntent.
//!
//! Adapters are stored in a `HashMap<ExecutionMode, Arc<dyn ExecutionAdapter>>`
//! so adding a new mode (e.g. dedicated futures live adapter) is one
//! `register` call — no central match arm to edit (CLAUDE.md rule #1).

use crate::adapter::{ExecutionAdapter, OrderAck};
use crate::builder::{split_intent, OrderBracket};
use crate::error::{ExecutionError, ExecutionResult};
use qtss_domain::execution::ExecutionMode;
use qtss_domain::v2::intent::ApprovedIntent;
use std::collections::HashMap;
use std::sync::Arc;

pub struct ExecutionRouter {
    adapters: HashMap<ExecutionMode, Arc<dyn ExecutionAdapter>>,
}

#[derive(Debug, Clone)]
pub struct RoutedAcks {
    pub entry: OrderAck,
    pub stop: OrderAck,
    pub take_profits: Vec<OrderAck>,
}

impl Default for ExecutionRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionRouter {
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    pub fn register(&mut self, mode: ExecutionMode, adapter: Arc<dyn ExecutionAdapter>) {
        self.adapters.insert(mode, adapter);
    }

    pub fn adapter_count(&self) -> usize {
        self.adapters.len()
    }

    /// Build the bracket and place every leg through the adapter that
    /// matches the intent's run mode. Returns a partial result if the
    /// stop or any take-profit fails (the entry has already filled by
    /// then), with the failing leg surfaced as `Err`.
    pub async fn route(&self, approved: &ApprovedIntent) -> ExecutionResult<RoutedAcks> {
        let bracket: OrderBracket = split_intent(approved)?;
        let mode = approved.intent.mode;
        let adapter = self
            .adapters
            .get(&mode)
            .cloned()
            .ok_or(ExecutionError::NoAdapter(mode))?;

        let entry = adapter.place(bracket.entry.clone()).await?;
        let stop = adapter.place(bracket.stop.clone()).await?;
        let mut tps = Vec::with_capacity(bracket.take_profits.len());
        for tp in &bracket.take_profits {
            tps.push(adapter.place(tp.clone()).await?);
        }
        Ok(RoutedAcks {
            entry,
            stop,
            take_profits: tps,
        })
    }
}

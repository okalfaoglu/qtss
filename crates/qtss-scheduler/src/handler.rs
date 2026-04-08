//! Handler trait + dispatch registry.
//!
//! A handler is the "what to do" half of a scheduled job. The DB row only
//! stores a `handler` string key; at runtime the scheduler looks the key
//! up in [`HandlerRegistry`] and invokes the corresponding implementation.
//! Adding a new periodic task is one `register()` call — no `match` arm
//! in the scheduler core, no `if` chain anywhere.

use crate::error::{SchedulerError, SchedulerResult};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

/// What a handler hands back. The scheduler persists this as the `output`
/// JSON column on the corresponding `job_runs` row.
#[derive(Debug, Clone)]
pub struct HandlerResult {
    pub output: serde_json::Value,
}

impl HandlerResult {
    pub fn empty() -> Self {
        Self {
            output: serde_json::json!({}),
        }
    }
}

#[async_trait]
pub trait Handler: Send + Sync {
    /// Execute the handler. The `payload` is the job row's payload column.
    async fn run(&self, payload: serde_json::Value) -> SchedulerResult<HandlerResult>;
}

#[derive(Default, Clone)]
pub struct HandlerRegistry {
    handlers: HashMap<String, Arc<dyn Handler>>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, key: impl Into<String>, handler: Arc<dyn Handler>) {
        self.handlers.insert(key.into(), handler);
    }

    pub fn get(&self, key: &str) -> SchedulerResult<Arc<dyn Handler>> {
        self.handlers
            .get(key)
            .cloned()
            .ok_or_else(|| SchedulerError::UnknownHandler(key.to_string()))
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.handlers.keys()
    }
}

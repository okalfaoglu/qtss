//! Event envelope — wraps any payload with topic, timestamp, and a
//! correlation id used for tracing across modules.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event<T> {
    pub id: Uuid,
    pub topic: String,
    pub at: DateTime<Utc>,
    /// Used to thread an event chain (e.g. bar.closed -> pattern.detected
    /// -> intent.created carries the same correlation id).
    pub correlation: Option<Uuid>,
    pub payload: T,
}

impl<T> Event<T> {
    pub fn new(topic: impl Into<String>, payload: T) -> Self {
        Self {
            id: Uuid::new_v4(),
            topic: topic.into(),
            at: Utc::now(),
            correlation: None,
            payload,
        }
    }

    pub fn with_correlation(mut self, correlation: Uuid) -> Self {
        self.correlation = Some(correlation);
        self
    }

    /// Map the payload while preserving id, topic, time, and correlation.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Event<U> {
        Event {
            id: self.id,
            topic: self.topic,
            at: self.at,
            correlation: self.correlation,
            payload: f(self.payload),
        }
    }
}

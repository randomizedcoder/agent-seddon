//! `CompositeMemory` — a `MemoryStore` that mirrors every appended event into
//! ClickHouse while delegating recall/distill (and the durable append) to an
//! inner store (the JSONL `FileMemory`).

use crate::TelemetryHandle;
use agent_core::{MemoryEvent, MemoryItem, MemoryStore, RecallQuery, Result};
use async_trait::async_trait;
use std::sync::Arc;

pub struct CompositeMemory {
    inner: Arc<dyn MemoryStore>,
    telemetry: TelemetryHandle,
}

impl CompositeMemory {
    pub fn new(inner: Arc<dyn MemoryStore>, telemetry: TelemetryHandle) -> Self {
        Self { inner, telemetry }
    }
}

#[async_trait]
impl MemoryStore for CompositeMemory {
    async fn recall(&self, query: &RecallQuery) -> Result<Vec<MemoryItem>> {
        self.inner.recall(query).await
    }

    async fn append(&self, event: MemoryEvent) -> Result<()> {
        // Mirror first (non-blocking), then persist to the durable inner store.
        self.telemetry.record_event(&event);
        self.inner.append(event).await
    }

    async fn distill(&self) -> Result<usize> {
        self.inner.distill().await
    }
}

//! Per-seam metrics wrappers.
//!
//! Each seam (provider, tools, memory, context, policy) is built by the registry
//! as an `Arc<dyn Trait>`, then wrapped here in a thin decorator that records the
//! component's own timings/counts into the shared [`Metrics`] registry before
//! delegating to the inner impl. Wrapping at the seam boundary (rather than in the
//! loop) means every component — local *or* a remote `= "grpc"` client — is
//! attributed independently, and the same wrapper on a `--serve-<seam>` process
//! captures that seam's server-side latency. The metric names mirror the tracing
//! span tree in `docs/tracing.md`.
//!
//! The loop (`agent.rs`) still records the top-level run/iteration/api/token/tool
//! counters; these wrappers add the per-component detail (provider TTFT, recall
//! item counts, compaction deltas, authorize decisions, per-tool latency).

use agent_core::{
    ChunkStream, CompletionRequest, CompletionResponse, ContextInput, ContextStrategy, Decision,
    LlmProvider, MemoryEvent, MemoryItem, MemoryStore, Message, ModelCapabilities, Observation,
    Policy, RecallQuery, Result, TokenBudget, Tool, ToolContext, ToolSchema, WorkingSet,
};
use agent_metrics::Metrics;
use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Instant;

/// Wrap each seam of a built agent in its metrics decorator. `provider_name`,
/// `context_name`, `policy_name` are the config-selected impl names (used as the
/// metric label so `= "grpc"` reads distinctly from `= "anthropic"`).
pub(crate) fn provider(
    inner: Arc<dyn LlmProvider>,
    m: Metrics,
    name: &str,
) -> Arc<dyn LlmProvider> {
    Arc::new(MeteredProvider {
        inner,
        metrics: m,
        name: name.to_string(),
    })
}
pub(crate) fn memory(inner: Arc<dyn MemoryStore>, m: Metrics) -> Arc<dyn MemoryStore> {
    Arc::new(MeteredMemory { inner, metrics: m })
}
pub(crate) fn context(inner: Arc<dyn ContextStrategy>, m: Metrics) -> Arc<dyn ContextStrategy> {
    Arc::new(MeteredContext { inner, metrics: m })
}
pub(crate) fn policy(inner: Arc<dyn Policy>, m: Metrics, name: &str) -> Arc<dyn Policy> {
    Arc::new(MeteredPolicy {
        inner,
        metrics: m,
        name: name.to_string(),
    })
}
pub(crate) fn tool(inner: Arc<dyn Tool>, m: Metrics) -> Arc<dyn Tool> {
    Arc::new(MeteredTool { inner, metrics: m })
}

// --- provider --------------------------------------------------------------

struct MeteredProvider {
    inner: Arc<dyn LlmProvider>,
    metrics: Metrics,
    name: String,
}

#[async_trait]
impl LlmProvider for MeteredProvider {
    fn capabilities(&self) -> ModelCapabilities {
        self.inner.capabilities()
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let start = Instant::now();
        let out = self.inner.complete(req).await;
        self.metrics
            .on_provider_request(&self.name, false, start.elapsed().as_secs_f64());
        if out.is_err() {
            self.metrics.on_provider_error(&self.name, "complete");
        }
        out
    }

    async fn stream(&self, req: CompletionRequest) -> Result<ChunkStream> {
        let start = Instant::now();
        let inner = match self.inner.stream(req).await {
            Ok(s) => s,
            Err(e) => {
                self.metrics.on_provider_error(&self.name, "stream");
                return Err(e);
            }
        };
        let metrics = self.metrics.clone();
        let name = self.name.clone();
        // Wrap the chunk stream to time-to-first-token, count chunks, and record
        // the total request duration once the stream is fully consumed.
        let wrapped = async_stream::stream! {
            let mut inner = inner;
            let mut first = true;
            let mut chunks = 0u64;
            while let Some(item) = inner.next().await {
                if first {
                    metrics.on_provider_ttft(&name, start.elapsed().as_secs_f64());
                    first = false;
                }
                chunks += 1;
                if item.is_err() {
                    metrics.on_provider_error(&name, "stream");
                }
                yield item;
            }
            metrics.add_provider_chunks(&name, chunks);
            metrics.on_provider_request(&name, true, start.elapsed().as_secs_f64());
        };
        Ok(Box::pin(wrapped))
    }
}

// --- memory ----------------------------------------------------------------

struct MeteredMemory {
    inner: Arc<dyn MemoryStore>,
    metrics: Metrics,
}

#[async_trait]
impl MemoryStore for MeteredMemory {
    async fn recall(&self, query: &RecallQuery) -> Result<Vec<MemoryItem>> {
        let start = Instant::now();
        let out = self.inner.recall(query).await;
        self.metrics
            .on_memory_op("recall", start.elapsed().as_secs_f64());
        match &out {
            Ok(items) => self.metrics.observe_recall_items(items.len()),
            Err(_) => self.metrics.on_memory_error("recall"),
        }
        out
    }
    async fn append(&self, event: MemoryEvent) -> Result<()> {
        let start = Instant::now();
        let out = self.inner.append(event).await;
        self.metrics
            .on_memory_op("append", start.elapsed().as_secs_f64());
        if out.is_err() {
            self.metrics.on_memory_error("append");
        }
        out
    }
    async fn distill(&self) -> Result<usize> {
        let start = Instant::now();
        let out = self.inner.distill().await;
        self.metrics
            .on_memory_op("distill", start.elapsed().as_secs_f64());
        if out.is_err() {
            self.metrics.on_memory_error("distill");
        }
        out
    }
}

// --- context ---------------------------------------------------------------

struct MeteredContext {
    inner: Arc<dyn ContextStrategy>,
    metrics: Metrics,
}

#[async_trait]
impl ContextStrategy for MeteredContext {
    async fn assemble(&self, input: ContextInput) -> Result<Vec<Message>> {
        let start = Instant::now();
        let out = self.inner.assemble(input).await;
        self.metrics
            .on_context_op("assemble", start.elapsed().as_secs_f64());
        out
    }
    async fn compact(&self, working: &mut WorkingSet, budget: &TokenBudget) -> Result<()> {
        let before = rough_tokens(&working.messages);
        let start = Instant::now();
        let out = self.inner.compact(working, budget).await;
        self.metrics
            .on_context_op("compact", start.elapsed().as_secs_f64());
        // `compact` is called every iteration; only record a compaction when it
        // actually trimmed the working set (tokens dropped).
        let after = rough_tokens(&working.messages);
        if after < before {
            self.metrics.on_compaction(before as i64, after as i64);
        }
        out
    }
}

/// Rough token estimate (~4 chars/token + per-message tax), mirroring
/// `agent_context::estimate_tokens` — kept local so this wrapper needs nothing
/// from the context crate.
fn rough_tokens(messages: &[Message]) -> u32 {
    let mut chars = 0usize;
    for m in messages {
        chars += m.content.len();
        for tc in &m.tool_calls {
            chars += tc.name.len() + tc.arguments.to_string().len();
        }
        chars += 8;
    }
    (chars / 4) as u32
}

// --- policy ----------------------------------------------------------------

struct MeteredPolicy {
    inner: Arc<dyn Policy>,
    metrics: Metrics,
    name: String,
}

#[async_trait]
impl Policy for MeteredPolicy {
    async fn authorize(&self, call: &agent_core::ToolCall) -> Decision {
        let start = Instant::now();
        let decision = self.inner.authorize(call).await;
        let label = match &decision {
            Decision::Allow => "allow",
            Decision::Deny(_) => "deny",
        };
        self.metrics
            .on_authorize(&self.name, label, start.elapsed().as_secs_f64());
        decision
    }
}

// --- tools -----------------------------------------------------------------

struct MeteredTool {
    inner: Arc<dyn Tool>,
    metrics: Metrics,
}

#[async_trait]
impl Tool for MeteredTool {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn schema(&self) -> ToolSchema {
        self.inner.schema()
    }
    fn parallel_safe(&self) -> bool {
        self.inner.parallel_safe()
    }
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<Observation> {
        let start = Instant::now();
        let out = self.inner.execute(args, ctx).await;
        self.metrics
            .on_tool_exec(self.inner.name(), start.elapsed().as_secs_f64());
        match &out {
            Ok(o) if o.is_error => self.metrics.on_tool_error(self.inner.name(), "observation"),
            Err(_) => self.metrics.on_tool_error(self.inner.name(), "error"),
            _ => {}
        }
        out
    }
}

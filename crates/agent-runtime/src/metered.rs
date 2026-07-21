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

#[cfg(feature = "git")]
use agent_core::{
    BlobContent, Checkpoint, CommitInfo, DiffResult, GrepHit, Oid, RepoBackend, RepoStatus,
    Revision, TreeEntry, WorktreeHandle, WorktreeSpec,
};
use agent_core::{
    ChunkStream, CompletionRequest, CompletionResponse, ContextInput, ContextStrategy, Decision,
    LlmProvider, MemoryEvent, MemoryItem, MemoryStore, Message, ModelCapabilities, Observation,
    Policy, RecallQuery, Result, TokenBudget, Tool, ToolContext, ToolSchema, WorkingSet,
};
#[cfg(feature = "search")]
use agent_core::{
    IndexState, IndexStatus, ProgressFn, SearchBackend, SearchCapabilities, SearchHit, SearchQuery,
};
use agent_metrics::Metrics;
use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Instant;
use tracing::Instrument;

// Serializes the span/metric tests that share a tracing callsite (`web.fetch`,
// `tasks.write`): a non-recording test can cache a callsite's interest as
// disabled, so a span-capturing test must rebuild the interest cache without a
// concurrent test swapping the subscriber underneath it. `#[cfg(test)]` only.
#[cfg(test)]
pub(crate) static CALLSITE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn callsite_guard() -> std::sync::MutexGuard<'static, ()> {
    CALLSITE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

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
/// Wrap a single search backend. `name` is the concrete backend (`tantivy`, …),
/// the label that makes the head-to-head comparison meaningful — so backends are
/// wrapped *before* being composed into a `DispatchSearch`.
#[cfg(feature = "search")]
pub(crate) fn search(
    inner: Arc<dyn SearchBackend>,
    m: Metrics,
    name: &str,
) -> Arc<dyn SearchBackend> {
    Arc::new(MeteredSearch {
        inner,
        metrics: m,
        name: name.to_string(),
    })
}
/// Wrap the git backend. `name` is the config-selected impl (`cli`/`hybrid`/`grpc`),
/// the metric label so a remote seam reads distinctly from a local one.
#[cfg(feature = "git")]
pub(crate) fn repo(inner: Arc<dyn RepoBackend>, m: Metrics, name: &str) -> Arc<dyn RepoBackend> {
    Arc::new(MeteredRepo {
        inner,
        metrics: m,
        name: name.to_string(),
    })
}

/// Wrap a [`Tokenizer`](agent_core::Tokenizer) so each `count` emits a
/// `tokenizer.count` span carrying `backend`/`model`/`text_bytes`/`tokens`
/// attributes (the span-attribute pattern from the telemetry work). Cost/cache
/// USD accounting is recorded in the loop where usage arrives, not here.
#[cfg(feature = "tokenizer")]
pub(crate) fn tokenizer(inner: Arc<dyn agent_core::Tokenizer>) -> Arc<dyn agent_core::Tokenizer> {
    Arc::new(MeteredTokenizer { inner })
}

/// Wrap a [`WebBackend`](agent_core::WebBackend) so each fetch emits a
/// `web.fetch` span carrying `host`/`format`/`status`/`bytes` attributes and
/// records the `agent_web_fetch_*` metrics. The host is a span attribute, not a
/// metric label (untrusted URL → cardinality DoS).
#[cfg(feature = "web")]
pub(crate) fn web(
    inner: Arc<dyn agent_core::WebBackend>,
    m: Metrics,
) -> Arc<dyn agent_core::WebBackend> {
    Arc::new(MeteredWeb { inner, metrics: m })
}

/// Wrap a [`TaskTracker`](agent_core::TaskTracker) so each mutation refreshes the
/// `agent_tasks_open`/`agent_tasks_closed` gauges and emits a `tasks.<op>` span
/// carrying `{op, total, in_progress, completed}` attributes.
#[cfg(feature = "tasks")]
pub(crate) fn tasks(
    inner: Arc<dyn agent_core::TaskTracker>,
    m: Metrics,
) -> Arc<dyn agent_core::TaskTracker> {
    Arc::new(MeteredTasks { inner, metrics: m })
}

#[cfg(feature = "tasks")]
struct MeteredTasks {
    inner: Arc<dyn agent_core::TaskTracker>,
    metrics: Metrics,
}

#[cfg(feature = "tasks")]
impl MeteredTasks {
    /// Refresh the progress gauges + record the plan-shape span attributes.
    fn record_plan(&self, span: &tracing::Span, list: &[agent_core::Todo]) {
        use agent_core::TodoStatus;
        let total = list.len();
        let in_progress = list
            .iter()
            .filter(|t| t.status == TodoStatus::InProgress)
            .count();
        let completed = list
            .iter()
            .filter(|t| t.status == TodoStatus::Completed)
            .count();
        let open = list.iter().filter(|t| t.status.is_open()).count();
        span.record("total", total);
        span.record("in_progress", in_progress);
        span.record("completed", completed);
        self.metrics
            .set_tasks_progress(open as i64, (total - open) as i64);
    }
}

#[cfg(feature = "tasks")]
#[async_trait]
impl agent_core::TaskTracker for MeteredTasks {
    async fn write(&self, todos: Vec<agent_core::Todo>) -> Result<Vec<agent_core::Todo>> {
        let span = tracing::info_span!(
            "tasks.write",
            op = "write",
            total = tracing::field::Empty,
            in_progress = tracing::field::Empty,
            completed = tracing::field::Empty,
        );
        let out = self.inner.write(todos).instrument(span.clone()).await;
        if let Ok(list) = &out {
            self.record_plan(&span, list);
        }
        out
    }
    async fn update(&self, patch: agent_core::TodoPatch) -> Result<Vec<agent_core::Todo>> {
        let span = tracing::info_span!(
            "tasks.update",
            op = "update",
            total = tracing::field::Empty,
            in_progress = tracing::field::Empty,
            completed = tracing::field::Empty,
        );
        let out = self.inner.update(patch).instrument(span.clone()).await;
        if let Ok(list) = &out {
            self.record_plan(&span, list);
        }
        out
    }
    async fn list(&self) -> Result<Vec<agent_core::Todo>> {
        self.inner.list().await
    }
    async fn clear(&self) -> Result<()> {
        let out = self
            .inner
            .clear()
            .instrument(tracing::info_span!("tasks.clear", op = "clear"))
            .await;
        if out.is_ok() {
            self.metrics.set_tasks_progress(0, 0);
        }
        out
    }
}

/// Wrap an [`OutputSchema`](agent_core::OutputSchema) so each `validate` emits a
/// `structured.validate` span (`ok` / `errors` attrs) and records the validation
/// latency. The per-completion outcome counter is recorded by the repair loop.
#[cfg(feature = "structured")]
pub(crate) fn validator(
    inner: Arc<dyn agent_core::OutputSchema>,
    m: Metrics,
) -> Arc<dyn agent_core::OutputSchema> {
    Arc::new(MeteredValidator { inner, metrics: m })
}

#[cfg(feature = "structured")]
struct MeteredValidator {
    inner: Arc<dyn agent_core::OutputSchema>,
    metrics: Metrics,
}

#[cfg(feature = "structured")]
impl agent_core::OutputSchema for MeteredValidator {
    fn validate(
        &self,
        schema: &serde_json::Value,
        value: &serde_json::Value,
    ) -> agent_core::Verdict {
        let start = Instant::now();
        let span = tracing::info_span!(
            "structured.validate",
            ok = tracing::field::Empty,
            errors = tracing::field::Empty,
        );
        let verdict = span.in_scope(|| self.inner.validate(schema, value));
        span.record("ok", verdict.ok);
        span.record("errors", verdict.errors.len());
        self.metrics
            .on_structured_validate(start.elapsed().as_secs_f64());
        verdict
    }
}

/// Wrap a [`Sandbox`](agent_core::Sandbox) so each `exec` emits a `sandbox.exec`
/// span (`backend` attr) and records per-backend exec latency + outcome.
#[cfg(feature = "tool-core")]
pub(crate) fn sandbox(
    inner: Arc<dyn agent_core::Sandbox>,
    m: Metrics,
) -> Arc<dyn agent_core::Sandbox> {
    Arc::new(MeteredSandbox { inner, metrics: m })
}

#[cfg(feature = "tool-core")]
struct MeteredSandbox {
    inner: Arc<dyn agent_core::Sandbox>,
    metrics: Metrics,
}

#[cfg(feature = "tool-core")]
#[async_trait]
impl agent_core::Sandbox for MeteredSandbox {
    async fn exec(&self, spec: &agent_core::ExecSpec) -> Result<agent_core::ExecOutput> {
        let backend = self.inner.capabilities().backend;
        let span = tracing::info_span!("sandbox.exec", backend = %backend);
        let start = Instant::now();
        let out = self.inner.exec(spec).instrument(span).await;
        let outcome = match &out {
            Ok(o) if o.exit_code == 0 && !o.timed_out => "ok",
            _ => "error",
        };
        self.metrics
            .on_sandbox_exec(&backend, outcome, start.elapsed().as_secs_f64());
        out
    }
    fn capabilities(&self) -> agent_core::SandboxCapabilities {
        self.inner.capabilities()
    }
}

/// Wrap a [`SessionStore`](agent_core::SessionStore) so each mutation emits a
/// `session.<op>` span (`session`/`id` attrs) + counts the op; `prune` also counts
/// reclaimed objects.
#[cfg(feature = "session")]
pub(crate) fn session(
    inner: Arc<dyn agent_core::SessionStore>,
    m: Metrics,
) -> Arc<dyn agent_core::SessionStore> {
    Arc::new(MeteredSession { inner, metrics: m })
}

#[cfg(feature = "session")]
struct MeteredSession {
    inner: Arc<dyn agent_core::SessionStore>,
    metrics: Metrics,
}

#[cfg(feature = "session")]
#[async_trait]
impl agent_core::SessionStore for MeteredSession {
    async fn checkpoint(
        &self,
        session: &str,
        ws: &agent_core::WorkingSet,
        label: &str,
    ) -> Result<agent_core::CheckpointId> {
        let out = self
            .inner
            .checkpoint(session, ws, label)
            .instrument(
                tracing::info_span!("session.checkpoint", session = %session, label = %label),
            )
            .await;
        self.metrics.on_session_op("checkpoint");
        out
    }
    async fn list(&self, session: &str) -> Result<Vec<agent_core::CheckpointMeta>> {
        self.inner.list(session).await
    }
    async fn restore(&self, id: &agent_core::CheckpointId) -> Result<agent_core::WorkingSet> {
        let out = self
            .inner
            .restore(id)
            .instrument(tracing::info_span!("session.restore", id = %id))
            .await;
        self.metrics.on_session_op("restore");
        out
    }
    async fn branch(
        &self,
        session: &str,
        from: &agent_core::CheckpointId,
        name: &str,
    ) -> Result<()> {
        let out = self
            .inner
            .branch(session, from, name)
            .instrument(tracing::info_span!("session.branch", session = %session, from = %from, name = %name))
            .await;
        self.metrics.on_session_op("branch");
        out
    }
    async fn undo(&self, session: &str, n: u32) -> Result<agent_core::CheckpointId> {
        let out = self
            .inner
            .undo(session, n)
            .instrument(tracing::info_span!("session.undo", session = %session, n))
            .await;
        self.metrics.on_session_op("undo");
        out
    }
    async fn fork(&self, session: &str) -> Result<String> {
        let out = self
            .inner
            .fork(session)
            .instrument(tracing::info_span!("session.fork", session = %session))
            .await;
        self.metrics.on_session_op("fork");
        out
    }
    async fn diff(
        &self,
        a: &agent_core::CheckpointId,
        b: &agent_core::CheckpointId,
    ) -> Result<agent_core::CheckpointDiff> {
        self.inner.diff(a, b).await
    }
    async fn prune(&self, session: &str) -> Result<usize> {
        let out = self
            .inner
            .prune(session)
            .instrument(tracing::info_span!("session.prune", session = %session))
            .await;
        if let Ok(n) = &out {
            self.metrics.on_session_gc(*n);
        }
        self.metrics.on_session_op("prune");
        out
    }
}

/// Wrap an [`Embedder`](agent_core::Embedder) so each embed emits an
/// `embedder.embed` span (`backend`/`batch` attrs) + records latency/batch size.
#[cfg(feature = "semantic-search")]
pub(crate) fn embedder(
    inner: Arc<dyn agent_core::Embedder>,
    m: Metrics,
    name: &str,
) -> Arc<dyn agent_core::Embedder> {
    Arc::new(MeteredEmbedder {
        inner,
        metrics: m,
        name: name.to_string(),
    })
}

#[cfg(feature = "semantic-search")]
struct MeteredEmbedder {
    inner: Arc<dyn agent_core::Embedder>,
    metrics: Metrics,
    name: String,
}

#[cfg(feature = "semantic-search")]
#[async_trait]
impl agent_core::Embedder for MeteredEmbedder {
    fn dimensions(&self) -> usize {
        self.inner.dimensions()
    }
    fn max_batch(&self) -> usize {
        self.inner.max_batch()
    }
    async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        let span = tracing::info_span!("embedder.embed", backend = %self.name, batch = 1);
        let start = Instant::now();
        let out = self.inner.embed_query(text).instrument(span).await;
        self.metrics
            .on_embed(&self.name, start.elapsed().as_secs_f64(), 1);
        out
    }
    async fn embed_docs(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let span = tracing::info_span!("embedder.embed", backend = %self.name, batch = texts.len());
        let start = Instant::now();
        let out = self.inner.embed_docs(texts).instrument(span).await;
        self.metrics
            .on_embed(&self.name, start.elapsed().as_secs_f64(), texts.len());
        out
    }
}

/// Wrap an [`LspBackend`](agent_core::LspBackend) so each `request` emits an
/// `lsp.request` span (`method`/`uri` attrs), records per-method latency/errors,
/// and counts observed diagnostics by severity.
#[cfg(feature = "lsp")]
pub(crate) fn lsp(
    inner: Arc<dyn agent_core::LspBackend>,
    m: Metrics,
) -> Arc<dyn agent_core::LspBackend> {
    Arc::new(MeteredLsp { inner, metrics: m })
}

#[cfg(feature = "lsp")]
struct MeteredLsp {
    inner: Arc<dyn agent_core::LspBackend>,
    metrics: Metrics,
}

#[cfg(feature = "lsp")]
#[async_trait]
impl agent_core::LspBackend for MeteredLsp {
    fn capabilities(&self, language: &str) -> agent_core::LspCapabilities {
        self.inner.capabilities(language)
    }
    async fn open(&self, uri: &str, text: &str) -> Result<()> {
        self.inner.open(uri, text).await
    }
    async fn request(&self, req: &agent_core::LspRequest) -> Result<agent_core::LspResult> {
        let span = tracing::info_span!(
            "lsp.request",
            method = req.method.as_str(),
            uri = %req.uri,
        );
        let start = Instant::now();
        let out = self.inner.request(req).instrument(span).await;
        self.metrics
            .on_lsp_request(req.method.as_str(), start.elapsed().as_secs_f64());
        match &out {
            Ok(agent_core::LspResult::Diagnostics(d)) => {
                for diag in d {
                    self.metrics.on_lsp_diagnostic(diag.severity.as_str());
                }
            }
            Err(_) => self.metrics.on_lsp_error(req.method.as_str()),
            _ => {}
        }
        out
    }
    async fn shutdown(&self) -> Result<()> {
        self.inner.shutdown().await
    }
}

#[cfg(feature = "web")]
struct MeteredWeb {
    inner: Arc<dyn agent_core::WebBackend>,
    metrics: Metrics,
}

#[cfg(feature = "web")]
#[async_trait]
impl agent_core::WebBackend for MeteredWeb {
    async fn fetch(&self, req: &agent_core::WebRequest) -> Result<agent_core::WebResponse> {
        let host = url::Url::parse(&req.url)
            .ok()
            .and_then(|u| u.host_str().map(String::from))
            .unwrap_or_else(|| "?".into());
        let span = tracing::info_span!(
            "web.fetch",
            host = %host,
            format = req.format.as_str(),
            status = tracing::field::Empty,
            bytes = tracing::field::Empty,
        );
        let start = Instant::now();
        let out = self.inner.fetch(req).instrument(span.clone()).await;
        let secs = start.elapsed().as_secs_f64();
        match &out {
            Ok(r) => {
                span.record("status", r.status);
                span.record("bytes", r.bytes);
                self.metrics.on_web_fetch("ok", secs, r.bytes);
            }
            Err(_) => self.metrics.on_web_fetch("error", secs, 0),
        }
        out
    }
}

#[cfg(feature = "tokenizer")]
struct MeteredTokenizer {
    inner: Arc<dyn agent_core::Tokenizer>,
}

#[cfg(feature = "tokenizer")]
#[async_trait]
impl agent_core::Tokenizer for MeteredTokenizer {
    fn backend(&self) -> &str {
        self.inner.backend()
    }
    async fn count(&self, text: &str, model: &str) -> Result<u32> {
        let span = tracing::info_span!(
            "tokenizer.count",
            backend = self.inner.backend(),
            model,
            text_bytes = text.len(),
            tokens = tracing::field::Empty,
        );
        let out = self.inner.count(text, model).instrument(span.clone()).await;
        if let Ok(n) = &out {
            span.record("tokens", *n);
        }
        out
    }
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
        let out = self
            .inner
            .append(event)
            .instrument(tracing::info_span!("memory.append"))
            .await;
        self.metrics
            .on_memory_op("append", start.elapsed().as_secs_f64());
        if out.is_err() {
            self.metrics.on_memory_error("append");
        }
        out
    }
    async fn distill(&self) -> Result<usize> {
        let start = Instant::now();
        let out = self
            .inner
            .distill()
            .instrument(tracing::info_span!("memory.distill"))
            .await;
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
    let mut tokens = 0u32;
    for m in messages {
        let mut chars = 0usize;
        for b in &m.content {
            match b {
                agent_core::ContentBlock::Text { text } => chars += text.len(),
                // Media is not text: charge it via the shared estimator so all
                // three estimators agree on what an image costs. Counting only
                // text here would let an image-bearing turn look nearly free and
                // overflow the model's window.
                media => tokens = tokens.saturating_add(agent_core::media_block_tokens(media)),
            }
        }
        for tc in &m.tool_calls {
            chars += tc.name.len() + tc.arguments.to_string().len();
        }
        chars += 8; // role/formatting overhead
        tokens = tokens.saturating_add((chars / 4) as u32);
    }
    tokens
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

// --- search ----------------------------------------------------------------

#[cfg(feature = "search")]
struct MeteredSearch {
    inner: Arc<dyn SearchBackend>,
    metrics: Metrics,
    name: String,
}

#[cfg(feature = "search")]
#[async_trait]
impl SearchBackend for MeteredSearch {
    fn capabilities(&self) -> SearchCapabilities {
        self.inner.capabilities()
    }
    async fn status(&self) -> Result<IndexStatus> {
        let out = self
            .inner
            .status()
            .instrument(tracing::info_span!("search.status", backend = %self.name))
            .await;
        match &out {
            Ok(s) => {
                self.metrics
                    .set_search_fresh(&self.name, s.state == IndexState::Fresh);
                self.metrics
                    .set_search_files(&self.name, s.indexed_files as i64);
            }
            Err(_) => self.metrics.on_search_error(&self.name, "status"),
        }
        out
    }
    async fn reindex(&self, progress: ProgressFn<'_>) -> Result<IndexStatus> {
        let start = Instant::now();
        let out = self
            .inner
            .reindex(progress)
            .instrument(tracing::info_span!("search.reindex", backend = %self.name))
            .await;
        match &out {
            Ok(s) => self.metrics.observe_reindex(
                &self.name,
                start.elapsed().as_secs_f64(),
                s.indexed_files as i64,
            ),
            Err(_) => self.metrics.on_search_error(&self.name, "reindex"),
        }
        out
    }
    async fn query(&self, q: &SearchQuery) -> Result<Vec<SearchHit>> {
        let start = Instant::now();
        let out = self
            .inner
            .query(q)
            .instrument(
                tracing::info_span!("search.query", backend = %self.name, mode = q.mode.as_str()),
            )
            .await;
        let seconds = start.elapsed().as_secs_f64();
        match &out {
            Ok(hits) => {
                self.metrics
                    .on_search_query(&self.name, q.mode.as_str(), seconds, hits.len())
            }
            Err(_) => self.metrics.on_search_error(&self.name, "query"),
        }
        out
    }
    async fn list_files(&self, globs: &[String]) -> Result<Vec<std::path::PathBuf>> {
        let start = Instant::now();
        let out = self
            .inner
            .list_files(globs)
            .instrument(tracing::info_span!("search.list_files", backend = %self.name))
            .await;
        let seconds = start.elapsed().as_secs_f64();
        match &out {
            Ok(paths) => {
                self.metrics
                    .on_search_query(&self.name, "list_files", seconds, paths.len())
            }
            Err(_) => self.metrics.on_search_error(&self.name, "list_files"),
        }
        out
    }
}

// --- git / repo ------------------------------------------------------------

#[cfg(feature = "git")]
struct MeteredRepo {
    inner: Arc<dyn RepoBackend>,
    metrics: Metrics,
    name: String,
}

#[cfg(feature = "git")]
impl MeteredRepo {
    /// A tracing span for a repo op, attributed by backend + op (so a remote git
    /// gateway and a local repo read distinctly in the trace tree).
    fn span(&self, op: &'static str) -> tracing::Span {
        tracing::info_span!("repo.op", backend = %self.name, op)
    }

    /// Record an op's latency + error, keyed by op name. Returns the result so it
    /// can be used inline: `self.record("resolve", start, out)`.
    fn record<T>(&self, op: &str, start: Instant, out: Result<T>) -> Result<T> {
        self.metrics
            .on_repo_op(&self.name, op, start.elapsed().as_secs_f64());
        if out.is_err() {
            self.metrics.on_repo_error(&self.name, op);
        }
        out
    }
}

#[cfg(feature = "git")]
#[async_trait]
impl RepoBackend for MeteredRepo {
    async fn resolve(&self, rev: &Revision) -> Result<Oid> {
        let start = Instant::now();
        let out = self
            .inner
            .resolve(rev)
            .instrument(self.span("resolve"))
            .await;
        self.record("resolve", start, out)
    }
    async fn read_file(&self, rev: &Revision, path: &std::path::Path) -> Result<BlobContent> {
        let start = Instant::now();
        let out = self
            .inner
            .read_file(rev, path)
            .instrument(self.span("read_file"))
            .await;
        self.record("read_file", start, out)
    }
    async fn list_tree(
        &self,
        rev: &Revision,
        path: &std::path::Path,
        recursive: bool,
    ) -> Result<Vec<TreeEntry>> {
        let start = Instant::now();
        let out = self
            .inner
            .list_tree(rev, path, recursive)
            .instrument(self.span("list_tree"))
            .await;
        self.record("list_tree", start, out)
    }
    async fn diff(
        &self,
        base: &Revision,
        target: &Revision,
        path_globs: &[String],
    ) -> Result<DiffResult> {
        let start = Instant::now();
        let out = self
            .inner
            .diff(base, target, path_globs)
            .instrument(self.span("diff"))
            .await;
        self.record("diff", start, out)
    }
    async fn grep(
        &self,
        rev: &Revision,
        pattern: &str,
        path_globs: &[String],
        limit: usize,
    ) -> Result<Vec<GrepHit>> {
        let start = Instant::now();
        let out = self
            .inner
            .grep(rev, pattern, path_globs, limit)
            .instrument(self.span("grep"))
            .await;
        self.record("grep", start, out)
    }
    async fn log(
        &self,
        rev: &Revision,
        path: Option<&std::path::Path>,
        limit: usize,
    ) -> Result<Vec<CommitInfo>> {
        let start = Instant::now();
        let out = self
            .inner
            .log(rev, path, limit)
            .instrument(self.span("log"))
            .await;
        self.record("log", start, out)
    }
    async fn branches(&self) -> Result<Vec<(String, Oid)>> {
        let start = Instant::now();
        let out = self
            .inner
            .branches()
            .instrument(self.span("branches"))
            .await;
        self.record("branches", start, out)
    }
    async fn status(&self) -> Result<RepoStatus> {
        let start = Instant::now();
        let out = self.inner.status().instrument(self.span("status")).await;
        if let Ok(st) = &out {
            self.metrics
                .set_repo_worktrees(&self.name, st.live_worktrees as i64);
        }
        self.record("status", start, out)
    }
    async fn fetch(&self) -> Result<RepoStatus> {
        let start = Instant::now();
        let out = self.inner.fetch().instrument(self.span("fetch")).await;
        let seconds = start.elapsed().as_secs_f64();
        self.metrics.observe_repo_fetch(&self.name, seconds);
        if let Ok(st) = &out {
            self.metrics
                .set_repo_worktrees(&self.name, st.live_worktrees as i64);
        }
        self.record("fetch", start, out)
    }
    async fn worktree_add(&self, spec: &WorktreeSpec) -> Result<WorktreeHandle> {
        let start = Instant::now();
        let out = self
            .inner
            .worktree_add(spec)
            .instrument(self.span("worktree_add"))
            .await;
        self.record("worktree_add", start, out)
    }
    async fn worktree_list(&self) -> Result<Vec<WorktreeHandle>> {
        let start = Instant::now();
        let out = self
            .inner
            .worktree_list()
            .instrument(self.span("worktree_list"))
            .await;
        if let Ok(ws) = &out {
            self.metrics.set_repo_worktrees(&self.name, ws.len() as i64);
        }
        self.record("worktree_list", start, out)
    }
    async fn worktree_remove(&self, id: &str) -> Result<()> {
        let start = Instant::now();
        let out = self
            .inner
            .worktree_remove(id)
            .instrument(self.span("worktree_remove"))
            .await;
        self.record("worktree_remove", start, out)
    }
    async fn checkpoint(&self, worktree_id: &str, name: &str) -> Result<Checkpoint> {
        let start = Instant::now();
        let out = self
            .inner
            .checkpoint(worktree_id, name)
            .instrument(self.span("checkpoint"))
            .await;
        self.record("checkpoint", start, out)
    }
    async fn push(&self, checkpoint: &Checkpoint, remote_ref: &str) -> Result<()> {
        let start = Instant::now();
        let out = self
            .inner
            .push(checkpoint, remote_ref)
            .instrument(self.span("push"))
            .await;
        self.record("push", start, out)
    }
}

/// Wrap a [`ReferenceResolver`](agent_core::ReferenceResolver) so each `resolve`
/// emits a `reference.resolve` span (`refs`/`blocked` attrs), records the
/// expansion latency, counts each resolved reference by `source`-derived kind +
/// outcome (block vs warn), and counts a budget-blocked expansion. Labels are the
/// bounded kind/outcome enums, never the attacker-controlled reference target.
#[cfg(feature = "reference")]
pub(crate) fn reference(
    inner: Arc<dyn agent_core::ReferenceResolver>,
    m: Metrics,
) -> Arc<dyn agent_core::ReferenceResolver> {
    Arc::new(MeteredReference { inner, metrics: m })
}

#[cfg(feature = "reference")]
struct MeteredReference {
    inner: Arc<dyn agent_core::ReferenceResolver>,
    metrics: Metrics,
}

#[cfg(feature = "reference")]
#[async_trait]
impl agent_core::ReferenceResolver for MeteredReference {
    async fn resolve(&self, prompt: &str, budget_tokens: usize) -> agent_core::Resolution {
        let span = tracing::info_span!(
            "reference.resolve",
            refs = tracing::field::Empty,
            blocked = tracing::field::Empty,
        );
        let start = Instant::now();
        let res = self
            .inner
            .resolve(prompt, budget_tokens)
            .instrument(span.clone())
            .await;
        self.metrics
            .on_reference_resolve(start.elapsed().as_secs_f64());
        // The `source` prefix ("file:"/"dir:"/"symbol:"/"url:") is the bounded kind.
        for b in &res.blocks {
            let kind = b.source.split_once(':').map(|(k, _)| k).unwrap_or("other");
            self.metrics.on_reference_ref(kind, "block");
        }
        for _ in &res.warnings {
            self.metrics.on_reference_ref("other", "warn");
        }
        if res.blocked {
            self.metrics.on_reference_blocked();
        }
        span.record("refs", res.blocks.len());
        span.record("blocked", res.blocked);
        res
    }
}

#[cfg(all(test, feature = "tool-patch"))]
mod tests {
    use super::*;
    use agent_testkit::observe::MetricsProbe;
    use agent_testkit::tempdir;

    // A metered tool must record `agent_tool_exec_seconds` (labelled by tool). Uses
    // a real `apply_patch` so the feature is proven observable end-to-end, not just
    // correct — the "prove it's observable" step of the per-feature pattern.
    #[tokio::test]
    async fn metered_tool_records_exec_metric() {
        let dir = tempdir();
        std::fs::write(dir.join("f.txt"), "before\n").unwrap();
        let metrics = Metrics::new();
        let probe = MetricsProbe::new(&metrics);
        let tool = super::tool(Arc::new(agent_tools::ApplyPatchTool), metrics.clone());

        let ctx = ToolContext { cwd: dir.clone() };
        let obs = tool
            .execute(
                serde_json::json!({
                    "patch": "*** Begin Patch\n*** Update File: f.txt\n@@\n-before\n+after\n*** End Patch"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!obs.is_error, "{}", obs.content);

        assert!(
            probe.delta(
                &metrics,
                "agent_tool_exec_seconds_count",
                Some("tool=\"apply_patch\"")
            ) >= 1.0,
            "apply_patch execution should record the tool-exec metric"
        );
    }
}

// A local seam op must now emit a tracing span through its metered wrapper (was
// metrics-only). Proven for `search.query`; the same `.instrument(...)` pattern
// covers the other search/repo/memory ops.
#[cfg(all(test, feature = "search"))]
mod span_tests {
    use super::*;
    use agent_core::SearchQuery;
    use agent_testkit::observe::captured_spans;
    use agent_testkit::FixtureSearch;

    #[test]
    fn metered_search_query_emits_span() {
        let spans = captured_spans(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(async {
                let backend =
                    super::search(Arc::new(FixtureSearch::new()), Metrics::new(), "fixture");
                let q = SearchQuery {
                    text: "x".into(),
                    mode: agent_core::SearchMode::Literal,
                    path_globs: vec![],
                    lang: None,
                    limit: 5,
                    fuzzy_distance: None,
                };
                let _ = backend.query(&q).await;
            });
        });
        assert!(spans.contains(&"search.query".to_string()), "{spans:?}");
    }
}

// web_fetch: the metered WebBackend emits a `web.fetch` span with attributes, and
// the Policy guard denies an SSRF target *before* the tool fetches — proving the
// SSRF screen and the tool compose the way the loop wires them.
#[cfg(all(test, feature = "web"))]
mod web_tests {
    use super::*;
    use crate::policy::{guard, AutoApprove, GuardMode};
    use agent_core::{
        Decision, Observation, Policy, Tool, ToolCall, ToolContext, WebFormat, WebRequest,
    };
    use agent_testkit::observe::captured_span_fields;
    use agent_testkit::FakeWebBackend;
    use serde_json::json;

    // The metered WebBackend emits `web.fetch` carrying host/format/status/bytes.
    #[test]
    fn metered_web_emits_span_with_attributes() {
        let _lock = super::callsite_guard();
        let fields = captured_span_fields(|| {
            // Another test creates `web.fetch` under a non-recording subscriber,
            // caching this callsite's interest as disabled; force a re-evaluation
            // against the recording subscriber so the fields are captured here.
            tracing::callsite::rebuild_interest_cache();
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(async {
                let inner = Arc::new(FakeWebBackend::new().with_response(
                    "https://example.com/doc",
                    "text/html",
                    "<h1>Hi</h1>",
                ));
                let backend = super::web(inner, Metrics::new());
                let req = WebRequest {
                    url: "https://example.com/doc".into(),
                    format: WebFormat::Markdown,
                    timeout_secs: 30,
                    max_bytes: 1 << 20,
                    max_redirects: 5,
                };
                let _ = backend.fetch(&req).await;
            });
        });
        let has = |f: &str, v: &str| {
            fields
                .iter()
                .any(|(s, fld, val)| s == "web.fetch" && fld == f && val == v)
        };
        assert!(has("host", "example.com"), "{fields:?}");
        assert!(has("format", "markdown"), "{fields:?}");
        assert!(has("status", "200"), "{fields:?}");
        assert!(
            fields
                .iter()
                .any(|(s, fld, _)| s == "web.fetch" && fld == "bytes"),
            "bytes attribute missing: {fields:?}"
        );
    }

    // Mirror the agent loop: authorize the call, then execute only if allowed.
    async fn run_guarded(
        g: &Arc<dyn Policy>,
        tool: &agent_tools::WebFetchTool,
        url: &str,
    ) -> Observation {
        let call = ToolCall {
            id: "c0".into(),
            name: "web_fetch".into(),
            arguments: json!({ "url": url }),
        };
        match g.authorize(&call).await {
            Decision::Allow => tool
                .execute(call.arguments, &ToolContext { cwd: ".".into() })
                .await
                .unwrap(),
            Decision::Deny(r) => Observation::error(r),
        }
    }

    // Sync `#[test]` (not `#[tokio::test]`) so the callsite lock isn't held across
    // an await point — it serializes this metered-fetch path (which primes the
    // `web.fetch` callsite under no subscriber) against the span-capturing test.
    #[test]
    fn guard_denies_ssrf_but_allows_public() {
        let _lock = super::callsite_guard();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            let backend = Arc::new(
                FakeWebBackend::new()
                    .with_response("http://169.254.169.254/meta", "text/plain", "SECRET")
                    .with_response("https://example.com/doc", "text/html", "<h1>Hi</h1>"),
            );
            let tool = agent_tools::WebFetchTool::new(
                super::web(backend.clone(), Metrics::new()),
                1 << 20,
                30,
                120,
                5,
            );
            let g: Arc<dyn Policy> = guard(
                Arc::new(AutoApprove),
                GuardMode::Deny,
                vec![],
                vec![],
                false,
                vec![],
                None,
                Metrics::new(),
            );

            // SSRF metadata target: denied by the guard, opaque reason, never fetched.
            let denied = run_guarded(&g, &tool, "http://169.254.169.254/meta").await;
            assert!(denied.is_error);
            assert!(
                denied.content.contains("blocked by policy guard"),
                "{}",
                denied.content
            );

            // Public target: allowed, fetched, converted to markdown.
            let ok = run_guarded(&g, &tool, "https://example.com/doc").await;
            assert!(!ok.is_error, "{}", ok.content);
            assert!(ok.content.contains("# Hi"), "{}", ok.content);

            // Only the public URL reached the backend — the SSRF target never did.
            assert_eq!(backend.requested(), vec!["https://example.com/doc"]);
        });
    }
}

// tasks: the metered TaskTracker refreshes the open/closed gauges and emits a
// `tasks.write` span. Uses the real in-memory tracker (dogfoods it).
#[cfg(all(test, feature = "tasks"))]
mod tasks_tests {
    use super::*;
    use agent_core::{Todo, TodoPatch, TodoPriority, TodoStatus};
    use agent_testkit::observe::{captured_spans, MetricsProbe};

    fn todo(content: &str, status: TodoStatus, priority: TodoPriority) -> Todo {
        Todo {
            content: content.into(),
            status,
            priority,
        }
    }

    // Sync `#[test]` (not `#[tokio::test]`): the callsite lock must not span an
    // await; this primes the `tasks.write` callsite under no subscriber, so it is
    // serialized against the span-capturing test.
    #[test]
    fn metered_tasks_gauge_reflects_plan() {
        let _lock = super::callsite_guard();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            let metrics = Metrics::new();
            let probe = MetricsProbe::new(&metrics);
            let tracker = super::tasks(
                Arc::new(agent_tasks::MemoryTaskTracker::new()),
                metrics.clone(),
            );

            // 2 open (in_progress + pending), 1 closed (completed).
            tracker
                .write(vec![
                    todo("a", TodoStatus::InProgress, TodoPriority::High),
                    todo("b", TodoStatus::Pending, TodoPriority::Low),
                    todo("c", TodoStatus::Completed, TodoPriority::High),
                ])
                .await
                .unwrap();
            assert_eq!(probe.delta(&metrics, "agent_tasks_open", None), 2.0);
            assert_eq!(probe.delta(&metrics, "agent_tasks_closed", None), 1.0);

            // c -> cancelled, a -> completed: open drops to 1 (b), closed rises to 2.
            tracker
                .update(TodoPatch {
                    content: "c".into(),
                    status: Some(TodoStatus::Cancelled),
                    priority: None,
                })
                .await
                .unwrap();
            tracker
                .update(TodoPatch {
                    content: "a".into(),
                    status: Some(TodoStatus::Completed),
                    priority: None,
                })
                .await
                .unwrap();
            assert_eq!(probe.delta(&metrics, "agent_tasks_open", None), 1.0);
            assert_eq!(probe.delta(&metrics, "agent_tasks_closed", None), 2.0);
        });
    }

    #[test]
    fn metered_tasks_emits_write_span() {
        let _lock = super::callsite_guard();
        let spans = captured_spans(|| {
            // Guard against the callsite-interest cache being primed disabled by a
            // sibling test that ran the tracker under no recording subscriber.
            tracing::callsite::rebuild_interest_cache();
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(async {
                let tracker = super::tasks(
                    Arc::new(agent_tasks::MemoryTaskTracker::new()),
                    Metrics::new(),
                );
                let _ = tracker
                    .write(vec![todo("a", TodoStatus::Pending, TodoPriority::High)])
                    .await;
            });
        });
        assert!(spans.contains(&"tasks.write".to_string()), "{spans:?}");
    }
}

// structured output: the metered validator emits a `structured.validate` span
// carrying the ok/errors attributes on both the pass and fail paths.
#[cfg(all(test, feature = "structured"))]
mod structured_tests {
    use super::*;
    use agent_testkit::observe::captured_span_fields;
    use serde_json::json;

    #[test]
    fn metered_validator_emits_span_with_attributes() {
        let _lock = super::callsite_guard();
        let fields = captured_span_fields(|| {
            tracing::callsite::rebuild_interest_cache();
            let v = super::validator(
                Arc::new(agent_validate::Draft07Validator::new()),
                Metrics::new(),
            );
            let schema = json!({"type": "object", "required": ["n"],
                                "properties": {"n": {"type": "integer"}}});
            let _ = v.validate(&schema, &json!({"n": 1})); // ok = true
            let _ = v.validate(&schema, &json!({})); // ok = false, errors >= 1
        });
        let has = |f: &str, val: &str| {
            fields
                .iter()
                .any(|(s, fld, v)| s == "structured.validate" && fld == f && v == val)
        };
        assert!(has("ok", "true"), "{fields:?}");
        assert!(has("ok", "false"), "{fields:?}");
        assert!(
            fields
                .iter()
                .any(|(s, fld, _)| s == "structured.validate" && fld == "errors"),
            "errors attr missing: {fields:?}"
        );
    }
}

// lsp: the metered LspBackend emits an `lsp.request` span and meters per-method
// latency + diagnostics by severity.
#[cfg(all(test, feature = "lsp"))]
mod lsp_tests {
    use super::*;
    use agent_core::{
        Diagnostic, DiagnosticSeverity, LspBackend, LspCapabilities, LspMethod, LspRequest,
        LspResult, Range,
    };
    use agent_testkit::observe::{captured_spans, MetricsProbe};

    struct FakeLsp;
    #[async_trait]
    impl LspBackend for FakeLsp {
        fn capabilities(&self, _language: &str) -> LspCapabilities {
            LspCapabilities::default()
        }
        async fn open(&self, _uri: &str, _text: &str) -> Result<()> {
            Ok(())
        }
        async fn request(&self, _req: &LspRequest) -> Result<LspResult> {
            Ok(LspResult::Diagnostics(vec![Diagnostic {
                range: Range::default(),
                severity: DiagnosticSeverity::Warning,
                message: "x".into(),
                code: None,
                source: None,
            }]))
        }
        async fn shutdown(&self) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn metered_lsp_emits_span_and_meters_diagnostics() {
        let _lock = super::callsite_guard();
        let metrics = Metrics::new();
        let probe = MetricsProbe::new(&metrics);
        let spans = captured_spans(|| {
            tracing::callsite::rebuild_interest_cache();
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(async {
                let backend = super::lsp(Arc::new(FakeLsp), metrics.clone());
                let req = LspRequest {
                    method: LspMethod::Diagnostics,
                    uri: "file:///a.rs".into(),
                    position: None,
                    new_name: None,
                };
                let _ = backend.request(&req).await;
            });
        });
        assert!(spans.contains(&"lsp.request".to_string()), "{spans:?}");
        assert_eq!(
            probe.delta(
                &metrics,
                "agent_lsp_diagnostics_total",
                Some("severity=\"warning\"")
            ),
            1.0
        );
        assert!(
            probe.delta(
                &metrics,
                "agent_lsp_request_seconds_count",
                Some("method=\"diagnostics\"")
            ) >= 1.0
        );
    }
}

// sandbox: the metered Sandbox emits a `sandbox.exec` span (backend attr) and
// meters per-backend exec latency + outcome.
#[cfg(all(test, feature = "tool-core"))]
mod sandbox_tests {
    use super::*;
    use agent_core::{ExecOutput, ExecSpec, Sandbox, SandboxCapabilities};
    use agent_testkit::observe::{captured_spans, MetricsProbe};

    struct FakeSandbox;
    #[async_trait]
    impl Sandbox for FakeSandbox {
        async fn exec(&self, _spec: &ExecSpec) -> Result<ExecOutput> {
            Ok(ExecOutput {
                stdout: "ok".into(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: false,
            })
        }
        fn capabilities(&self) -> SandboxCapabilities {
            SandboxCapabilities {
                backend: "fake".into(),
                ..Default::default()
            }
        }
    }

    #[test]
    fn metered_sandbox_emits_span_and_meters() {
        let _lock = super::callsite_guard();
        let metrics = Metrics::new();
        let probe = MetricsProbe::new(&metrics);
        let spans = captured_spans(|| {
            tracing::callsite::rebuild_interest_cache();
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(async {
                let sb = super::sandbox(Arc::new(FakeSandbox), metrics.clone());
                let _ = sb.exec(&ExecSpec::sh("echo hi", ".")).await;
            });
        });
        assert!(spans.contains(&"sandbox.exec".to_string()), "{spans:?}");
        assert!(
            probe.delta(
                &metrics,
                "agent_sandbox_exec_total",
                Some("backend=\"fake\"")
            ) >= 1.0
        );
    }
}

// session: the metered SessionStore emits a `session.checkpoint` span and meters
// the op.
#[cfg(all(test, feature = "session"))]
mod session_tests {
    use super::*;
    use agent_core::{Message, Role, WorkingSet};
    use agent_testkit::observe::{captured_spans, MetricsProbe};
    use agent_testkit::tempdir;

    #[test]
    fn metered_session_emits_span_and_meters() {
        let _lock = super::callsite_guard();
        let metrics = Metrics::new();
        let probe = MetricsProbe::new(&metrics);
        let dir = tempdir();
        let spans = captured_spans(|| {
            tracing::callsite::rebuild_interest_cache();
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(async {
                let store = super::session(
                    Arc::new(agent_session::FileSessionStore::new(dir.clone())),
                    metrics.clone(),
                );
                let ws = WorkingSet {
                    messages: vec![Message {
                        role: Role::User,
                        content: vec![agent_core::ContentBlock::text("hi")],
                        tool_calls: vec![],
                        tool_call_id: None,
                    }],
                };
                let _ = store.checkpoint("s", &ws, "t").await;
            });
        });
        assert!(
            spans.contains(&"session.checkpoint".to_string()),
            "{spans:?}"
        );
        assert!(
            probe.delta(
                &metrics,
                "agent_session_ops_total",
                Some("op=\"checkpoint\"")
            ) >= 1.0
        );
    }
}

// The metered tokenizer must emit a `tokenizer.count` span (the observability
// half of parity spec 23), mirroring the `search.query` proof above.
#[cfg(all(test, feature = "tokenizer"))]
mod tokenizer_span_tests {
    use super::*;
    use agent_testkit::observe::captured_spans;
    use agent_testkit::FixedVocabTokenizer;

    #[test]
    fn metered_tokenizer_count_emits_span() {
        let spans = captured_spans(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(async {
                let tok = super::tokenizer(Arc::new(FixedVocabTokenizer));
                let _ = tok.count("one two three", "some-model").await;
            });
        });
        assert!(spans.contains(&"tokenizer.count".to_string()), "{spans:?}");
    }
}

/// Wrap a [`Scanner`](agent_core::Scanner) so each `scan` emits a `scanner.scan`
/// span (`kind`, finding count, max severity) and counts findings by
/// `(severity, rule, kind)`. Labels are the bounded rule/severity/kind enums —
/// never the scanned content or the matched bytes.
#[cfg(feature = "scanner")]
pub(crate) fn scanner(
    inner: Arc<dyn agent_core::Scanner>,
    m: Metrics,
) -> Arc<dyn agent_core::Scanner> {
    Arc::new(MeteredScanner { inner, metrics: m })
}

#[cfg(feature = "scanner")]
struct MeteredScanner {
    inner: Arc<dyn agent_core::Scanner>,
    metrics: Metrics,
}

#[cfg(feature = "scanner")]
#[async_trait]
impl agent_core::Scanner for MeteredScanner {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn scan(&self, kind: agent_core::ScanKind, content: &str) -> Vec<agent_core::Finding> {
        let span = tracing::info_span!(
            "scanner.scan",
            kind = kind.as_str(),
            findings = tracing::field::Empty,
            max_severity = tracing::field::Empty,
        );
        let start = Instant::now();
        let findings = self
            .inner
            .scan(kind, content)
            .instrument(span.clone())
            .await;
        self.metrics.on_scan(start.elapsed().as_secs_f64());
        span.record("findings", findings.len());
        if let Some(worst) = agent_core::max_severity(&findings) {
            span.record("max_severity", worst.as_str());
        }
        for f in &findings {
            self.metrics
                .on_scanner_finding(f.severity.as_str(), &f.rule, kind.as_str());
        }
        findings
    }
}

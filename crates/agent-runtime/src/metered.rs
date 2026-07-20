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

//! `agent-testkit` — shared test doubles for the agent seams.
//!
//! Every seam in `agent-core` is a trait, which makes them trivial to fake — but
//! until now each test hand-rolled its own mock provider, recording memory, and
//! echo tool. This crate collects those doubles in one place so a contributor
//! testing a new seam impl (or the loop against it) reaches for a ready-made fake
//! instead of copy-pasting one.
//!
//! It is a **dev-dependency only** — nothing here belongs in a release build.
//!
//! * [`ScriptedProvider`] / [`FnProvider`] — fake `LlmProvider`s.
//! * [`RecordingMemory`] — a `MemoryStore` that captures appended events.
//! * [`StaticContext`] — a trivial `ContextStrategy`.
//! * [`EchoTool`] — a `Tool` that echoes its args (optionally after a delay).
//! * [`mcp::ScriptedTransport`] — a canned `McpTransport` for client tests.
//! * [`bench`] — deterministic input fixtures shared by benches + larger tests.
//! * [`observe`] — assert observability: metric deltas ([`observe::MetricsProbe`])
//!   and emitted spans ([`observe::captured_spans`]).

pub mod bench;
pub mod observe;

use agent_core::{
    BlobContent, Checkpoint, CommitInfo, CompletionRequest, CompletionResponse, ContextInput,
    ContextStrategy, DiffResult, FileDiff, GrepHit, IndexState, IndexStatus, LlmProvider,
    MemoryEvent, MemoryItem, MemoryStore, Message, ModelCapabilities, ModelPrices, Observation,
    Oid, Prices, ProgressFn, RecallQuery, ReindexProgress, RepoBackend, RepoStatus, Result,
    Revision, Role, SearchBackend, SearchCapabilities, SearchHit, SearchMode, SearchQuery,
    TokenBudget, Tokenizer, Tool, ToolCall, ToolContext, ToolSchema, TreeEntry, WorkingSet,
    WorktreeHandle, WorktreeSpec,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Filesystem fixtures
// ---------------------------------------------------------------------------

/// A unique, freshly-created temp directory for a filesystem test. Collision-proof
/// under parallel test runs: the name mixes the process id, a nanosecond clock, and
/// a per-process atomic counter (so two calls in the same process, same nanosecond,
/// still differ). Replaces the nanos-suffixed helper hand-rolled across crates.
pub fn tempdir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut p = std::env::temp_dir();
    p.push(format!("agent-testkit-{}-{nanos}-{n}", std::process::id()));
    std::fs::create_dir_all(&p).expect("create temp dir");
    p
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

/// A provider that replays a fixed sequence of responses — one per `complete`
/// call, in order. Once the script is exhausted the final response repeats, so a
/// loop that runs an extra turn never panics.
pub struct ScriptedProvider {
    responses: Vec<CompletionResponse>,
    next: AtomicUsize,
    caps: ModelCapabilities,
}

impl ScriptedProvider {
    /// Replay `responses` in order.
    pub fn new(responses: Vec<CompletionResponse>) -> Self {
        assert!(
            !responses.is_empty(),
            "ScriptedProvider needs at least one response"
        );
        Self {
            responses,
            next: AtomicUsize::new(0),
            caps: ModelCapabilities {
                supports_tools: true,
                context_window: 1000,
            },
        }
    }

    /// The common two-turn script: request `calls` on the first turn, then answer
    /// `final_text`.
    pub fn tools_then_final(calls: Vec<ToolCall>, final_text: impl Into<String>) -> Self {
        Self::new(vec![tool_turn(calls), final_turn(final_text)])
    }

    /// Override the advertised capabilities (defaults: tools on, 1000-token window).
    pub fn with_capabilities(mut self, caps: ModelCapabilities) -> Self {
        self.caps = caps;
        self
    }
}

#[async_trait]
impl LlmProvider for ScriptedProvider {
    fn capabilities(&self) -> ModelCapabilities {
        self.caps.clone()
    }
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
        let i = self
            .next
            .fetch_add(1, Ordering::SeqCst)
            .min(self.responses.len() - 1);
        Ok(self.responses[i].clone())
    }
}

/// A provider that computes its response from the request via a closure — handy
/// for asserting on what the loop actually sent (e.g. counting user messages).
pub struct FnProvider<F> {
    f: F,
    caps: ModelCapabilities,
}

impl<F> FnProvider<F>
where
    F: Fn(&CompletionRequest) -> CompletionResponse + Send + Sync,
{
    pub fn new(f: F) -> Self {
        Self {
            f,
            caps: ModelCapabilities {
                supports_tools: false,
                context_window: 1000,
            },
        }
    }
    pub fn with_capabilities(mut self, caps: ModelCapabilities) -> Self {
        self.caps = caps;
        self
    }
}

#[async_trait]
impl<F> LlmProvider for FnProvider<F>
where
    F: Fn(&CompletionRequest) -> CompletionResponse + Send + Sync,
{
    fn capabilities(&self) -> ModelCapabilities {
        self.caps.clone()
    }
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        Ok((self.f)(&req))
    }
}

/// A response whose assistant message requests `calls` (finish reason `tool_calls`).
pub fn tool_turn(calls: Vec<ToolCall>) -> CompletionResponse {
    CompletionResponse {
        message: Message {
            role: Role::Assistant,
            content: String::new(),
            tool_calls: calls,
            tool_call_id: None,
        },
        finish_reason: "tool_calls".into(),
        usage: None,
    }
}

/// A terminal response carrying `text` (finish reason `stop`).
pub fn final_turn(text: impl Into<String>) -> CompletionResponse {
    CompletionResponse {
        message: Message::assistant(text),
        finish_reason: "stop".into(),
        usage: None,
    }
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

/// A `MemoryStore` that recalls nothing but records every appended event, so a
/// test can assert on what the loop wrote (and in what order). Cloneable: clones
/// share the same underlying log.
#[derive(Default, Clone)]
pub struct RecordingMemory {
    events: Arc<Mutex<Vec<MemoryEvent>>>,
}

impl RecordingMemory {
    pub fn new() -> Self {
        Self::default()
    }
    /// Every appended event, in append order.
    pub fn events(&self) -> Vec<MemoryEvent> {
        self.events.lock().unwrap().clone()
    }
    /// The `tool_call_id` of each appended `tool` event, in order — the usual
    /// assertion for "did tool results come back in call order?".
    pub fn tool_order(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.kind == "tool")
            .filter_map(|e| e.message.tool_call_id.clone())
            .collect()
    }
}

#[async_trait]
impl MemoryStore for RecordingMemory {
    async fn recall(&self, _q: &RecallQuery) -> Result<Vec<MemoryItem>> {
        Ok(vec![])
    }
    async fn append(&self, event: MemoryEvent) -> Result<()> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
    async fn distill(&self) -> Result<usize> {
        Ok(0)
    }
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

/// A trivial `ContextStrategy`: assemble a system + user message and never
/// compact. Enough to exercise the loop without a real windowing strategy.
pub struct StaticContext;

#[async_trait]
impl ContextStrategy for StaticContext {
    async fn assemble(&self, input: ContextInput) -> Result<Vec<Message>> {
        Ok(vec![
            Message::system(input.system_prompt),
            Message::user(input.goal),
        ])
    }
    async fn compact(&self, _working: &mut WorkingSet, _budget: &TokenBudget) -> Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tokenizer + prices
// ---------------------------------------------------------------------------

/// A deterministic `Tokenizer` for tests: 1 token per whitespace-delimited chunk,
/// counted by `char` (so it never inflates on multi-byte UTF-8). Exact and
/// byte-stable, so a test can assert precise counts (and the compaction boundary)
/// without shipping a real BPE vocab. `backend()` is `"fixed"`.
pub struct FixedVocabTokenizer;

#[async_trait]
impl Tokenizer for FixedVocabTokenizer {
    fn backend(&self) -> &str {
        "fixed"
    }
    async fn count(&self, text: &str, _model: &str) -> Result<u32> {
        Ok(text.split_whitespace().count() as u32)
    }
}

/// A fixed `{model → ModelPrices}` map implementing the [`Prices`] seam, for cost
/// tests. Mirrors the `ScriptedProvider`/`StaticContext` style — canned data, no
/// I/O — so `calculate_cost` can be exercised deterministically.
pub struct StaticPrices {
    rows: HashMap<String, ModelPrices>,
}

impl StaticPrices {
    pub fn new(rows: impl IntoIterator<Item = (String, ModelPrices)>) -> Self {
        Self {
            rows: rows.into_iter().collect(),
        }
    }

    /// A one-model table with the four `$/MTok` rates spelled out — the common
    /// case for a single `cost_cases` row.
    pub fn one(model: &str, input: f64, output: f64, cache_read: f64, cache_write: f64) -> Self {
        Self::new([(
            model.to_string(),
            ModelPrices {
                input,
                output,
                cache_read,
                cache_write,
            },
        )])
    }
}

impl Prices for StaticPrices {
    fn get(&self, model: &str) -> Option<ModelPrices> {
        self.rows.get(model).copied()
    }
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

/// A tool named `echo` that returns its `val` argument after sleeping `sleep_ms`
/// milliseconds. The optional delay lets a test make tool completion order differ
/// from call order (to prove the loop preserves call order).
pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "echo".into(),
            description: "echo the `val` arg (after an optional `sleep_ms` delay)".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "val": {"type": "string"},
                    "sleep_ms": {"type": "integer"}
                }
            }),
        }
    }
    async fn execute(&self, args: serde_json::Value, _ctx: &ToolContext) -> Result<Observation> {
        let ms = args.get("sleep_ms").and_then(|v| v.as_u64()).unwrap_or(0);
        if ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        }
        Ok(Observation::ok(
            args.get("val")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// A `SearchBackend` that returns a fixed hit list + settable index status, and
/// streams a canned reindex progression. Cloneable-cheap via `Arc` counters so a
/// test can assert a reindex happened. Enough to exercise the search seam (and its
/// gRPC transport) without pulling in tantivy.
pub struct FixtureSearch {
    caps: SearchCapabilities,
    status: IndexStatus,
    hits: Vec<SearchHit>,
    files: Vec<PathBuf>,
    reindexed: Arc<AtomicUsize>,
}

impl Default for FixtureSearch {
    fn default() -> Self {
        Self {
            caps: SearchCapabilities {
                backend: "fixture".into(),
                modes: vec![
                    SearchMode::Literal,
                    SearchMode::Phrase,
                    SearchMode::Fuzzy,
                    SearchMode::Regex,
                ],
                content_search: true,
                scored: true,
                incremental: true,
                max_concurrent_queries: 0,
            },
            status: IndexStatus {
                state: IndexState::Fresh,
                indexed_files: 3,
                last_indexed_ms: 0,
                manifest_digest: "fixture".into(),
            },
            hits: Vec::new(),
            files: Vec::new(),
            reindexed: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl FixtureSearch {
    pub fn new() -> Self {
        Self::default()
    }
    /// The file paths `list_files` returns (globs are ignored by the fixture).
    pub fn with_files(mut self, files: Vec<PathBuf>) -> Self {
        self.files = files;
        self
    }
    /// The hits every `query` returns.
    pub fn with_hits(mut self, hits: Vec<SearchHit>) -> Self {
        self.hits = hits;
        self
    }
    /// The status `status()` reports.
    pub fn with_status(mut self, status: IndexStatus) -> Self {
        self.status = status;
        self
    }
    /// How many times `reindex` has been called.
    pub fn reindex_count(&self) -> usize {
        self.reindexed.load(Ordering::SeqCst)
    }
    /// A single hit, for convenience in assertions.
    pub fn hit(path: &str, line: u32, snippet: &str) -> SearchHit {
        SearchHit {
            path: PathBuf::from(path),
            line,
            col_start: 0,
            col_end: 0,
            score: 1.0,
            snippet: snippet.into(),
        }
    }
}

#[async_trait]
impl SearchBackend for FixtureSearch {
    fn capabilities(&self) -> SearchCapabilities {
        self.caps.clone()
    }
    async fn status(&self) -> Result<IndexStatus> {
        Ok(self.status.clone())
    }
    async fn reindex(&self, progress: ProgressFn<'_>) -> Result<IndexStatus> {
        self.reindexed.fetch_add(1, Ordering::SeqCst);
        progress(ReindexProgress {
            files_done: 1,
            files_total: 2,
            done: false,
        });
        progress(ReindexProgress {
            files_done: 2,
            files_total: 2,
            done: true,
        });
        Ok(self.status.clone())
    }
    async fn query(&self, _q: &SearchQuery) -> Result<Vec<SearchHit>> {
        Ok(self.hits.clone())
    }
    async fn list_files(&self, _globs: &[String]) -> Result<Vec<PathBuf>> {
        Ok(self.files.clone())
    }
}

// ---------------------------------------------------------------------------
// Git (RepoBackend)
// ---------------------------------------------------------------------------

/// A `RepoBackend` double: canned branches + blobs + diff for the object-read
/// methods, and an in-memory worktree list that `worktree_add`/`worktree_remove`
/// mutate. Enough to exercise the git seam (and, later, its gRPC transport)
/// without a real repo. Cloneable: clones share the worktree list.
#[derive(Clone, Default)]
pub struct FixtureRepo {
    branches: Vec<(String, Oid)>,
    /// `"rev:path"` → file text.
    blobs: std::collections::HashMap<String, String>,
    diff: Vec<FileDiff>,
    worktrees: Arc<Mutex<Vec<WorktreeHandle>>>,
}

impl FixtureRepo {
    pub fn new() -> Self {
        Self::default()
    }
    /// Add a branch and its head oid.
    pub fn with_branch(mut self, name: impl Into<String>, oid: impl Into<String>) -> Self {
        self.branches.push((name.into(), Oid(oid.into())));
        self
    }
    /// Set a file's text at a revision (looked up by `read_file`).
    pub fn with_blob(
        mut self,
        rev: impl Into<String>,
        path: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        self.blobs
            .insert(format!("{}:{}", rev.into(), path.into()), text.into());
        self
    }
    /// Set the file list `diff` returns.
    pub fn with_diff(mut self, files: Vec<FileDiff>) -> Self {
        self.diff = files;
        self
    }
    /// A deterministic 40-hex oid from a seed (no randomness).
    pub fn fake_oid(seed: &str) -> Oid {
        let mut h: String = seed.bytes().map(|b| format!("{b:02x}")).collect();
        h.truncate(40);
        while h.len() < 40 {
            h.push('0');
        }
        Oid(h)
    }
}

#[async_trait]
impl RepoBackend for FixtureRepo {
    async fn resolve(&self, rev: &Revision) -> Result<Oid> {
        Ok(Self::fake_oid(rev.as_str()))
    }
    async fn read_file(&self, rev: &Revision, path: &std::path::Path) -> Result<BlobContent> {
        let key = format!("{}:{}", rev.as_str(), path.to_string_lossy());
        match self.blobs.get(&key) {
            Some(text) => Ok(BlobContent {
                oid: Self::fake_oid(&key),
                path: path.to_path_buf(),
                bytes_len: text.len() as u64,
                is_binary: false,
                text: text.clone(),
            }),
            None => Err(agent_core::Error::Repo(format!("no blob `{key}`"))),
        }
    }
    async fn list_tree(
        &self,
        _rev: &Revision,
        _path: &std::path::Path,
        _recursive: bool,
    ) -> Result<Vec<TreeEntry>> {
        Ok(vec![])
    }
    async fn diff(
        &self,
        base: &Revision,
        target: &Revision,
        _globs: &[String],
    ) -> Result<DiffResult> {
        Ok(DiffResult {
            base: Self::fake_oid(base.as_str()),
            target: Self::fake_oid(target.as_str()),
            files: self.diff.clone(),
        })
    }
    async fn grep(
        &self,
        _rev: &Revision,
        _pattern: &str,
        _globs: &[String],
        _limit: usize,
    ) -> Result<Vec<GrepHit>> {
        Ok(vec![])
    }
    async fn log(
        &self,
        _rev: &Revision,
        _path: Option<&std::path::Path>,
        _limit: usize,
    ) -> Result<Vec<CommitInfo>> {
        Ok(vec![])
    }
    async fn branches(&self) -> Result<Vec<(String, Oid)>> {
        Ok(self.branches.clone())
    }
    async fn status(&self) -> Result<RepoStatus> {
        Ok(RepoStatus {
            mirror_path: std::path::PathBuf::from("/fixture"),
            last_fetch_ms: 0,
            live_worktrees: self.worktrees.lock().unwrap().len() as u32,
            heads: self.branches.iter().cloned().collect(),
        })
    }
    async fn fetch(&self) -> Result<RepoStatus> {
        self.status().await
    }
    async fn worktree_add(&self, spec: &WorktreeSpec) -> Result<WorktreeHandle> {
        let id = spec
            .id
            .clone()
            .unwrap_or_else(|| format!("wt{}", self.worktrees.lock().unwrap().len()));
        let handle = WorktreeHandle {
            id: id.clone(),
            path: std::path::PathBuf::from(format!("/fixture/worktrees/{id}")),
            head: Self::fake_oid(spec.revision.as_str()),
            revision: spec.revision.clone(),
            writable: spec.writable,
        };
        self.worktrees.lock().unwrap().push(handle.clone());
        Ok(handle)
    }
    async fn worktree_list(&self) -> Result<Vec<WorktreeHandle>> {
        Ok(self.worktrees.lock().unwrap().clone())
    }
    async fn worktree_remove(&self, id: &str) -> Result<()> {
        self.worktrees.lock().unwrap().retain(|w| w.id != id);
        Ok(())
    }
    async fn checkpoint(&self, worktree_id: &str, name: &str) -> Result<Checkpoint> {
        Ok(Checkpoint {
            name: name.to_string(),
            oid: Self::fake_oid(&format!("{worktree_id}:{name}")),
            ref_name: format!("refs/agent/checkpoints/{worktree_id}/{name}"),
        })
    }
    async fn push(&self, _checkpoint: &Checkpoint, _remote_ref: &str) -> Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MCP transport
// ---------------------------------------------------------------------------

/// Test doubles for the MCP client's transport seam.
pub mod mcp {
    use agent_mcp::{McpTransport, Result};
    use async_trait::async_trait;
    use serde_json::Value;
    use std::collections::HashMap;

    /// A transport that answers requests from a canned `method → result` map
    /// (unmapped methods return JSON `null`) and swallows notifications. Lets a
    /// test drive the MCP client (`initialize`, `tools/list`, `tools/call`)
    /// without spawning a subprocess or opening a socket.
    #[derive(Default)]
    pub struct ScriptedTransport {
        results: HashMap<String, Value>,
    }

    impl ScriptedTransport {
        pub fn new() -> Self {
            Self::default()
        }
        /// Map a request `method` to the `result` value it should return.
        pub fn on(mut self, method: impl Into<String>, result: Value) -> Self {
            self.results.insert(method.into(), result);
            self
        }
    }

    #[async_trait]
    impl McpTransport for ScriptedTransport {
        async fn request(&self, method: &str, _params: Value) -> Result<Value> {
            Ok(self.results.get(method).cloned().unwrap_or(Value::Null))
        }
        async fn notify(&self, _method: &str, _params: Value) -> Result<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scripted_provider_replays_then_repeats() {
        let p = ScriptedProvider::tools_then_final(
            vec![ToolCall {
                id: "t0".into(),
                name: "echo".into(),
                arguments: serde_json::json!({"val": "a"}),
            }],
            "done",
        );
        let req = CompletionRequest {
            messages: vec![],
            tools: vec![],
            max_tokens: 10,
            temperature: 0.0,
        };
        // turn 0: tool call
        let r0 = p.complete(req.clone()).await.unwrap();
        assert_eq!(r0.message.tool_calls.len(), 1);
        // turn 1: final answer
        let r1 = p.complete(req.clone()).await.unwrap();
        assert_eq!(r1.message.content, "done");
        // turn 2+: repeats the final answer, never panics
        let r2 = p.complete(req).await.unwrap();
        assert_eq!(r2.message.content, "done");
    }

    #[tokio::test]
    async fn scripted_transport_drives_the_mcp_client() {
        use agent_mcp::McpClient;
        // A canned server: initialize + one tool in tools/list.
        let transport = mcp::ScriptedTransport::new()
            .on(
                "initialize",
                serde_json::json!({"protocolVersion": "2025-06-18"}),
            )
            .on(
                "tools/list",
                serde_json::json!({"tools": [{"name": "ping", "description": "p"}]}),
            );
        let client = McpClient::with_transport(Box::new(transport));
        client.initialize().await.unwrap();
        let defs = client.list_tools().await.unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "ping");
    }

    #[tokio::test]
    async fn fixture_repo_reads_and_mutates_worktrees() {
        let repo = FixtureRepo::new()
            .with_branch("main", "a".repeat(40))
            .with_blob("main", "a.txt", "hello");
        // canned read
        let blob = repo
            .read_file(&Revision::from("main"), std::path::Path::new("a.txt"))
            .await
            .unwrap();
        assert_eq!(blob.text, "hello");
        assert!(repo
            .read_file(&Revision::from("main"), std::path::Path::new("nope"))
            .await
            .is_err());
        // worktree list mutates
        assert!(repo.worktree_list().await.unwrap().is_empty());
        repo.worktree_add(&WorktreeSpec {
            revision: Revision::from("main"),
            writable: true,
            id: Some("w0".into()),
        })
        .await
        .unwrap();
        assert_eq!(repo.worktree_list().await.unwrap().len(), 1);
        assert_eq!(repo.status().await.unwrap().live_worktrees, 1);
        repo.worktree_remove("w0").await.unwrap();
        assert!(repo.worktree_list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn recording_memory_tracks_tool_order() {
        let mem = RecordingMemory::new();
        for id in ["t0", "t1"] {
            mem.append(MemoryEvent {
                kind: "tool".into(),
                message: Message::tool(id, "ok"),
                ts_ms: 0,
                session_id: String::new(),
                usage: None,
                iter: None,
            })
            .await
            .unwrap();
        }
        assert_eq!(mem.tool_order(), vec!["t0", "t1"]);
    }
}

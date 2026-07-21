//! `agent-core` — the seams.
//!
//! This crate defines the traits and shared types that the agent loop depends
//! on, and nothing else. Every replaceable component (LLM provider, tools,
//! memory, context assembly, policy) is an `async` trait here; concrete
//! implementations live in sibling crates and are wired together at runtime.
//!
//! See `DESIGN.md` §4 for the design rationale behind each seam.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("provider error: {0}")]
    Provider(String),
    #[error("tool error: {0}")]
    Tool(String),
    #[error("memory error: {0}")]
    Memory(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("search error: {0}")]
    Search(String),
    #[error("repo error: {0}")]
    Repo(String),
    #[error("tokenizer error: {0}")]
    Tokenizer(String),
}

// ---------------------------------------------------------------------------
// Messages — the common currency between every seam.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// A single tool invocation requested by the model. Provider impls are
/// responsible for parsing their own on-the-wire format (native JSON,
/// XML-tagged, …) into this normalized shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    /// A tool-result message, linked back to the call that produced it.
    pub fn tool(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// Seam 1: LlmProvider
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ModelCapabilities {
    pub supports_tools: bool,
    pub context_window: u32,
}

#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
    pub max_tokens: u32,
    pub temperature: f32,
}

/// A per-model USD cost breakdown for a [`Usage`], in dollars. The four lines are
/// billed at distinct rates (see [`ModelPrices`]); `total` is their sum.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub total: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
    /// Input tokens served from the provider's prompt cache (billed at the cheap
    /// cache-read rate). Additive/serde-defaulted so the JSONL episodic log and the
    /// gRPC wire stay backward-compatible.
    #[serde(default)]
    pub cache_read_tokens: u32,
    /// Input tokens written into the prompt cache (billed at the cache-write
    /// premium over the input rate).
    #[serde(default)]
    pub cache_write_tokens: u32,
    /// USD cost breakdown, filled in once a price table is applied to the token
    /// counts (`None` until then — providers report tokens, not money).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
}

#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub message: Message,
    pub finish_reason: String,
    pub usage: Option<Usage>,
}

/// One increment of a streamed completion. A provider emits any number of these:
/// text arrives via `delta_text`, each fully-assembled tool call via `tool_call`,
/// and the terminal chunk carries `finish_reason` (+ `usage` when reported).
#[derive(Debug, Clone, Default)]
pub struct CompletionChunk {
    pub delta_text: String,
    pub tool_call: Option<ToolCall>,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
}

/// A boxed stream of completion chunks (the streaming counterpart of
/// `CompletionResponse`).
pub type ChunkStream =
    std::pin::Pin<Box<dyn futures_core::Stream<Item = Result<CompletionChunk>> + Send>>;

/// Wraps a model behind a uniform request/response. Mirrors Hermes'
/// `ProviderTransport` split: each impl owns its own message + tool-call
/// conversion.
///
/// A provider must implement `complete` (buffered); `stream` is optional and
/// defaults to adapting `complete` into a single terminal chunk, so existing and
/// third-party providers keep working unchanged. Providers that support
/// server-sent events override `stream` for incremental output.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn capabilities(&self) -> ModelCapabilities;
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse>;

    /// Streaming completion. Default: run `complete`, then emit text, each tool
    /// call, and a terminal chunk with the finish reason + usage.
    async fn stream(&self, req: CompletionRequest) -> Result<ChunkStream> {
        let resp = self.complete(req).await?;
        let mut chunks: Vec<Result<CompletionChunk>> = Vec::new();
        if !resp.message.content.is_empty() {
            chunks.push(Ok(CompletionChunk {
                delta_text: resp.message.content.clone(),
                ..Default::default()
            }));
        }
        for tc in resp.message.tool_calls {
            chunks.push(Ok(CompletionChunk {
                tool_call: Some(tc),
                ..Default::default()
            }));
        }
        chunks.push(Ok(CompletionChunk {
            finish_reason: Some(resp.finish_reason),
            usage: resp.usage,
            ..Default::default()
        }));
        Ok(Box::pin(futures_util::stream::iter(chunks)))
    }
}

// ---------------------------------------------------------------------------
// Seam: Tokenizer + cost model
// ---------------------------------------------------------------------------
//
// Accurate, per-model token counting replaces the crate-private `~chars/4`
// heuristic in `agent-context`, and — once tokens are counted per model — a price
// table turns them into USD. Concrete tokenizer backends (approx / tiktoken / HF /
// provider-endpoint) live in `agent-tokenizer` behind cargo features; the cost
// math is a pure function here so every crate shares one definition. See
// `docs/components/tokenizer.md` and parity spec 23.

/// Per-message structural overhead (role tag + delimiters) folded into
/// [`Tokenizer::count_messages`], in tokens. A model-agnostic approximation of the
/// framing every chat API adds around each message.
pub const MESSAGE_TOKEN_OVERHEAD: u32 = 3;

/// Accurate, per-model token counting — the seam the compaction loop and the cost
/// model call instead of a byte heuristic. `count` is the primitive; the default
/// `count_messages` folds per-message + per-tool-call overhead on top of it, so a
/// backend only has to implement `count` (though it may override `count_messages`
/// to use a provider's own message-counting endpoint).
#[async_trait]
pub trait Tokenizer: Send + Sync {
    /// The backend's name (used as a metric/span label), e.g. `"approx"`.
    fn backend(&self) -> &str;

    /// Count the tokens in `text` as the given `model` would tokenize it.
    async fn count(&self, text: &str, model: &str) -> Result<u32>;

    /// Count tokens across a message array, adding per-message +
    /// per-tool-call name/argument overhead. Default: sum `count` over every
    /// content and tool-call field plus [`MESSAGE_TOKEN_OVERHEAD`] per message —
    /// the accurate analogue of what `estimate_tokens` approximated.
    async fn count_messages(&self, messages: &[Message], model: &str) -> Result<u32> {
        let mut total: u32 = 0;
        for m in messages {
            total = total.saturating_add(self.count(&m.content, model).await?);
            for tc in &m.tool_calls {
                total = total.saturating_add(self.count(&tc.name, model).await?);
                total = total.saturating_add(self.count(&tc.arguments.to_string(), model).await?);
            }
            total = total.saturating_add(MESSAGE_TOKEN_OVERHEAD);
        }
        Ok(total)
    }
}

/// Per-model prices in USD per **million** tokens (`$/MTok`), one rate per billed
/// line. Cache-read is the discounted rate; cache-write is the premium over input.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModelPrices {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl ModelPrices {
    /// The unknown-model fallback: everything free, so a missing model never bills.
    pub const ZERO: ModelPrices = ModelPrices {
        input: 0.0,
        output: 0.0,
        cache_read: 0.0,
        cache_write: 0.0,
    };
}

/// Where a cost figure came from (mirrors hermes' `CostStatus`): `Actual` when the
/// model was found in the price table, `Estimated` when it fell back to zero-price.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CostStatus {
    Actual,
    Estimated,
    Unknown,
}

/// A source of per-model prices. `PriceTable` (in `agent-tokenizer`) and
/// `StaticPrices` (the test double in `agent-testkit`) implement it, so the cost
/// math below is agnostic to where the rates come from.
pub trait Prices: Send + Sync {
    fn get(&self, model: &str) -> Option<ModelPrices>;
}

/// Compute the USD [`Cost`] of a [`Usage`] under `prices` for `model`. Each line is
/// `(rate / 1_000_000) * tokens` (pi's `calculateCost` formula): input←prompt,
/// output←completion, plus the cache-read (discounted) and cache-write (premium)
/// lines. An unknown model yields a zero-priced cost + [`CostStatus::Estimated`] —
/// never a panic and never a wrong bill.
pub fn calculate_cost(model: &str, usage: &Usage, prices: &dyn Prices) -> (Cost, CostStatus) {
    let (p, status) = match prices.get(model) {
        Some(p) => (p, CostStatus::Actual),
        None => (ModelPrices::ZERO, CostStatus::Estimated),
    };
    // A price row is config/provider data and could be malformed or hostile
    // (`NaN`, negative, `inf`). Treat any non-finite or negative rate as 0 so a bad
    // row can't poison `total` (NaN propagates through `+`) or reach the Prometheus
    // cost counter, whose `inc_by` panics on NaN/negative.
    let line = |rate: f64, tokens: u32| {
        if rate.is_finite() && rate > 0.0 {
            (rate / 1_000_000.0) * tokens as f64
        } else {
            0.0
        }
    };
    let input = line(p.input, usage.prompt_tokens);
    let output = line(p.output, usage.completion_tokens);
    let cache_read = line(p.cache_read, usage.cache_read_tokens);
    let cache_write = line(p.cache_write, usage.cache_write_tokens);
    let cost = Cost {
        input,
        output,
        cache_read,
        cache_write,
        total: input + output + cache_read + cache_write,
    };
    (cost, status)
}

// ---------------------------------------------------------------------------
// Seam 2: Tools + registry
// ---------------------------------------------------------------------------

/// A tool's advertised interface (what we hand the model).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    /// JSON Schema for the arguments object.
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct Observation {
    pub content: String,
    pub is_error: bool,
}

impl Observation {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

/// Ambient context handed to every tool invocation.
pub struct ToolContext {
    pub cwd: PathBuf,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<Observation>;

    /// Whether this tool is safe to run concurrently with other tool calls in the
    /// same turn. Defaults to `true`; a tool with side effects that must not
    /// interleave (e.g. an interactive REPL) can override to `false` to force the
    /// whole turn's tools to run sequentially.
    fn parallel_safe(&self) -> bool {
        true
    }
}

/// A name→tool map. The tools are the pluggable part; the registry is a plain
/// container the loop reads from.
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }
    pub fn describe_all(&self) -> Vec<ToolSchema> {
        let mut v: Vec<ToolSchema> = self.tools.values().map(|t| t.schema()).collect();
        v.sort_by(|a, b| a.name.cmp(&b.name)); // deterministic ordering for reproducible runs
        v
    }
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Seam 3: Memory (layered — see DESIGN.md §3)
// ---------------------------------------------------------------------------

/// A recalled item, ready to be injected into context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    pub source: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct RecallQuery {
    pub text: String,
    pub limit: usize,
}

/// An append-only episodic event. `kind` distinguishes e.g. "goal",
/// "assistant", "tool", "usage".
///
/// `session_id`, `usage`, and `iter` are additive (serde-defaulted) so the JSONL
/// episodic log stays backward-compatible; they carry the extra context the
/// telemetry sink needs to route rows into ClickHouse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub kind: String,
    pub message: Message,
    pub ts_ms: u64,
    #[serde(default)]
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iter: Option<u32>,
}

/// The loop-facing memory facade. This is the whole store the agent loop talks
/// to. A backend can implement it directly (one type owns every layer), or
/// compose an [`EpisodicStore`] and a [`SemanticStore`] via [`LayeredMemory`] so
/// the two layers are swappable independently — see DESIGN.md §3.
#[async_trait]
pub trait MemoryStore: Send + Sync {
    /// Retrieve relevant items for the given query (recall pipeline).
    async fn recall(&self, query: &RecallQuery) -> Result<Vec<MemoryItem>>;
    /// Append an event to the episodic log.
    async fn append(&self, event: MemoryEvent) -> Result<()>;
    /// Promote durable facts from episodic → semantic. Returns count written.
    async fn distill(&self) -> Result<usize>;
}

/// The append-only "what happened" layer, split out of [`MemoryStore`] so a
/// backend can swap the durable log (JSONL, sqlite, …) independently of how
/// semantic recall works.
#[async_trait]
pub trait EpisodicStore: Send + Sync {
    /// Append an event to the log.
    async fn append(&self, event: MemoryEvent) -> Result<()>;
    /// The most recent events (oldest first), capped at `limit`. Feeds
    /// distillation; a store with no readback can return an empty vec.
    async fn recent(&self, limit: usize) -> Result<Vec<MemoryEvent>>;
}

/// The "what is true" layer: relevance recall plus promotion of durable facts.
/// This is the seam a contributor swaps to move from keyword recall to a
/// vector/embedding store — the episodic log and the loop stay unchanged.
#[async_trait]
pub trait SemanticStore: Send + Sync {
    /// Retrieve relevant items for the query.
    async fn recall(&self, query: &RecallQuery) -> Result<Vec<MemoryItem>>;
    /// Promote durable facts from the given `episodic` events into semantic
    /// storage. Returns the number of facts written (0 if nothing was durable).
    async fn distill(&self, episodic: &[MemoryEvent]) -> Result<usize>;
}

/// Composes an [`EpisodicStore`] and a [`SemanticStore`] into the [`MemoryStore`]
/// facade: `append` → episodic, `recall` → semantic, and `distill` reads a window
/// of recent episodic events and hands them to the semantic layer to promote.
///
/// Both halves are trait objects, so they can be chosen independently at runtime
/// (e.g. a file episodic log paired with a vector semantic store).
pub struct LayeredMemory {
    episodic: Arc<dyn EpisodicStore>,
    semantic: Arc<dyn SemanticStore>,
    distill_window: usize,
}

impl LayeredMemory {
    /// Default distillation window (how many recent episodic events to consider).
    pub const DEFAULT_DISTILL_WINDOW: usize = 200;

    pub fn new(episodic: Arc<dyn EpisodicStore>, semantic: Arc<dyn SemanticStore>) -> Self {
        Self {
            episodic,
            semantic,
            distill_window: Self::DEFAULT_DISTILL_WINDOW,
        }
    }

    /// Override how many recent episodic events distillation considers.
    pub fn with_distill_window(mut self, n: usize) -> Self {
        self.distill_window = n;
        self
    }

    pub fn episodic(&self) -> &Arc<dyn EpisodicStore> {
        &self.episodic
    }
    pub fn semantic(&self) -> &Arc<dyn SemanticStore> {
        &self.semantic
    }
}

#[async_trait]
impl MemoryStore for LayeredMemory {
    async fn recall(&self, query: &RecallQuery) -> Result<Vec<MemoryItem>> {
        self.semantic.recall(query).await
    }
    async fn append(&self, event: MemoryEvent) -> Result<()> {
        self.episodic.append(event).await
    }
    async fn distill(&self) -> Result<usize> {
        let events = self.episodic.recent(self.distill_window).await?;
        self.semantic.distill(&events).await
    }
}

// ---------------------------------------------------------------------------
// Seam 4: Context assembly / compaction
// ---------------------------------------------------------------------------

/// A fixed, user-provided block of context (from a `context.d/` file). Always
/// injected — unlike recalled memory, it is not relevance-gated.
#[derive(Debug, Clone)]
pub struct ContextBlock {
    pub source: String,
    pub content: String,
}

pub struct ContextInput {
    pub system_prompt: String,
    /// User context injected before the conversation (folded into the system prompt).
    pub prepend: Vec<ContextBlock>,
    pub recalled: Vec<MemoryItem>,
    pub goal: String,
    /// User context injected after the goal (a trailing system message).
    pub append: Vec<ContextBlock>,
}

/// The live message window handed to the model each turn.
#[derive(Default)]
pub struct WorkingSet {
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone)]
pub struct TokenBudget {
    pub max_context_tokens: u32,
    pub reserve_output: u32,
}

#[async_trait]
pub trait ContextStrategy: Send + Sync {
    /// Build the initial model-ready message list.
    async fn assemble(&self, input: ContextInput) -> Result<Vec<Message>>;
    /// Compact when over budget. Must be non-destructive w.r.t. episodic memory
    /// (it only trims the working set).
    async fn compact(&self, working: &mut WorkingSet, budget: &TokenBudget) -> Result<()>;
}

// ---------------------------------------------------------------------------
// Supporting seam: Policy (the tool approval gate)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(String),
}

#[async_trait]
pub trait Policy: Send + Sync {
    async fn authorize(&self, call: &ToolCall) -> Decision;
}

// ---------------------------------------------------------------------------
// Seam 6: Search (high-performance code search)
// ---------------------------------------------------------------------------
//
// A replaceable code-search backend. The agent indexes the repo it starts in
// (in the background if the index is stale) and issues many concurrent queries
// during planning. Concrete backends (tantivy, …) live in `agent-search` behind
// cargo features; a single gRPC `SearchService` can front one or several so
// their performance is comparable head-to-head. See `docs/components/search.md`.

/// How the query text is interpreted. Backends advertise which modes they can
/// serve via [`SearchCapabilities`]; an unsupported mode is rejected before
/// dispatch rather than silently degraded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    /// Match the literal tokens (the intersection every backend supports).
    Literal,
    /// Match the terms as an ordered phrase.
    Phrase,
    /// Levenshtein-fuzzy term match (see [`SearchQuery::fuzzy_distance`]).
    Fuzzy,
    /// Regular-expression match.
    Regex,
}

impl SearchMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SearchMode::Literal => "literal",
            SearchMode::Phrase => "phrase",
            SearchMode::Fuzzy => "fuzzy",
            SearchMode::Regex => "regex",
        }
    }
}

/// A search request. `path_globs`/`lang` narrow the corpus; `limit` caps hits.
/// The optional fields are serde-defaulted so the wire/JSON shape stays additive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub text: String,
    pub mode: SearchMode,
    /// Include filters, e.g. `["**/*.rs"]`. Empty ⇒ the whole corpus.
    #[serde(default)]
    pub path_globs: Vec<String>,
    /// Restrict to a language label, e.g. `"rust"`. `None` ⇒ any.
    #[serde(default)]
    pub lang: Option<String>,
    pub limit: usize,
    /// Max edit distance for [`SearchMode::Fuzzy`] (`None` ⇒ backend default).
    #[serde(default)]
    pub fuzzy_distance: Option<u8>,
}

/// One match. `line == 0` denotes a filename-only match (no content position),
/// which some backends return for path matches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub path: PathBuf,
    /// 1-based line of the match; `0` ⇒ filename-only match.
    pub line: u32,
    pub col_start: u32,
    pub col_end: u32,
    /// Relevance score (BM25 for scored backends; rank-derived otherwise).
    pub score: f32,
    pub snippet: String,
}

/// A backend's advertised feature set — the search analogue of
/// [`ModelCapabilities`]. The dispatcher consults it to reject a query whose
/// [`SearchMode`] the backend cannot serve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchCapabilities {
    pub backend: String,
    pub modes: Vec<SearchMode>,
    /// Matches inside file contents (vs. filename-only).
    pub content_search: bool,
    /// Returns meaningful relevance scores.
    pub scored: bool,
    /// Supports incremental reindex (vs. full rebuild only).
    pub incremental: bool,
    /// Advisory cap on concurrent queries (`0` ⇒ unbounded).
    pub max_concurrent_queries: u32,
}

impl SearchCapabilities {
    pub fn supports(&self, mode: SearchMode) -> bool {
        self.modes.contains(&mode)
    }
}

/// Freshness of the on-disk index relative to the working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexState {
    /// Up to date with the working tree.
    Fresh,
    /// An index exists but the tree has changed since it was built.
    Stale,
    /// No index yet.
    Missing,
    /// A (re)index is currently running.
    Building,
}

/// A read-only snapshot of the index state (see [`SearchBackend::status`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStatus {
    pub state: IndexState,
    pub indexed_files: u64,
    pub last_indexed_ms: u64,
    /// Digest of the freshness manifest — cheap over-the-wire equality check.
    pub manifest_digest: String,
}

/// Progress emitted during a (re)index; streamed over gRPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReindexProgress {
    pub files_done: u64,
    pub files_total: u64,
    pub done: bool,
}

/// A callback invoked with incremental [`ReindexProgress`] during a reindex.
/// Boxed as a plain `Fn` so the trait stays object-safe; the gRPC server adapter
/// turns each call into a streamed response, local callers pass a no-op or a
/// metrics-recording closure.
pub type ProgressFn<'a> = &'a (dyn Fn(ReindexProgress) + Send + Sync);

/// A replaceable code-search backend. `status` is a cheap freshness probe safe
/// to call on every start; `reindex` is long-running (the runtime drives it on a
/// background task); `query` must be safe to call concurrently from many tasks,
/// including while a reindex runs (serve-stale semantics).
#[async_trait]
pub trait SearchBackend: Send + Sync {
    fn capabilities(&self) -> SearchCapabilities;

    /// Cheap, read-only staleness probe. Never triggers a rebuild.
    async fn status(&self) -> Result<IndexStatus>;

    /// Bring the index up to date (incremental where supported), reporting
    /// progress. Long-running — callers should run it off the request path.
    async fn reindex(&self, progress: ProgressFn<'_>) -> Result<IndexStatus>;

    /// Run a query. Safe to call concurrently, including during a reindex.
    async fn query(&self, q: &SearchQuery) -> Result<Vec<SearchHit>>;

    /// List indexed file paths matching `globs` (empty ⇒ all), sorted and
    /// de-duplicated — the index-backed alternative to walking the tree with
    /// `ls`/`find`. Reflects the index (fast, but as fresh as the last reindex).
    /// Backends that don't enumerate paths return an error (the default).
    async fn list_files(&self, _globs: &[String]) -> Result<Vec<std::path::PathBuf>> {
        Err(Error::Search(
            "this search backend does not support listing files".into(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Seam 7: RepoBackend (multi-branch git — see docs/components/git.md)
// ---------------------------------------------------------------------------
//
// One shared bare/mirror object database fronts many disposable worktrees. The
// trait has two halves: immutable, revision-addressed *object reads* (safe to
// call concurrently from many planning tasks) and side-effecting *worktree /
// mirror / ref* lifecycle (session-scoped). `push` is the only operation that
// leaves the sandbox — the runtime gates it through the `Policy` seam and the
// `[git] push_policy`. Concrete backends live in `agent-git` behind cargo
// features (`git-hybrid` = gix reads + git-CLI writes, `git-cli` = all shell-out).

/// A resolved git object id (commit/tree/blob), as its hex string. A newtype so
/// cache keys and diffs are type-checked; `Display`/`as_str` yield the hex.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Oid(pub String);

impl Oid {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Oid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// An unresolved revision spec the model may pass: a branch, tag, `HEAD~3`, a raw
/// oid, or a `base...target` range for `diff`. Backends resolve it via `resolve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision(pub String);

impl Revision {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<S: Into<String>> From<S> for Revision {
    fn from(s: S) -> Self {
        Revision(s.into())
    }
}

/// The kind of object a [`TreeEntry`] points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Blob,
    Tree,
    Symlink,
    Submodule,
}

/// One entry in a tree listing (`list_tree`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeEntry {
    /// Repo-relative path.
    pub path: PathBuf,
    /// Blob or tree oid.
    pub oid: Oid,
    pub kind: EntryKind,
    /// Git filemode (e.g. `0o100644`).
    pub mode: u32,
    /// Blob size when cheaply known.
    #[serde(default)]
    pub size: Option<u64>,
}

/// A file's contents at a revision. Carries its blob `oid` so callers can key
/// AST/semantic caches by immutable identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobContent {
    pub oid: Oid,
    pub path: PathBuf,
    pub bytes_len: u64,
    pub is_binary: bool,
    /// Empty when `is_binary`.
    #[serde(default)]
    pub text: String,
}

/// Per-file change class in a `base..target` diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChange,
}

/// One file's diff within a [`DiffResult`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    pub change: ChangeKind,
    #[serde(default)]
    pub old_path: Option<PathBuf>,
    #[serde(default)]
    pub new_path: Option<PathBuf>,
    #[serde(default)]
    pub old_oid: Option<Oid>,
    #[serde(default)]
    pub new_oid: Option<Oid>,
    pub additions: u32,
    pub deletions: u32,
    /// Unified diff text (the tool layer may truncate it).
    #[serde(default)]
    pub patch: String,
}

/// The result of comparing two revisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResult {
    pub base: Oid,
    pub target: Oid,
    pub files: Vec<FileDiff>,
}

/// One commit in a history walk (`log`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    pub oid: Oid,
    #[serde(default)]
    pub parents: Vec<Oid>,
    pub author: String,
    #[serde(default)]
    pub author_email: String,
    pub committed_ms: u64,
    /// First line of the message.
    pub summary: String,
    #[serde(default)]
    pub body: String,
}

/// A grep-at-revision hit (content search against the object DB, not a worktree).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepHit {
    pub path: PathBuf,
    /// 1-based line of the match.
    pub line: u32,
    pub text: String,
}

/// A live disposable worktree checked out from the shared object DB.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeHandle {
    /// Stable id (used as the directory name under the runs dir).
    pub id: String,
    /// Absolute checkout path.
    pub path: PathBuf,
    /// Detached HEAD oid.
    pub head: Oid,
    /// What it was created from.
    pub revision: Revision,
    /// `false` ⇒ a read-only comparison worktree.
    pub writable: bool,
}

/// A request to materialize a worktree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeSpec {
    /// Branch/tag/oid to check out (detached).
    pub revision: Revision,
    pub writable: bool,
    /// Caller-chosen id; the backend generates one when `None`.
    #[serde(default)]
    pub id: Option<String>,
}

/// A private agent ref (checkpoint) under `refs/agent/<session>/<name>` — never
/// pushed upstream unless the push policy allows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub name: String,
    pub oid: Oid,
    /// Full ref path.
    pub ref_name: String,
}

/// A cheap probe of the mirror's state — the git analogue of [`IndexStatus`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoStatus {
    pub mirror_path: PathBuf,
    pub last_fetch_ms: u64,
    pub live_worktrees: u32,
    /// Resolved head oid per known remote branch.
    #[serde(default)]
    pub heads: HashMap<String, Oid>,
}

/// A replaceable git backend. The object-read methods are documented as
/// concurrent-safe (they address immutable objects); the lifecycle methods
/// side-effect on the shared mirror and the runs directory. `status` is the
/// cheap probe (mirrors [`SearchBackend::status`]); `fetch` is long-running (the
/// runtime drives it off the request path, mirroring `reindex`).
#[async_trait]
pub trait RepoBackend: Send + Sync {
    // --- object-level, read-only, revision-addressed (concurrent-safe) ---

    /// Resolve a revision spec to a concrete object id.
    async fn resolve(&self, rev: &Revision) -> Result<Oid>;
    /// Read a file's contents at a revision.
    async fn read_file(&self, rev: &Revision, path: &Path) -> Result<BlobContent>;
    /// List a tree at a revision, optionally recursing.
    async fn list_tree(
        &self,
        rev: &Revision,
        path: &Path,
        recursive: bool,
    ) -> Result<Vec<TreeEntry>>;
    /// Diff `base` against `target`, optionally narrowed by path globs.
    async fn diff(
        &self,
        base: &Revision,
        target: &Revision,
        path_globs: &[String],
    ) -> Result<DiffResult>;
    /// Regex content search at a revision (object DB, not a worktree).
    async fn grep(
        &self,
        rev: &Revision,
        pattern: &str,
        path_globs: &[String],
        limit: usize,
    ) -> Result<Vec<GrepHit>>;
    /// Commit history for a revision, optionally following one path.
    async fn log(
        &self,
        rev: &Revision,
        path: Option<&Path>,
        limit: usize,
    ) -> Result<Vec<CommitInfo>>;
    /// All known branches with their resolved head oids.
    async fn branches(&self) -> Result<Vec<(String, Oid)>>;

    // --- mirror / worktree / ref lifecycle (side-effecting, session-scoped) ---

    /// Cheap, read-only probe of the mirror and live worktrees. Never fetches.
    async fn status(&self) -> Result<RepoStatus>;
    /// Update the shared mirror from upstream. Long-running.
    async fn fetch(&self) -> Result<RepoStatus>;
    /// Materialize a disposable worktree checked out at the spec's revision.
    async fn worktree_add(&self, spec: &WorktreeSpec) -> Result<WorktreeHandle>;
    /// List the live worktrees.
    async fn worktree_list(&self) -> Result<Vec<WorktreeHandle>>;
    /// Remove a worktree by id (best-effort cleanup on session end).
    async fn worktree_remove(&self, id: &str) -> Result<()>;
    /// Commit a worktree's current state to a private agent ref (checkpoint).
    async fn checkpoint(&self, worktree_id: &str, name: &str) -> Result<Checkpoint>;
    /// Push a checkpoint to a remote ref. Policy-gated — the only sandbox escape.
    async fn push(&self, checkpoint: &Checkpoint, remote_ref: &str) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use rstest::rstest;

    /// A provider whose `complete` returns a fixed response, exercising the
    /// default `stream` adapter.
    struct Fixed(CompletionResponse);
    #[async_trait]
    impl LlmProvider for Fixed {
        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities {
                supports_tools: true,
                context_window: 1000,
            }
        }
        async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
            Ok(self.0.clone())
        }
    }

    fn response(content: &str, n_tools: usize, finish: &str, usage: bool) -> CompletionResponse {
        let tool_calls = (0..n_tools)
            .map(|i| ToolCall {
                id: i.to_string(),
                name: "t".into(),
                arguments: serde_json::json!({}),
            })
            .collect();
        CompletionResponse {
            message: Message {
                role: Role::Assistant,
                content: content.into(),
                tool_calls,
                tool_call_id: None,
            },
            finish_reason: finish.into(),
            usage: usage.then(Usage::default),
        }
    }

    /// The default `stream` reconstructs `complete`: text chunk (if any) + one
    /// chunk per tool call + a terminal chunk carrying finish reason and usage.
    #[rstest]
    #[case::text_only("hello", 0, "stop", false)]
    #[case::tool_only("", 1, "tool_use", true)]
    #[case::text_and_multi_tool("hi", 2, "tool_use", true)]
    #[case::boundary_empty("", 0, "stop", false)]
    #[tokio::test]
    async fn default_stream_reconstructs_complete(
        #[case] content: &str,
        #[case] n_tools: usize,
        #[case] finish: &str,
        #[case] has_usage: bool,
    ) {
        let provider = Fixed(response(content, n_tools, finish, has_usage));
        let req = CompletionRequest {
            messages: vec![],
            tools: vec![],
            max_tokens: 10,
            temperature: 0.0,
        };
        let mut s = provider.stream(req).await.unwrap();
        let (mut text, mut calls, mut got_finish, mut got_usage) =
            (String::new(), 0usize, None, None);
        while let Some(chunk) = s.next().await {
            let chunk = chunk.unwrap();
            text.push_str(&chunk.delta_text);
            if chunk.tool_call.is_some() {
                calls += 1;
            }
            if let Some(f) = chunk.finish_reason {
                got_finish = Some(f);
            }
            if chunk.usage.is_some() {
                got_usage = chunk.usage;
            }
        }
        assert_eq!(text, content);
        assert_eq!(calls, n_tools);
        assert_eq!(got_finish.as_deref(), Some(finish));
        assert_eq!(got_usage.is_some(), has_usage);
    }
}

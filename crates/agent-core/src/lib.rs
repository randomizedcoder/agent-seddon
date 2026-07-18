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
use std::path::PathBuf;
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
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

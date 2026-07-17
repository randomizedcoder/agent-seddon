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

/// Wraps a model behind a uniform request/response. Mirrors Hermes'
/// `ProviderTransport` split: each impl owns its own message + tool-call
/// conversion. (Streaming is a documented future addition; v1 is buffered.)
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn capabilities(&self) -> ModelCapabilities;
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse>;
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

#[async_trait]
pub trait MemoryStore: Send + Sync {
    /// Retrieve relevant items for the given query (recall pipeline).
    async fn recall(&self, query: &RecallQuery) -> Result<Vec<MemoryItem>>;
    /// Append an event to the episodic log.
    async fn append(&self, event: MemoryEvent) -> Result<()>;
    /// Promote durable facts from episodic → semantic. Returns count written.
    async fn distill(&self) -> Result<usize>;
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

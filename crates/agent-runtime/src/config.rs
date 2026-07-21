//! Config schema (TOML). This is the experimentation lever: the string fields
//! under `[agent]` and `[memory]` select which seam implementation is used, and
//! the factory (`builder.rs`) turns those strings into wired trait objects.

use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub agent: AgentCfg,
    pub provider: ProviderCfg,
    #[serde(default)]
    pub memory: MemoryCfg,
    #[serde(default)]
    pub tools: ToolsCfg,
    #[serde(default)]
    pub mcp: McpCfg,
    #[serde(default)]
    pub telemetry: TelemetryCfg,
    #[serde(default)]
    pub context_files: ContextFilesCfg,
    #[serde(default)]
    pub metrics: MetricsCfg,
    #[serde(default)]
    pub grpc: GrpcCfg,
    #[serde(default)]
    pub search: SearchCfg,
    #[serde(default)]
    pub tokenizer: TokenizerCfg,
    #[serde(default)]
    pub git: GitCfg,
    #[serde(default)]
    pub policy: PolicyCfg,
    #[serde(default)]
    pub web: WebCfg,
    #[serde(default)]
    pub tasks: TasksCfg,
    #[serde(default)]
    pub structured: StructuredCfg,
    #[serde(default)]
    pub lsp: LspCfg,
    #[serde(default)]
    pub sandbox: SandboxCfg,
    #[serde(default)]
    pub embedder: EmbedderCfg,
    #[serde(default)]
    pub session: SessionCfg,
    #[serde(default)]
    pub reference: ReferenceCfg,
    #[serde(default)]
    pub scanner: ScannerCfg,
    #[serde(default)]
    pub cache: CacheCfg,
    #[serde(default)]
    pub web_search: WebSearchCfg,
}

/// Live web search (the `WebSearch` seam, parity spec 12). `backends` lists the
/// providers in preference order — the first is the default and the rest are
/// selectable per query. Empty ⇒ the `web_search` tool is not registered.
/// Results are cached for `cache_ttl_secs`, keyed by (backend, normalized query,
/// options). See docs/components/web-search.md.
#[derive(Debug, Deserialize)]
pub struct WebSearchCfg {
    #[serde(default)]
    pub backends: Vec<String>,
    #[serde(default = "default_ws_ttl")]
    pub cache_ttl_secs: u64,
    #[serde(default = "default_ws_cache_entries")]
    pub cache_max_entries: usize,
    #[serde(default = "default_ws_limit")]
    pub default_limit: u32,
    #[serde(default = "default_ws_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_ws_retries")]
    pub max_retries: u32,
    /// Brave endpoint override (defaults to the public API).
    #[serde(default)]
    pub brave_endpoint: String,
    /// Brave API key; read from `brave_api_key_env` when empty.
    #[serde(default)]
    pub brave_api_key: String,
    #[serde(default)]
    pub brave_api_key_env: String,
    /// SearXNG instance search endpoint, e.g. `http://localhost:8888/search`.
    #[serde(default)]
    pub searxng_endpoint: String,
}

impl Default for WebSearchCfg {
    fn default() -> Self {
        Self {
            backends: Vec::new(),
            cache_ttl_secs: default_ws_ttl(),
            cache_max_entries: default_ws_cache_entries(),
            default_limit: default_ws_limit(),
            timeout_secs: default_ws_timeout(),
            max_retries: default_ws_retries(),
            brave_endpoint: String::new(),
            brave_api_key: String::new(),
            brave_api_key_env: String::new(),
            searxng_endpoint: String::new(),
        }
    }
}

fn default_ws_ttl() -> u64 {
    900
}
fn default_ws_cache_entries() -> usize {
    256
}
fn default_ws_limit() -> u32 {
    5
}
fn default_ws_timeout() -> u64 {
    20
}
fn default_ws_retries() -> u32 {
    2
}

/// Prompt-cache breakpoint placement (the `CacheStrategy` seam, parity spec 24).
/// `strategy` = "stable-prefix" (default) | "tail-window" | "off". Placement is a
/// no-op on providers with no prompt cache. See docs/components/prompt-cache.md.
#[derive(Debug, Deserialize)]
pub struct CacheCfg {
    #[serde(default = "default_cache_strategy")]
    pub strategy: String,
    /// `tail-window` only: how far back from the newest message to anchor.
    #[serde(default = "default_cache_tail_back")]
    pub tail_back: usize,
}

impl Default for CacheCfg {
    fn default() -> Self {
        Self {
            strategy: default_cache_strategy(),
            tail_back: default_cache_tail_back(),
        }
    }
}

fn default_cache_strategy() -> String {
    "stable-prefix".to_string()
}
fn default_cache_tail_back() -> usize {
    2
}

/// Content security scanning (the `Scanner` seam, parity spec 18). Findings feed
/// the `Policy` gate: a finding at or above `deny_at` blocks the call.
/// `rules` selects the sub-scanners; empty ⇒ scanning is off. `allow_rules`
/// waives specific rule ids (e.g. a known fixture secret) without disabling the
/// scanner. See docs/components/scanner.md.
#[derive(Debug, Deserialize)]
pub struct ScannerCfg {
    #[serde(default)]
    pub rules: Vec<String>,
    #[serde(default = "default_scanner_deny_at")]
    pub deny_at: String,
    #[serde(default)]
    pub allow_rules: Vec<String>,
    /// Threat-pattern breadth: "all" | "context" | "strict".
    #[serde(default = "default_scanner_scope")]
    pub scope: String,
}

impl Default for ScannerCfg {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            deny_at: default_scanner_deny_at(),
            allow_rules: Vec::new(),
            scope: default_scanner_scope(),
        }
    }
}

fn default_scanner_deny_at() -> String {
    "high".to_string()
}
fn default_scanner_scope() -> String {
    "context".to_string()
}

/// Content-addressed session history (the `SessionStore` seam, parity spec 19).
/// `backend` = `"file"` (immutable objects under `dir`); `dir` empty ⇒
/// `<working_dir>/.agent-seddon/session`. See docs/components/session.md.
#[derive(Debug, Deserialize)]
pub struct SessionCfg {
    #[serde(default = "default_session_backend")]
    pub backend: String,
    #[serde(default)]
    pub dir: String,
}

impl Default for SessionCfg {
    fn default() -> Self {
        Self {
            backend: default_session_backend(),
            dir: String::new(),
        }
    }
}

fn default_session_backend() -> String {
    "file".to_string()
}

/// `@`-reference expansion (the `ReferenceResolver` seam, parity spec 17).
/// `backend` = `"local"` (workspace-confined `@file`/`@dir`, routed `@symbol`/
/// `@url`). `budget_tokens` caps how much a prompt's mentions may expand (0 ⇒
/// unbounded); `per_block_max_chars` truncates a single oversized block. See
/// docs/components/reference.md.
#[derive(Debug, Deserialize)]
pub struct ReferenceCfg {
    #[serde(default = "default_reference_backend")]
    pub backend: String,
    #[serde(default = "default_reference_budget")]
    pub budget_tokens: usize,
    #[serde(default = "default_reference_block_chars")]
    pub per_block_max_chars: usize,
}

impl Default for ReferenceCfg {
    fn default() -> Self {
        Self {
            backend: default_reference_backend(),
            budget_tokens: default_reference_budget(),
            per_block_max_chars: default_reference_block_chars(),
        }
    }
}

fn default_reference_backend() -> String {
    "local".to_string()
}
fn default_reference_budget() -> usize {
    8_000
}
fn default_reference_block_chars() -> usize {
    8_000
}

/// The embedding model for semantic search (the `Embedder` seam, parity spec 15).
/// `backend` = `"local"` (dependency-free feature-hashing embedder). `dimensions`
/// sizes its vectors. Enable the vector backend via `[search] backends =
/// ["tantivy", "vector"]`. See docs/components/embedder.md.
#[derive(Debug, Deserialize)]
pub struct EmbedderCfg {
    #[serde(default = "default_embedder_backend")]
    pub backend: String,
    #[serde(default = "default_embedder_dims")]
    pub dimensions: usize,
}

impl Default for EmbedderCfg {
    fn default() -> Self {
        Self {
            backend: default_embedder_backend(),
            dimensions: default_embedder_dims(),
        }
    }
}

fn default_embedder_backend() -> String {
    "local".to_string()
}
fn default_embedder_dims() -> usize {
    256
}

/// Execution isolation for `bash` (the `Sandbox` seam, parity spec 14). `backend`
/// = `"local"` (unconfined spawn, today's behaviour) or `"nix"` (run inside the
/// repo's pinned flake closure — reproducible + content-addressed). See
/// docs/components/sandbox.md.
#[derive(Debug, Deserialize)]
pub struct SandboxCfg {
    #[serde(default = "default_sandbox_backend")]
    pub backend: String,
}

impl Default for SandboxCfg {
    fn default() -> Self {
        Self {
            backend: default_sandbox_backend(),
        }
    }
}

fn default_sandbox_backend() -> String {
    "local".to_string()
}

/// Language servers (the `LspBackend` seam, parity spec 13). Empty ⇒ LSP is off
/// (no daemons spawned). Each entry maps a language + file extensions to a server
/// command. See docs/components/lsp.md.
#[derive(Debug, Default, Deserialize)]
pub struct LspCfg {
    #[serde(default)]
    pub servers: Vec<LspServerCfg>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LspServerCfg {
    pub language: String,
    pub command: Vec<String>,
    pub extensions: Vec<String>,
}

/// Structured output (the `OutputSchema` seam, parity spec 16). `validator`
/// selects the impl (`"draft07"`); `max_repairs` bounds the one-shot repair loop.
/// See docs/components/structured-output.md.
#[derive(Debug, Deserialize)]
pub struct StructuredCfg {
    #[serde(default = "default_validator")]
    pub validator: String,
    #[serde(default = "default_max_repairs")]
    pub max_repairs: usize,
}

impl Default for StructuredCfg {
    fn default() -> Self {
        Self {
            validator: default_validator(),
            max_repairs: default_max_repairs(),
        }
    }
}

fn default_validator() -> String {
    "draft07".to_string()
}
fn default_max_repairs() -> usize {
    1
}

/// The `todo_write` tool (the `TaskTracker` seam, parity spec 21). `backend`
/// selects the impl (`"memory"` = an in-process plan for the session; a
/// `SessionStore`-backed backend is a follow-up). See docs/components/tasks.md.
#[derive(Debug, Deserialize)]
pub struct TasksCfg {
    #[serde(default = "default_tasks_backend")]
    pub backend: String,
}

impl Default for TasksCfg {
    fn default() -> Self {
        Self {
            backend: default_tasks_backend(),
        }
    }
}

fn default_tasks_backend() -> String {
    "memory".to_string()
}

/// The `web_fetch` tool (the `WebBackend` seam, parity spec 11). `backend`
/// selects the impl (`"local"` = in-process reqwest, `"grpc"` = a remote fetch
/// worker). The caps bound a single fetch; the SSRF fields feed the `Policy`
/// guard's `web_fetch` screen — `allow_private = false` (default) denies
/// loopback/private/link-local/metadata targets, and `allow_hosts` globs
/// exempt named hosts. See `docs/components/web-fetch.md`.
#[derive(Debug, Deserialize)]
pub struct WebCfg {
    #[serde(default = "default_web_backend")]
    pub backend: String,
    /// Max bytes read from a single response body (hard cap; default 5 MiB).
    #[serde(default = "default_web_max_bytes")]
    pub max_bytes: u64,
    /// Per-fetch timeout, seconds (clamped to `max_timeout_secs`; default 30).
    #[serde(default = "default_web_timeout")]
    pub timeout_secs: u64,
    /// Upper bound the requested timeout is clamped to (default 120).
    #[serde(default = "default_web_max_timeout")]
    pub max_timeout_secs: u64,
    /// Redirect hops followed before giving up (default 5).
    #[serde(default = "default_web_max_redirects")]
    pub max_redirects: u32,
    /// SSRF screen: allow private/loopback/link-local targets (default false).
    #[serde(default)]
    pub allow_private: bool,
    /// Host globs that bypass the SSRF screen entirely (explicit opt-in).
    #[serde(default)]
    pub allow_hosts: Vec<String>,
}

impl Default for WebCfg {
    fn default() -> Self {
        Self {
            backend: default_web_backend(),
            max_bytes: default_web_max_bytes(),
            timeout_secs: default_web_timeout(),
            max_timeout_secs: default_web_max_timeout(),
            max_redirects: default_web_max_redirects(),
            allow_private: false,
            allow_hosts: Vec::new(),
        }
    }
}

fn default_web_backend() -> String {
    "local".to_string()
}
fn default_web_max_bytes() -> u64 {
    5 * 1024 * 1024
}
fn default_web_timeout() -> u64 {
    30
}
fn default_web_max_timeout() -> u64 {
    120
}
fn default_web_max_redirects() -> u32 {
    5
}

/// Policy parameters. `allow` feeds the `allow-list` policy (used when `[agent]
/// policy = "allow-list"`): each rule allows a tool whose name matches `tool` (a
/// minimal `*` glob) and, when `arg` is set, whose serialized arguments contain
/// that substring; an empty `allow` list denies every tool call.
///
/// The `guard` fields are independent of the base policy: a **guard** wraps
/// whatever `[agent] policy` selects and screens each call for dangerous shell
/// commands (`rm -rf /`, `curl … | sh`, `chmod 777`, …) and writes to sensitive
/// paths (`.env`, `.ssh/`, `.git/`, `/etc/`, credentials). `guard = "prompt"`
/// (default) asks the operator to confirm a flagged call — a hard deny when
/// stdin isn't a TTY; `"deny"` blocks outright; `"off"` disables the screen.
#[derive(Debug, Deserialize)]
pub struct PolicyCfg {
    #[serde(default)]
    pub allow: Vec<AllowRule>,
    /// `"prompt"` (default) | `"deny"` | `"off"`.
    #[serde(default = "default_guard")]
    pub guard: String,
    /// Extra sensitive-path globs to deny/flag (in addition to the built-ins).
    #[serde(default)]
    pub deny_paths: Vec<String>,
    /// Path globs to exempt from the sensitive-path guard (escape hatch).
    #[serde(default)]
    pub allow_paths: Vec<String>,
}

impl Default for PolicyCfg {
    fn default() -> Self {
        Self {
            allow: Vec::new(),
            guard: default_guard(),
            deny_paths: Vec::new(),
            allow_paths: Vec::new(),
        }
    }
}

pub(crate) fn default_guard() -> String {
    "prompt".into()
}

/// One `allow-list` rule: `{ tool = "git_*", arg = "..." }` (arg optional).
#[derive(Debug, Clone, Deserialize)]
pub struct AllowRule {
    pub tool: String,
    #[serde(default)]
    pub arg: Option<String>,
}

/// Multi-branch git (the `RepoBackend` seam). One shared bare/mirror object DB
/// under `mirror_dir` fronts disposable worktrees under `worktrees_dir`; a run
/// directory is `<worktrees_dir>/<session_id>/`. `backend` selects the impl
/// ("cli" = all shell-out, "hybrid" = in-process gix reads + git-CLI writes,
/// "grpc" = a remote seam). `push_policy` gates the only operation that leaves
/// the sandbox. See `docs/components/git.md`.
#[derive(Debug, Deserialize)]
pub struct GitCfg {
    /// "cli" (default) | "hybrid" | "grpc". Empty ⇒ "cli".
    #[serde(default)]
    pub backend: String,
    /// Shared bare/mirror object DB. Empty ⇒ `<repo>/.agent-seddon/mirror`.
    #[serde(default)]
    pub mirror_dir: String,
    /// Parent dir for disposable worktrees. Empty ⇒ `<repo>/.agent-seddon/worktrees`.
    #[serde(default)]
    pub worktrees_dir: String,
    /// Upstream remote URL for the mirror. Empty ⇒ infer from the checkout's origin.
    #[serde(default)]
    pub remote: String,
    /// On start, fetch the mirror in the background if it is older than this many
    /// seconds. `0` ⇒ never auto-fetch.
    #[serde(default)]
    pub auto_fetch_secs: u64,
    /// Max concurrent live worktrees (`0` ⇒ unbounded).
    #[serde(default = "default_max_worktrees")]
    pub max_worktrees: u32,
    /// "never" (default) | "checkpoint-only" | "explicit".
    #[serde(default = "default_push_policy")]
    pub push_policy: String,
}

impl Default for GitCfg {
    fn default() -> Self {
        Self {
            backend: String::new(),
            mirror_dir: String::new(),
            worktrees_dir: String::new(),
            remote: String::new(),
            auto_fetch_secs: 0,
            max_worktrees: default_max_worktrees(),
            push_policy: default_push_policy(),
        }
    }
}

impl GitCfg {
    /// The configured backend name, or the default when unset.
    pub fn backend_name(&self) -> &str {
        if self.backend.is_empty() {
            "cli"
        } else {
            &self.backend
        }
    }
}

/// High-performance code search (the `SearchBackend` seam). `backends` selects
/// which backends to wire behind the single search interface (the first is the
/// default); empty ⇒ just `["tantivy"]`. The on-disk index lives under
/// `<repo>/.agent-seddon/index/<backend>` unless `index_dir` overrides it. See
/// `docs/components/search.md`.
#[derive(Debug, Deserialize)]
pub struct SearchCfg {
    #[serde(default)]
    pub backends: Vec<String>,
    /// Override the index directory (empty ⇒ per-repo default).
    #[serde(default)]
    pub index_dir: String,
    /// On start, check index freshness and reindex in the background if stale.
    #[serde(default = "default_true")]
    pub auto_index: bool,
}

impl Default for SearchCfg {
    fn default() -> Self {
        Self {
            backends: Vec::new(),
            index_dir: String::new(),
            auto_index: true,
        }
    }
}

/// `[tokenizer]` — selects the [`agent_core::Tokenizer`] backend that counts tokens
/// for compaction budgeting and cost accounting. `approx` (the default) is the
/// dependency-free segmenter; `grpc` dials a remote tokenizer seam (follow-up).
/// See parity spec 23 and `docs/components/tokenizer.md`.
#[derive(Debug, Deserialize)]
pub struct TokenizerCfg {
    #[serde(default = "default_tokenizer")]
    pub backend: String,
}

impl Default for TokenizerCfg {
    fn default() -> Self {
        Self {
            backend: default_tokenizer(),
        }
    }
}

fn default_tokenizer() -> String {
    "approx".to_string()
}

impl SearchCfg {
    /// The configured backend names, or the single default when unset.
    pub fn backend_names(&self) -> Vec<String> {
        if self.backends.is_empty() {
            vec!["tantivy".to_string()]
        } else {
            self.backends.clone()
        }
    }
}

/// gRPC transport wiring. Per seam: the `endpoint` a `= "grpc"` client dials, and
/// the `listen` address an `agent --serve-<seam>` process binds. Both default
/// (when empty) to `127.0.0.1:<port>` using the port from the generated
/// `agent_grpc::constants`; set `endpoint`/`listen` to `unix:/path` to use a
/// unix-domain socket (TCP-bypassing, same-host). See `docs/grpc.md`.
#[derive(Debug, Default, Deserialize)]
pub struct GrpcCfg {
    #[serde(default)]
    pub provider: GrpcSeamCfg,
    #[serde(default)]
    pub memory: GrpcSeamCfg,
    /// A remote tool worker. Unlike the other seams, tools are only fetched when
    /// `endpoint` is non-empty (there is no implicit default worker).
    #[serde(default)]
    pub tools: GrpcSeamCfg,
    #[serde(default)]
    pub context: GrpcSeamCfg,
    #[serde(default)]
    pub policy: GrpcSeamCfg,
    #[serde(default)]
    pub search: GrpcSeamCfg,
    #[serde(default)]
    pub repo: GrpcSeamCfg,
}

#[derive(Debug, Default, Deserialize)]
pub struct GrpcSeamCfg {
    /// Endpoint a `= "grpc"` client dials. Empty ⇒ `127.0.0.1:<default port>`.
    #[serde(default)]
    pub endpoint: String,
    /// Address a `--serve-<seam>` listener binds. Empty ⇒ `127.0.0.1:<default port>`.
    #[serde(default)]
    pub listen: String,
}

/// External MCP (Model Context Protocol) servers whose tools are discovered at
/// startup and registered as `mcp_<server>_<tool>`. A server is stdio if it has
/// a `command`, or HTTP if it has a `url`.
#[derive(Debug, Default, Deserialize)]
pub struct McpCfg {
    #[serde(default)]
    pub servers: Vec<McpServerCfg>,
}

#[derive(Debug, Deserialize)]
pub struct McpServerCfg {
    pub name: String,
    /// Transport kind. Empty ⇒ inferred (`command` → stdio, `url` → http). Set to a
    /// custom kind registered via `Registry::transport` to use an out-of-tree
    /// transport; the whole server config is handed to its factory as `params`.
    #[serde(default)]
    pub kind: String,
    // --- stdio ---
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    // --- http ---
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

/// User context injected from `<dir>/prepend/` and `<dir>/append/` (NNNN_*.md).
#[derive(Debug, Deserialize)]
pub struct ContextFilesCfg {
    #[serde(default = "default_context_dir")]
    pub dir: String,
}

impl Default for ContextFilesCfg {
    fn default() -> Self {
        Self {
            dir: default_context_dir(),
        }
    }
}

/// Prometheus metrics. Off by default.
#[derive(Debug, Deserialize)]
pub struct MetricsCfg {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_metrics_listen")]
    pub listen: String,
    /// If set, push metrics to this Pushgateway base URL on exit.
    #[serde(default)]
    pub pushgateway: String,
    #[serde(default = "default_metrics_job")]
    pub job: String,
}

impl Default for MetricsCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: default_metrics_listen(),
            pushgateway: String::new(),
            job: default_metrics_job(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AgentCfg {
    /// Which `LlmProvider` impl, e.g. "openai-compat".
    pub provider: String,
    /// Working directory the file/shell tools operate under (`bash`, `read_file`,
    /// `edit`, `grep`, …). Empty ⇒ the process's current directory. Set this to
    /// point the agent at a target repo without `cd`-ing the process — needed for
    /// embedded/served use and hermetic tests. `~` is expanded.
    #[serde(default)]
    pub working_dir: String,
    /// Which `ContextStrategy` impl, e.g. "sliding-window".
    #[serde(default = "default_context")]
    pub context: String,
    /// Which `Policy` impl, e.g. "auto-approve" | "interactive".
    #[serde(default = "default_policy")]
    pub policy: String,
    #[serde(default = "default_max_iters")]
    pub max_iterations: usize,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_context_window")]
    pub context_window: u32,
    #[serde(default = "default_reserve_output")]
    pub reserve_output: u32,
    /// For `context = "summarizing-window"`: tokens of recent history to keep
    /// verbatim; older turns are summarized.
    #[serde(default = "default_keep_recent")]
    pub keep_recent_tokens: u32,
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    /// Consume completions as a stream and echo assistant text live to stderr.
    /// (The loop always uses the provider's `stream`; this toggles the echo.)
    #[serde(default = "default_true")]
    pub stream: bool,
    /// Execute a turn's tool calls concurrently (when all are parallel-safe).
    #[serde(default = "default_true")]
    pub parallel_tools: bool,
    /// Per-tool wall-clock timeout in seconds (a backstop against a hung tool
    /// freezing the loop). Generous by default so real builds/tests aren't cut;
    /// `bash` also has its own shorter timeout. `0` disables the loop-level guard.
    #[serde(default = "default_tool_timeout")]
    pub tool_timeout_secs: u64,
    /// Expose a `delegate` tool so the model can spawn child agents with isolated
    /// context. Off by default (nested loops multiply cost).
    #[serde(default)]
    pub subagents: bool,
    /// Maximum delegation depth (levels of nested `delegate`).
    #[serde(default = "default_subagent_depth")]
    pub subagent_max_depth: usize,
}

#[derive(Debug, Deserialize)]
pub struct ProviderCfg {
    /// Base URL of the API. Optional for providers with a well-known default
    /// (e.g. Anthropic → `https://api.anthropic.com/v1`); required for
    /// openai-compat.
    #[serde(default)]
    pub base_url: String,
    pub model: String,
    /// `anthropic-version` header for the Anthropic provider.
    #[serde(default = "default_anthropic_version")]
    pub version: String,
    /// Inline key (avoid committing). Takes precedence if non-empty.
    #[serde(default)]
    pub api_key: String,
    /// Read the key from this env var if `api_key` is empty.
    #[serde(default)]
    pub api_key_env: String,
    /// Read the key from this file path if `api_key`/env are empty.
    /// Used to keep the secret out of the repo (see README).
    #[serde(default)]
    pub api_key_file: String,
    #[serde(default)]
    pub insecure_tls: bool,
    /// Retries for transient request failures (HTTP 429 / 5xx / timeout /
    /// connection error), with exponential backoff. `0` disables retrying.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Whether the configured model accepts image content blocks (parity spec 26).
    /// Defaults **off** and is opted into per deployment: this endpoint is generic,
    /// and sending an image to a text-only model fails the whole request. The
    /// Anthropic provider ignores this (every Claude model takes images).
    #[serde(default)]
    pub supports_vision: bool,
}

#[derive(Debug, Deserialize)]
pub struct MemoryCfg {
    /// Which `MemoryStore` backend, e.g. "file". Selected via the registry.
    #[serde(default = "default_memory_backend")]
    pub backend: String,
    #[serde(default = "default_episodic_path")]
    pub episodic_path: String,
    #[serde(default = "default_semantic_dir")]
    pub semantic_dir: String,
    /// Optional independent `SemanticStore` backend, e.g. "file" or a custom
    /// "vector". When set, the runtime composes the file episodic log with this
    /// semantic layer via `LayeredMemory` instead of using `backend`'s whole
    /// store. Empty ⇒ use `backend`.
    #[serde(default)]
    pub semantic: String,
    /// Whether `distill()` promotes episodic events into semantic facts (a model
    /// call at each run's end). Off by default so the default build makes no extra
    /// model calls.
    #[serde(default)]
    pub distill: bool,
    #[serde(default = "default_recall_limit")]
    pub recall_limit: usize,
}

impl Default for MemoryCfg {
    fn default() -> Self {
        Self {
            backend: default_memory_backend(),
            episodic_path: default_episodic_path(),
            semantic_dir: default_semantic_dir(),
            semantic: String::new(),
            distill: false,
            recall_limit: default_recall_limit(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct ToolsCfg {
    #[serde(default)]
    pub enabled: Vec<String>,
}

/// Streaming telemetry into ClickHouse. Off by default — behavior is unchanged
/// unless `enabled = true`.
#[derive(Debug, Deserialize)]
pub struct TelemetryCfg {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_clickhouse_url")]
    pub clickhouse_url: String,
    #[serde(default = "default_database")]
    pub database: String,
    #[serde(default = "default_ch_user")]
    pub user: String,
    #[serde(default)]
    pub password: String,
    /// Stream `tracing` log events into `agent_logs` (in addition to stdout).
    #[serde(default = "default_true")]
    pub stream_logs: bool,
    #[serde(default = "default_batch_rows")]
    pub batch_max_rows: usize,
    #[serde(default = "default_flush_ms")]
    pub flush_interval_ms: u64,
    /// OTLP/gRPC endpoint for exporting distributed traces to the ClickStack OTEL
    /// collector (e.g. `http://localhost:4317`). Empty (the default) = no exporter.
    /// Independent of `enabled`: OTLP tracing can run with the native ClickHouse
    /// sink off, and vice versa.
    #[serde(default)]
    pub otlp_endpoint: String,
    /// `service.name` resource attribute on OTLP-exported spans.
    #[serde(default = "default_otel_service_name")]
    pub otel_service_name: String,
    /// Extra OTLP request headers, raw `key=value` pairs (comma-separated). Needed
    /// for collectors that authenticate ingestion — e.g. HyperDX/ClickStack expects
    /// `authorization=<ingestion-key>`. Empty (the default) ⇒ no headers.
    #[serde(default)]
    pub otlp_headers: String,
}

impl Default for TelemetryCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            clickhouse_url: default_clickhouse_url(),
            database: default_database(),
            user: default_ch_user(),
            password: String::new(),
            stream_logs: default_true(),
            batch_max_rows: default_batch_rows(),
            flush_interval_ms: default_flush_ms(),
            otlp_endpoint: String::new(),
            otel_service_name: default_otel_service_name(),
            otlp_headers: String::new(),
        }
    }
}

fn default_otel_service_name() -> String {
    "agent-seddon".into()
}

fn default_context() -> String {
    "sliding-window".into()
}
fn default_policy() -> String {
    // Fail safe: if the policy is unspecified, gate every tool call on the
    // operator rather than silently granting unattended code execution. A model
    // steered by prompt injection (e.g. via a malicious file it reads) could
    // otherwise reach `bash`. Unattended runs must opt in with "auto-approve".
    "interactive".into()
}
fn default_max_iters() -> usize {
    12
}
fn default_tool_timeout() -> u64 {
    // 10 minutes: a backstop for a truly hung tool, well above a normal
    // build/test invoked via `bash` (which has its own shorter timeout).
    600
}
fn default_max_tokens() -> u32 {
    8192
}
fn default_temperature() -> f32 {
    0.7
}
fn default_context_window() -> u32 {
    131_072
}
fn default_reserve_output() -> u32 {
    8_192
}
fn default_keep_recent() -> u32 {
    20_000
}
fn default_system_prompt() -> String {
    "You are a coding agent operating in a terminal working directory. \
     Use the provided tools to inspect and modify files and to run commands. \
     Work step by step: call a tool, observe the result, then decide the next \
     step. When the task is complete, reply with a short plain-text summary and \
     do not call any more tools."
        .into()
}
fn default_anthropic_version() -> String {
    "2023-06-01".into()
}
fn default_max_retries() -> u32 {
    3
}
fn default_subagent_depth() -> usize {
    2
}
fn default_memory_backend() -> String {
    "file".into()
}
fn default_episodic_path() -> String {
    ".agent/episodic.jsonl".into()
}
fn default_semantic_dir() -> String {
    ".agent/memory".into()
}
fn default_recall_limit() -> usize {
    5
}
fn default_context_dir() -> String {
    "context.d".into()
}
fn default_metrics_listen() -> String {
    "127.0.0.1:9600".into()
}
fn default_metrics_job() -> String {
    "agent-seddon".into()
}
fn default_clickhouse_url() -> String {
    // Native protocol (TCP), host:port — fastest wire format.
    "localhost:9000".into()
}
fn default_database() -> String {
    "agent".into()
}
fn default_ch_user() -> String {
    "default".into()
}
fn default_true() -> bool {
    true
}
fn default_batch_rows() -> usize {
    256
}
fn default_max_worktrees() -> u32 {
    8
}
fn default_push_policy() -> String {
    "never".into()
}
fn default_flush_ms() -> u64 {
    1_000
}

#[cfg(test)]
impl Config {
    /// A minimal, valid config for unit tests (no network use).
    pub fn minimal_for_test() -> Self {
        Config {
            agent: AgentCfg {
                provider: "openai-compat".into(),
                working_dir: String::new(),
                context: default_context(),
                policy: default_policy(),
                max_iterations: default_max_iters(),
                max_tokens: default_max_tokens(),
                temperature: default_temperature(),
                context_window: default_context_window(),
                reserve_output: default_reserve_output(),
                keep_recent_tokens: default_keep_recent(),
                system_prompt: default_system_prompt(),
                stream: true,
                parallel_tools: true,
                tool_timeout_secs: default_tool_timeout(),
                subagents: false,
                subagent_max_depth: default_subagent_depth(),
            },
            provider: ProviderCfg {
                base_url: "http://localhost:1".into(),
                model: "test-model".into(),
                version: default_anthropic_version(),
                api_key: "test-key".into(),
                api_key_env: String::new(),
                api_key_file: String::new(),
                insecure_tls: false,
                // No retries in tests: fail fast, keep the suite quick (a test that
                // hits the unreachable localhost URL shouldn't sleep through backoff).
                max_retries: 0,
                supports_vision: false,
            },
            memory: MemoryCfg::default(),
            tools: ToolsCfg::default(),
            mcp: McpCfg::default(),
            telemetry: TelemetryCfg::default(),
            context_files: ContextFilesCfg::default(),
            metrics: MetricsCfg::default(),
            grpc: GrpcCfg::default(),
            search: SearchCfg::default(),
            tokenizer: TokenizerCfg::default(),
            git: GitCfg::default(),
            // Guard off in tests: full-agent tests stay hermetic and never block on
            // an interactive prompt. Guard behaviour is unit-tested directly.
            policy: PolicyCfg {
                guard: "off".into(),
                ..PolicyCfg::default()
            },
            web: WebCfg::default(),
            tasks: TasksCfg::default(),
            structured: StructuredCfg::default(),
            lsp: LspCfg::default(),
            sandbox: SandboxCfg::default(),
            embedder: EmbedderCfg::default(),
            session: SessionCfg::default(),
            reference: ReferenceCfg::default(),
            scanner: ScannerCfg::default(),
            cache: CacheCfg::default(),
            web_search: WebSearchCfg::default(),
        }
    }
}

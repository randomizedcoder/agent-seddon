//! Config schema (TOML). This is the experimentation lever: the string fields
//! under `[agent]` and `[memory]` select which seam implementation is used, and
//! the factory (`builder.rs`) turns those strings into wired trait objects.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub agent: AgentCfg,
    pub provider: ProviderCfg,
    #[serde(default)]
    pub memory: MemoryCfg,
    #[serde(default)]
    pub tools: ToolsCfg,
    #[serde(default)]
    pub telemetry: TelemetryCfg,
    #[serde(default)]
    pub context_files: ContextFilesCfg,
    #[serde(default)]
    pub metrics: MetricsCfg,
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
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    /// Consume completions as a stream and echo assistant text live to stderr.
    /// (The loop always uses the provider's `stream`; this toggles the echo.)
    #[serde(default = "default_true")]
    pub stream: bool,
    /// Execute a turn's tool calls concurrently (when all are parallel-safe).
    #[serde(default = "default_true")]
    pub parallel_tools: bool,
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
    #[serde(default = "default_recall_limit")]
    pub recall_limit: usize,
}

impl Default for MemoryCfg {
    fn default() -> Self {
        Self {
            backend: default_memory_backend(),
            episodic_path: default_episodic_path(),
            semantic_dir: default_semantic_dir(),
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
        }
    }
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
                context: default_context(),
                policy: default_policy(),
                max_iterations: default_max_iters(),
                max_tokens: default_max_tokens(),
                temperature: default_temperature(),
                context_window: default_context_window(),
                reserve_output: default_reserve_output(),
                system_prompt: default_system_prompt(),
                stream: true,
                parallel_tools: true,
            },
            provider: ProviderCfg {
                base_url: "http://localhost:1".into(),
                model: "test-model".into(),
                version: default_anthropic_version(),
                api_key: "test-key".into(),
                api_key_env: String::new(),
                api_key_file: String::new(),
                insecure_tls: false,
            },
            memory: MemoryCfg::default(),
            tools: ToolsCfg::default(),
            telemetry: TelemetryCfg::default(),
            context_files: ContextFilesCfg::default(),
            metrics: MetricsCfg::default(),
        }
    }
}

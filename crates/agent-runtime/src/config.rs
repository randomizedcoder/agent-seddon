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
}

#[derive(Debug, Deserialize)]
pub struct ProviderCfg {
    pub base_url: String,
    pub model: String,
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
fn default_episodic_path() -> String {
    ".agent/episodic.jsonl".into()
}
fn default_semantic_dir() -> String {
    ".agent/memory".into()
}
fn default_recall_limit() -> usize {
    5
}

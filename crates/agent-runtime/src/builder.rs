//! The factory / registry: turn config strings into wired trait objects.
//!
//! This is the "registry keyed by config strings" from DESIGN.md §5. With only
//! a handful of impls it's a `match`; as impls grow this can become a real
//! name→constructor map without changing the loop.

use crate::agent::{Agent, Settings};
use crate::config::{Config, ProviderCfg};
use crate::policy::{AutoApprove, Interactive};
use agent_context::SlidingWindow;
use agent_core::{ContextStrategy, LlmProvider, MemoryStore, Policy, ToolRegistry};
use agent_memory::FileMemory;
use agent_providers::{OpenAiCompatConfig, OpenAiCompatProvider};
use anyhow::{anyhow, Context};
use std::sync::Arc;

pub async fn build_agent(cfg: Config) -> anyhow::Result<Agent> {
    let provider = build_provider(&cfg)?;
    let tools = build_tools(&cfg);
    let memory = build_memory(&cfg).await?;
    let context = build_context(&cfg.agent.context)?;
    let policy = build_policy(&cfg.agent.policy)?;

    let settings = Settings {
        max_iterations: cfg.agent.max_iterations,
        max_tokens: cfg.agent.max_tokens,
        temperature: cfg.agent.temperature,
        context_window: cfg.agent.context_window,
        reserve_output: cfg.agent.reserve_output,
        system_prompt: cfg.agent.system_prompt,
        recall_limit: cfg.memory.recall_limit,
        cwd: std::env::current_dir().context("resolving cwd")?,
    };

    Ok(Agent::new(
        provider, tools, memory, context, policy, settings,
    ))
}

fn build_provider(cfg: &Config) -> anyhow::Result<Arc<dyn LlmProvider>> {
    match cfg.agent.provider.as_str() {
        "openai-compat" => {
            if cfg.provider.insecure_tls {
                tracing::warn!(
                    "provider.insecure_tls=true: TLS certificate validation is DISABLED \
                     (needed for the self-signed GLM dev server). This exposes the API key \
                     and traffic to man-in-the-middle attacks — do not use over untrusted networks."
                );
            }
            let api_key = resolve_api_key(&cfg.provider)?;
            let provider = OpenAiCompatProvider::new(OpenAiCompatConfig {
                base_url: cfg.provider.base_url.clone(),
                model: cfg.provider.model.clone(),
                api_key,
                insecure_tls: cfg.provider.insecure_tls,
                context_window: cfg.agent.context_window,
            })
            .map_err(|e| anyhow!("building provider: {e}"))?;
            Ok(Arc::new(provider))
        }
        other => Err(anyhow!("unknown provider `{other}` (known: openai-compat)")),
    }
}

/// Resolve the API key without ever storing it in the repo: inline > env > file.
fn resolve_api_key(p: &ProviderCfg) -> anyhow::Result<String> {
    if !p.api_key.is_empty() {
        return Ok(p.api_key.clone());
    }
    if !p.api_key_env.is_empty() {
        if let Ok(v) = std::env::var(&p.api_key_env) {
            if !v.is_empty() {
                return Ok(v);
            }
        }
    }
    if !p.api_key_file.is_empty() {
        let expanded = expand_tilde(&p.api_key_file);
        let v = std::fs::read_to_string(&expanded)
            .with_context(|| format!("reading api_key_file `{expanded}`"))?;
        return Ok(v.trim().to_string());
    }
    Err(anyhow!(
        "no API key: set provider.api_key, provider.api_key_env, or provider.api_key_file"
    ))
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    path.to_string()
}

fn build_tools(cfg: &Config) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    let enabled = &cfg.tools.enabled;
    for tool in agent_tools::default_tools() {
        if enabled.is_empty() || enabled.iter().any(|n| n == tool.name()) {
            registry.register(tool);
        }
    }
    registry
}

async fn build_memory(cfg: &Config) -> anyhow::Result<Arc<dyn MemoryStore>> {
    let mem = FileMemory::new(&cfg.memory.episodic_path, &cfg.memory.semantic_dir);
    mem.ensure_dirs().await.context("preparing memory dirs")?;
    Ok(Arc::new(mem))
}

fn build_context(name: &str) -> anyhow::Result<Arc<dyn ContextStrategy>> {
    match name {
        "sliding-window" => Ok(Arc::new(SlidingWindow)),
        other => Err(anyhow!(
            "unknown context strategy `{other}` (known: sliding-window)"
        )),
    }
}

fn build_policy(name: &str) -> anyhow::Result<Arc<dyn Policy>> {
    match name {
        "auto-approve" => {
            tracing::warn!(
                "policy=auto-approve: every tool call (including `bash`) runs WITHOUT \
                 confirmation. Only use this on trusted goals/inputs — a prompt-injected \
                 model can reach arbitrary code execution."
            );
            Ok(Arc::new(AutoApprove))
        }
        "interactive" => Ok(Arc::new(Interactive)),
        other => Err(anyhow!(
            "unknown policy `{other}` (known: auto-approve, interactive)"
        )),
    }
}

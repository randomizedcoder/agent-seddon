//! Turn a `Config` into a wired `Agent` via the plugin [`Registry`].
//!
//! The registry (`registry.rs`) owns the config-string → factory maps; this
//! module drives it: resolve each seam by its config string, wrap memory with
//! telemetry when enabled, and assemble `Settings`. It also hosts the built-in
//! factory functions that need more than a one-liner (the OpenAI-compatible
//! provider and the file memory backend).

use crate::agent::{Agent, Settings};
use crate::config::Config;
#[cfg(any(feature = "provider-openai-compat", feature = "provider-anthropic"))]
use crate::config::ProviderCfg;
use crate::context_files;
use crate::metrics::Metrics;
use crate::registry::Registry;
use agent_core::{MemoryStore, ToolRegistry};
use agent_telemetry::{CompositeMemory, TelemetryHandle};
use anyhow::Context;
use std::sync::Arc;

/// Build the agent with the feature-gated built-in modules. When `telemetry` is
/// `Some`, the episodic store is wrapped in a `CompositeMemory` that mirrors
/// events into ClickHouse, and `session_id` is stamped on every recorded event.
pub async fn build_agent(
    cfg: Config,
    telemetry: Option<TelemetryHandle>,
    session_id: String,
    metrics: Metrics,
) -> anyhow::Result<Agent> {
    let registry = Registry::with_builtins();
    build_agent_with(&registry, cfg, telemetry, session_id, metrics).await
}

/// Build the agent from a caller-supplied [`Registry`]. Out-of-tree binaries use
/// this to register their own provider/tool/memory/etc. factories (see
/// `docs/extending.md`) before wiring the loop — no fork required.
pub async fn build_agent_with(
    registry: &Registry,
    cfg: Config,
    telemetry: Option<TelemetryHandle>,
    session_id: String,
    metrics: Metrics,
) -> anyhow::Result<Agent> {
    let provider = registry
        .build_provider(&cfg.agent.provider, &cfg)
        .context("building provider")?;
    let tools = build_tools(registry, &cfg)?;

    let inner_memory = registry
        .build_memory(&cfg.memory.backend, &cfg)
        .context("building memory")?;
    let memory: Arc<dyn MemoryStore> = match telemetry {
        Some(handle) => Arc::new(CompositeMemory::new(inner_memory, handle)),
        None => inner_memory,
    };

    let context = registry
        .build_context(&cfg.agent.context, &cfg)
        .context("building context strategy")?;
    let policy = registry
        .build_policy(&cfg.agent.policy, &cfg)
        .context("building policy")?;

    let (context_prepend, context_append) = context_files::load(&cfg.context_files.dir);
    if !context_prepend.is_empty() || !context_append.is_empty() {
        tracing::info!(
            prepend = context_prepend.len(),
            append = context_append.len(),
            "loaded user context files"
        );
    }

    let settings = Settings {
        max_iterations: cfg.agent.max_iterations,
        max_tokens: cfg.agent.max_tokens,
        temperature: cfg.agent.temperature,
        context_window: cfg.agent.context_window,
        reserve_output: cfg.agent.reserve_output,
        system_prompt: cfg.agent.system_prompt,
        stream: cfg.agent.stream,
        parallel_tools: cfg.agent.parallel_tools,
        recall_limit: cfg.memory.recall_limit,
        cwd: std::env::current_dir().context("resolving cwd")?,
        model: cfg.provider.model.clone(),
        session_id,
        context_prepend,
        context_append,
    };

    Ok(Agent::new(
        provider, tools, memory, context, policy, metrics, settings,
    ))
}

/// Resolve the enabled tools through the registry. Empty `[tools] enabled` means
/// "every registered tool"; otherwise only the named ones (erroring, with the
/// known names listed, on a typo).
fn build_tools(registry: &Registry, cfg: &Config) -> anyhow::Result<ToolRegistry> {
    let mut tools = ToolRegistry::new();
    if cfg.tools.enabled.is_empty() {
        for name in registry.tool_names() {
            tools.register(registry.build_tool(name, cfg)?);
        }
    } else {
        for name in &cfg.tools.enabled {
            let tool = registry
                .build_tool(name, cfg)
                .with_context(|| format!("enabling tool `{name}`"))?;
            tools.register(tool);
        }
    }
    Ok(tools)
}

// --- built-in factory functions (referenced from `register_builtins`) ------

/// Factory for the OpenAI-compatible provider.
#[cfg(feature = "provider-openai-compat")]
pub(crate) fn openai_compat_provider(
    cfg: &Config,
) -> anyhow::Result<Arc<dyn agent_core::LlmProvider>> {
    use agent_providers::{OpenAiCompatConfig, OpenAiCompatProvider};
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
    .map_err(|e| anyhow::anyhow!("building provider: {e}"))?;
    Ok(Arc::new(provider))
}

/// Factory for the Anthropic-native provider.
#[cfg(feature = "provider-anthropic")]
pub(crate) fn anthropic_provider(cfg: &Config) -> anyhow::Result<Arc<dyn agent_core::LlmProvider>> {
    use agent_providers::{AnthropicConfig, AnthropicProvider};
    let api_key = resolve_api_key(&cfg.provider)?;
    let base_url = if cfg.provider.base_url.is_empty() {
        "https://api.anthropic.com/v1".to_string()
    } else {
        cfg.provider.base_url.clone()
    };
    let provider = AnthropicProvider::new(AnthropicConfig {
        base_url,
        model: cfg.provider.model.clone(),
        api_key,
        version: cfg.provider.version.clone(),
        context_window: cfg.agent.context_window,
    })
    .map_err(|e| anyhow::anyhow!("building provider: {e}"))?;
    Ok(Arc::new(provider))
}

/// Factory for the file-backed memory store.
#[cfg(feature = "memory-file")]
pub(crate) fn file_memory(cfg: &Config) -> anyhow::Result<Arc<dyn MemoryStore>> {
    use agent_memory::FileMemory;
    // Best-effort directory prep (the factory is sync; async `ensure_dirs` is not
    // available here). The store also creates the episodic parent on first write.
    if let Some(parent) = std::path::Path::new(&cfg.memory.episodic_path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let _ = std::fs::create_dir_all(&cfg.memory.semantic_dir);
    Ok(Arc::new(FileMemory::new(
        &cfg.memory.episodic_path,
        &cfg.memory.semantic_dir,
    )))
}

/// Resolve the API key without ever storing it in the repo: inline > env > file.
/// Shared by every provider factory that needs a key.
#[cfg(any(feature = "provider-openai-compat", feature = "provider-anthropic"))]
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
    Err(anyhow::anyhow!(
        "no API key: set provider.api_key, provider.api_key_env, or provider.api_key_file"
    ))
}

#[cfg(any(feature = "provider-openai-compat", feature = "provider-anthropic"))]
fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    path.to_string()
}

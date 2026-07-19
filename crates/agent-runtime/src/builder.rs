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
use crate::registry::Registry;
use agent_core::{MemoryStore, ToolRegistry};
use agent_metrics::Metrics;
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
    // Wrap the provider in its metrics decorator up front, so every downstream
    // user (the loop, the summarizing context strategy, distillation) is
    // attributed the same way — including a remote `= "grpc"` client.
    let provider = crate::metered::provider(
        registry
            .build_provider(&cfg.agent.provider, &cfg)
            .context("building provider")?,
        metrics.clone(),
        &cfg.agent.provider,
    );
    #[allow(unused_mut)]
    let mut tools = build_tools(registry, &cfg, &metrics)?;
    #[cfg(feature = "mcp")]
    register_mcp_tools(&mut tools, &cfg, registry, &metrics).await;
    #[cfg(feature = "grpc")]
    register_grpc_tools(&mut tools, &cfg, &metrics).await;

    // Memory: either the whole-store backend, or — when `[memory] semantic` is
    // set — the episodic layer of that backend composed with an independently
    // chosen `SemanticStore` (e.g. a vector store) via `LayeredMemory`.
    let inner_memory: Arc<dyn MemoryStore> = if cfg.memory.semantic.is_empty() {
        registry
            .build_memory(&cfg.memory.backend, &cfg, &provider)
            .context("building memory")?
    } else {
        let episodic = registry
            .build_episodic(&cfg.memory.backend, &cfg)
            .context("building episodic layer")?;
        let semantic = registry
            .build_semantic(&cfg.memory.semantic, &cfg, &provider)
            .context("building semantic layer")?;
        Arc::new(agent_core::LayeredMemory::new(episodic, semantic))
    };
    let composed_memory: Arc<dyn MemoryStore> = match telemetry {
        Some(handle) => Arc::new(CompositeMemory::new(inner_memory, handle)),
        None => inner_memory,
    };
    // Metrics wrapper outermost, so it times the whole memory op the loop sees
    // (including the telemetry mirror, when enabled).
    let memory = crate::metered::memory(composed_memory, metrics.clone());

    let context = crate::metered::context(
        registry
            .build_context(&cfg.agent.context, &cfg, &provider)
            .context("building context strategy")?,
        metrics.clone(),
    );
    let policy = crate::metered::policy(
        registry
            .build_policy(&cfg.agent.policy, &cfg)
            .context("building policy")?,
        metrics.clone(),
        &cfg.agent.policy,
    );

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

    // Subagents: register a `delegate` tool whose children reuse the worker tool
    // set (captured before `delegate` is added, so children can't see it) plus a
    // deeper `delegate` while depth remains.
    #[cfg(feature = "subagents")]
    if cfg.agent.subagents {
        let ctx = Arc::new(crate::subagent::SubagentContext {
            provider: provider.clone(),
            context: context.clone(),
            policy: policy.clone(),
            memory: memory.clone(),
            worker_tools: tools.clone(),
            metrics: metrics.clone(),
            max_depth: cfg.agent.subagent_max_depth.max(1),
            child_settings: settings.clone(),
        });
        tools.register(Arc::new(crate::subagent::DelegateTool::root(ctx)));
    }

    Ok(Agent::new(
        provider, tools, memory, context, policy, metrics, settings,
    ))
}

/// Resolve the enabled tools through the registry. Empty `[tools] enabled` means
/// "every registered tool"; otherwise only the named ones (erroring, with the
/// known names listed, on a typo).
fn build_tools(
    registry: &Registry,
    cfg: &Config,
    metrics: &Metrics,
) -> anyhow::Result<ToolRegistry> {
    let mut tools = ToolRegistry::new();
    if cfg.tools.enabled.is_empty() {
        for name in registry.tool_names() {
            let tool = registry.build_tool(name, cfg)?;
            tools.register(crate::metered::tool(tool, metrics.clone()));
        }
    } else {
        for name in &cfg.tools.enabled {
            let tool = registry
                .build_tool(name, cfg)
                .with_context(|| format!("enabling tool `{name}`"))?;
            tools.register(crate::metered::tool(tool, metrics.clone()));
        }
    }
    Ok(tools)
}

/// Connect to each configured MCP server, discover its tools, and register them
/// (as `mcp_<server>_<tool>`) into the tool registry. Best-effort: a server that
/// fails to start or handshake is logged and skipped, never aborting the run.
/// MCP tools are always added when their server is configured — the `[tools]
/// enabled` allowlist only filters the built-ins.
#[cfg(feature = "mcp")]
async fn register_mcp_tools(
    tools: &mut ToolRegistry,
    cfg: &Config,
    registry: &Registry,
    metrics: &Metrics,
) {
    for s in &cfg.mcp.servers {
        // A custom `kind` selects an out-of-tree transport registered on the
        // registry; the whole server config rides along as `params`. Otherwise the
        // kind is inferred: `command` → stdio, `url` → http.
        let is_custom_kind = !s.kind.is_empty() && s.kind != "stdio" && s.kind != "http";
        let transport = if is_custom_kind {
            agent_mcp::Transport::Other {
                kind: s.kind.clone(),
                params: serde_json::json!({
                    "command": s.command,
                    "args": s.args,
                    "env": s.env,
                    "url": s.url,
                    "headers": s.headers,
                }),
            }
        } else if !s.command.is_empty() {
            agent_mcp::Transport::Stdio {
                command: s.command.clone(),
                args: s.args.clone(),
                env: s.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            }
        } else if !s.url.is_empty() {
            agent_mcp::Transport::Http {
                url: s.url.clone(),
                headers: s
                    .headers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            }
        } else {
            tracing::warn!(
                "mcp server `{}` has neither command nor url; skipping",
                s.name
            );
            continue;
        };
        let server = agent_mcp::ServerConfig {
            name: s.name.clone(),
            transport,
        };
        match agent_mcp::connect_tools(registry.transports(), &server).await {
            Ok(mcp_tools) => {
                let n = mcp_tools.len();
                for tool in mcp_tools {
                    tools.register(crate::metered::tool(tool, metrics.clone()));
                }
                tracing::info!("mcp server `{}`: registered {n} tool(s)", s.name);
            }
            Err(e) => tracing::warn!("mcp server `{}` unavailable: {e}", s.name),
        }
    }
}

/// Discover and register a remote gRPC tool worker's tools (mirrors
/// `register_mcp_tools`). Only acts when `[grpc.tools] endpoint` is set — there is
/// no implicit default worker. Best-effort: a failing worker is logged and skipped.
#[cfg(feature = "grpc")]
async fn register_grpc_tools(tools: &mut ToolRegistry, cfg: &Config, metrics: &Metrics) {
    let endpoint = &cfg.grpc.tools.endpoint;
    if endpoint.is_empty() {
        return;
    }
    let ep = agent_grpc::Endpoint::parse(endpoint);
    match agent_grpc::client::grpc_tools(&ep).await {
        Ok(remote) => {
            let n = remote.len();
            for tool in remote {
                tools.register(crate::metered::tool(tool, metrics.clone()));
            }
            tracing::info!("grpc tool worker `{endpoint}`: registered {n} tool(s)");
        }
        Err(e) => tracing::warn!("grpc tool worker `{endpoint}` unavailable: {e}"),
    }
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

/// Factory for the file-backed whole memory store: the file episodic log paired
/// with the file semantic store, composed via `LayeredMemory`. Distillation is
/// wired only when `[memory] distill = true` (it costs a model call per run).
#[cfg(feature = "memory-file")]
pub(crate) fn file_memory(
    cfg: &Config,
    provider: &Arc<dyn agent_core::LlmProvider>,
) -> anyhow::Result<Arc<dyn MemoryStore>> {
    file_dir_prep(cfg);
    let distill_provider = cfg.memory.distill.then(|| provider.clone());
    Ok(Arc::new(agent_memory::file_memory(
        &cfg.memory.episodic_path,
        &cfg.memory.semantic_dir,
        distill_provider,
    )))
}

/// Factory for just the file episodic layer (used when a custom semantic backend
/// is composed against it via `[memory] semantic`).
#[cfg(feature = "memory-file")]
pub(crate) fn file_episodic(cfg: &Config) -> anyhow::Result<Arc<dyn agent_core::EpisodicStore>> {
    file_dir_prep(cfg);
    Ok(Arc::new(agent_memory::FileEpisodic::new(
        &cfg.memory.episodic_path,
    )))
}

/// Factory for just the file semantic layer.
#[cfg(feature = "memory-file")]
pub(crate) fn file_semantic(
    cfg: &Config,
    provider: &Arc<dyn agent_core::LlmProvider>,
) -> anyhow::Result<Arc<dyn agent_core::SemanticStore>> {
    file_dir_prep(cfg);
    let mut semantic = agent_memory::FileSemantic::new(&cfg.memory.semantic_dir);
    if cfg.memory.distill {
        semantic = semantic.with_provider(provider.clone());
    }
    Ok(Arc::new(semantic))
}

/// Best-effort directory prep shared by the file memory factories. (The factories
/// are sync; the stores also create dirs on first write.)
#[cfg(feature = "memory-file")]
fn file_dir_prep(cfg: &Config) {
    if let Some(parent) = std::path::Path::new(&cfg.memory.episodic_path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let _ = std::fs::create_dir_all(&cfg.memory.semantic_dir);
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

#[cfg(all(
    test,
    any(feature = "provider-openai-compat", feature = "provider-anthropic")
))]
mod tests {
    use super::*;
    use crate::config::ProviderCfg;
    use rstest::rstest;

    fn pcfg(api_key: &str, api_key_env: &str, api_key_file: &str) -> ProviderCfg {
        ProviderCfg {
            base_url: String::new(),
            model: String::new(),
            version: String::new(),
            api_key: api_key.into(),
            api_key_env: api_key_env.into(),
            api_key_file: api_key_file.into(),
            insecure_tls: false,
        }
    }

    // --- expand_tilde: `~/` → $HOME expansion ------------------------------
    #[rstest]
    #[case::negative_absolute("/abs/f")]
    #[case::negative_tilde_no_slash("~root/f")]
    #[case::corner_tilde_mid_path("a~/f")]
    #[case::boundary_empty("")]
    fn expand_tilde_passthrough_cases(#[case] input: &str) {
        assert_eq!(expand_tilde(input), input);
    }

    #[test]
    fn expand_tilde_replaces_home_prefix() {
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(expand_tilde("~/f.txt"), format!("{home}/f.txt"));
        }
    }

    // --- resolve_api_key: precedence inline > env > file -------------------
    #[test]
    fn resolve_api_key_prefers_inline() {
        assert_eq!(
            resolve_api_key(&pcfg("INLINE", "UNSET_ENV_NAME", "")).unwrap(),
            "INLINE"
        );
    }

    #[test]
    fn resolve_api_key_falls_back_to_env() {
        // A unique var name so parallel cases can't race on shared env state.
        let var = "AGENT_SEDDON_TEST_API_KEY_UNIQUE";
        std::env::set_var(var, "FROMENV");
        let got = resolve_api_key(&pcfg("", var, "")).unwrap();
        std::env::remove_var(var);
        assert_eq!(got, "FROMENV");
    }

    #[test]
    fn resolve_api_key_reads_file_trimmed() {
        let dir = agent_testkit::tempdir();
        let f = dir.join("key");
        std::fs::write(&f, "  FILEKEY\n").unwrap();
        assert_eq!(
            resolve_api_key(&pcfg("", "", f.to_str().unwrap())).unwrap(),
            "FILEKEY"
        );
    }

    #[test]
    fn resolve_api_key_errors_when_none_configured() {
        assert!(resolve_api_key(&pcfg("", "", "")).is_err());
    }
}

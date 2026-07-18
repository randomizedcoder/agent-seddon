//! The plugin registry: a config-string → factory map for each seam.
//!
//! This replaces the hand-written `match` statements that used to live in
//! `builder.rs`. Every seam (provider, context, policy, memory, tool) has a
//! `BTreeMap<name, factory>`; the builder looks up the config string and calls
//! the factory. Built-ins are wired in one place — [`register_builtins`] —
//! gated by cargo features. External code can construct a [`Registry`], register
//! its own factories, and call [`crate::build_agent_with`] without forking.
//!
//! See `docs/extending.md` for the contributor workflow.

use crate::config::Config;
use agent_core::{
    ContextStrategy, EpisodicStore, LlmProvider, MemoryStore, Policy, SemanticStore, Tool,
};
use anyhow::anyhow;
use std::collections::BTreeMap;
use std::sync::Arc;

type ProviderFactory = Box<dyn Fn(&Config) -> anyhow::Result<Arc<dyn LlmProvider>> + Send + Sync>;
// Context strategies receive the already-built provider (so a summarizing
// strategy can call the model); most ignore it.
type ContextFactory = Box<
    dyn Fn(&Config, &Arc<dyn LlmProvider>) -> anyhow::Result<Arc<dyn ContextStrategy>>
        + Send
        + Sync,
>;
type PolicyFactory = Box<dyn Fn(&Config) -> anyhow::Result<Arc<dyn Policy>> + Send + Sync>;
// Memory + semantic factories receive the provider too — a store that distills
// (promotes episodic → semantic facts) needs the model. Most ignore it.
type MemoryFactory = Box<
    dyn Fn(&Config, &Arc<dyn LlmProvider>) -> anyhow::Result<Arc<dyn MemoryStore>> + Send + Sync,
>;
type EpisodicFactory = Box<dyn Fn(&Config) -> anyhow::Result<Arc<dyn EpisodicStore>> + Send + Sync>;
type SemanticFactory = Box<
    dyn Fn(&Config, &Arc<dyn LlmProvider>) -> anyhow::Result<Arc<dyn SemanticStore>> + Send + Sync,
>;
type ToolFactory = Box<dyn Fn(&Config) -> anyhow::Result<Arc<dyn Tool>> + Send + Sync>;

/// Name → factory maps for every swappable seam. Keys are `&'static str` and the
/// maps are ordered so error messages list known names deterministically.
#[derive(Default)]
pub struct Registry {
    providers: BTreeMap<&'static str, ProviderFactory>,
    contexts: BTreeMap<&'static str, ContextFactory>,
    policies: BTreeMap<&'static str, PolicyFactory>,
    memories: BTreeMap<&'static str, MemoryFactory>,
    episodics: BTreeMap<&'static str, EpisodicFactory>,
    semantics: BTreeMap<&'static str, SemanticFactory>,
    tools: BTreeMap<&'static str, ToolFactory>,
    // MCP transports live behind their own registry in `agent-mcp`; the runtime
    // owns one so a custom transport is registrable out-of-tree like any seam.
    #[cfg(feature = "mcp")]
    transports: agent_mcp::TransportRegistry,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// A `Registry` pre-populated with the feature-gated built-in modules.
    pub fn with_builtins() -> Self {
        let mut r = Self::new();
        register_builtins(&mut r);
        r
    }

    // --- registration ------------------------------------------------------

    pub fn provider(
        &mut self,
        name: &'static str,
        f: impl Fn(&Config) -> anyhow::Result<Arc<dyn LlmProvider>> + Send + Sync + 'static,
    ) {
        self.providers.insert(name, Box::new(f));
    }
    pub fn context(
        &mut self,
        name: &'static str,
        f: impl Fn(&Config, &Arc<dyn LlmProvider>) -> anyhow::Result<Arc<dyn ContextStrategy>>
            + Send
            + Sync
            + 'static,
    ) {
        self.contexts.insert(name, Box::new(f));
    }
    pub fn policy(
        &mut self,
        name: &'static str,
        f: impl Fn(&Config) -> anyhow::Result<Arc<dyn Policy>> + Send + Sync + 'static,
    ) {
        self.policies.insert(name, Box::new(f));
    }
    pub fn memory(
        &mut self,
        name: &'static str,
        f: impl Fn(&Config, &Arc<dyn LlmProvider>) -> anyhow::Result<Arc<dyn MemoryStore>>
            + Send
            + Sync
            + 'static,
    ) {
        self.memories.insert(name, Box::new(f));
    }
    pub fn episodic(
        &mut self,
        name: &'static str,
        f: impl Fn(&Config) -> anyhow::Result<Arc<dyn EpisodicStore>> + Send + Sync + 'static,
    ) {
        self.episodics.insert(name, Box::new(f));
    }
    pub fn semantic(
        &mut self,
        name: &'static str,
        f: impl Fn(&Config, &Arc<dyn LlmProvider>) -> anyhow::Result<Arc<dyn SemanticStore>>
            + Send
            + Sync
            + 'static,
    ) {
        self.semantics.insert(name, Box::new(f));
    }
    pub fn tool(
        &mut self,
        name: &'static str,
        f: impl Fn(&Config) -> anyhow::Result<Arc<dyn Tool>> + Send + Sync + 'static,
    ) {
        self.tools.insert(name, Box::new(f));
    }

    /// Register an MCP transport factory under a `kind` (e.g. `"websocket"`),
    /// selected by an `[[mcp.servers]] kind = "..."` (or `Transport::Other`).
    #[cfg(feature = "mcp")]
    pub fn transport(
        &mut self,
        kind: impl Into<String>,
        factory: impl agent_mcp::TransportFactory + 'static,
    ) {
        self.transports.register(kind, factory);
    }

    /// The MCP transport registry (used by the builder to connect servers).
    #[cfg(feature = "mcp")]
    pub fn transports(&self) -> &agent_mcp::TransportRegistry {
        &self.transports
    }

    // --- resolution --------------------------------------------------------

    pub fn build_provider(&self, name: &str, cfg: &Config) -> anyhow::Result<Arc<dyn LlmProvider>> {
        let f = self
            .providers
            .get(name)
            .ok_or_else(|| unknown("provider", name, self.providers.keys().copied()))?;
        f(cfg)
    }
    pub fn build_context(
        &self,
        name: &str,
        cfg: &Config,
        provider: &Arc<dyn LlmProvider>,
    ) -> anyhow::Result<Arc<dyn ContextStrategy>> {
        let f = self
            .contexts
            .get(name)
            .ok_or_else(|| unknown("context strategy", name, self.contexts.keys().copied()))?;
        f(cfg, provider)
    }
    pub fn build_policy(&self, name: &str, cfg: &Config) -> anyhow::Result<Arc<dyn Policy>> {
        let f = self
            .policies
            .get(name)
            .ok_or_else(|| unknown("policy", name, self.policies.keys().copied()))?;
        f(cfg)
    }
    pub fn build_memory(
        &self,
        name: &str,
        cfg: &Config,
        provider: &Arc<dyn LlmProvider>,
    ) -> anyhow::Result<Arc<dyn MemoryStore>> {
        let f = self
            .memories
            .get(name)
            .ok_or_else(|| unknown("memory backend", name, self.memories.keys().copied()))?;
        f(cfg, provider)
    }
    pub fn build_episodic(
        &self,
        name: &str,
        cfg: &Config,
    ) -> anyhow::Result<Arc<dyn EpisodicStore>> {
        let f = self
            .episodics
            .get(name)
            .ok_or_else(|| unknown("episodic backend", name, self.episodics.keys().copied()))?;
        f(cfg)
    }
    pub fn build_semantic(
        &self,
        name: &str,
        cfg: &Config,
        provider: &Arc<dyn LlmProvider>,
    ) -> anyhow::Result<Arc<dyn SemanticStore>> {
        let f = self
            .semantics
            .get(name)
            .ok_or_else(|| unknown("semantic backend", name, self.semantics.keys().copied()))?;
        f(cfg, provider)
    }
    pub fn build_tool(&self, name: &str, cfg: &Config) -> anyhow::Result<Arc<dyn Tool>> {
        let f = self
            .tools
            .get(name)
            .ok_or_else(|| unknown("tool", name, self.tools.keys().copied()))?;
        f(cfg)
    }

    /// All registered tool names (used when `[tools] enabled` is empty ⇒ all).
    pub fn tool_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.tools.keys().copied()
    }
}

fn unknown(kind: &str, name: &str, known: impl Iterator<Item = &'static str>) -> anyhow::Error {
    let names: Vec<&str> = known.collect();
    anyhow!(
        "unknown {kind} `{name}` (known: {})",
        if names.is_empty() {
            "<none — check enabled cargo features>".to_string()
        } else {
            names.join(", ")
        }
    )
}

/// Wire every built-in module into the registry. This is the one place a
/// contributor adds a line for a new in-tree module — each guarded by the cargo
/// feature that compiles the module in. See `docs/extending.md`.
pub fn register_builtins(r: &mut Registry) {
    // --- mcp transports (stdio + http) ---
    #[cfg(feature = "mcp")]
    {
        r.transports = agent_mcp::TransportRegistry::with_builtins();
    }

    // --- providers ---
    #[cfg(feature = "provider-openai-compat")]
    r.provider("openai-compat", crate::builder::openai_compat_provider);
    #[cfg(feature = "provider-anthropic")]
    r.provider("anthropic", crate::builder::anthropic_provider);

    // --- context strategies ---
    #[cfg(feature = "context-sliding-window")]
    r.context("sliding-window", |_cfg, _provider| {
        Ok(Arc::new(agent_context::SlidingWindow) as Arc<dyn ContextStrategy>)
    });
    #[cfg(feature = "context-summarizing")]
    r.context("summarizing-window", |cfg, provider| {
        Ok(Arc::new(agent_context::SummarizingWindow::new(
            provider.clone(),
            cfg.agent.keep_recent_tokens,
        )) as Arc<dyn ContextStrategy>)
    });

    // --- policies (always available; they live in agent-runtime) ---
    r.policy("auto-approve", |_cfg| {
        tracing::warn!(
            "policy=auto-approve: every tool call (including `bash`) runs WITHOUT \
             confirmation. Only use this on trusted goals/inputs — a prompt-injected \
             model can reach arbitrary code execution."
        );
        Ok(Arc::new(crate::policy::AutoApprove) as Arc<dyn Policy>)
    });
    r.policy("interactive", |_cfg| {
        Ok(Arc::new(crate::policy::Interactive) as Arc<dyn Policy>)
    });

    // --- memory backends (whole-store + independently-swappable layers) ---
    #[cfg(feature = "memory-file")]
    {
        r.memory("file", crate::builder::file_memory);
        r.episodic("file", crate::builder::file_episodic);
        r.semantic("file", crate::builder::file_semantic);
    }

    // --- tools ---
    #[cfg(feature = "tool-core")]
    {
        r.tool("bash", |_cfg| {
            Ok(Arc::new(agent_tools::BashTool) as Arc<dyn Tool>)
        });
        r.tool("read_file", |_cfg| {
            Ok(Arc::new(agent_tools::ReadFileTool) as Arc<dyn Tool>)
        });
        r.tool("write_file", |_cfg| {
            Ok(Arc::new(agent_tools::WriteFileTool) as Arc<dyn Tool>)
        });
    }
    #[cfg(feature = "tool-edit")]
    r.tool("edit", |_cfg| {
        Ok(Arc::new(agent_tools::EditTool) as Arc<dyn Tool>)
    });
    #[cfg(feature = "tool-search")]
    {
        r.tool("grep", |_cfg| {
            Ok(Arc::new(agent_tools::GrepTool) as Arc<dyn Tool>)
        });
        r.tool("find", |_cfg| {
            Ok(Arc::new(agent_tools::FindTool) as Arc<dyn Tool>)
        });
        r.tool("ls", |_cfg| {
            Ok(Arc::new(agent_tools::LsTool) as Arc<dyn Tool>)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // --- unknown(): error message formatting -------------------------------
    #[rstest]
    #[case::some_known(&["a", "b", "c"], "unknown thing `x` (known: a, b, c)")]
    #[case::single_known(&["only"], "unknown thing `x` (known: only)")]
    #[case::none_known(&[], "unknown thing `x` (known: <none — check enabled cargo features>)")]
    fn unknown_error_cases(#[case] known: &[&'static str], #[case] expected: &str) {
        let err = unknown("thing", "x", known.iter().copied());
        assert_eq!(err.to_string(), expected);
    }

    #[test]
    fn builtins_register_expected_names() {
        let r = Registry::with_builtins();
        // Policies are always present.
        assert!(r.policies.contains_key("auto-approve"));
        assert!(r.policies.contains_key("interactive"));
        #[cfg(feature = "provider-openai-compat")]
        assert!(r.providers.contains_key("openai-compat"));
        #[cfg(feature = "tool-core")]
        {
            let names: Vec<&str> = r.tool_names().collect();
            assert!(names.contains(&"bash"));
            assert!(names.contains(&"read_file"));
            assert!(names.contains(&"write_file"));
        }
    }

    #[test]
    fn unknown_key_lists_known_names() {
        let r = Registry::with_builtins();
        let err = r
            .build_policy("nope", &crate::config::Config::minimal_for_test())
            .map(|_| ())
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown policy `nope`"));
        assert!(err.contains("auto-approve"));
    }
}

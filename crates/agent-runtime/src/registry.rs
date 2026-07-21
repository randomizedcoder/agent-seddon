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
// Context strategies receive the already-built (metered) provider and the
// optional (metered) tokenizer, so a summarizing strategy can call the model and
// either strategy can budget with real token counts.
type ContextFactory = Box<
    dyn Fn(
            &Config,
            &Arc<dyn LlmProvider>,
            Option<&Arc<dyn agent_core::Tokenizer>>,
        ) -> anyhow::Result<Arc<dyn ContextStrategy>>
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
#[cfg(feature = "search")]
type SearchFactory =
    Box<dyn Fn(&Config) -> anyhow::Result<Arc<dyn agent_core::SearchBackend>> + Send + Sync>;
#[cfg(feature = "git")]
type RepoFactory =
    Box<dyn Fn(&Config) -> anyhow::Result<Arc<dyn agent_core::RepoBackend>> + Send + Sync>;
#[cfg(feature = "tokenizer")]
type TokenizerFactory =
    Box<dyn Fn(&Config) -> anyhow::Result<Arc<dyn agent_core::Tokenizer>> + Send + Sync>;

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
    #[cfg(feature = "search")]
    searches: BTreeMap<&'static str, SearchFactory>,
    #[cfg(feature = "git")]
    repos: BTreeMap<&'static str, RepoFactory>,
    #[cfg(feature = "tokenizer")]
    tokenizers: BTreeMap<&'static str, TokenizerFactory>,
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
        f: impl Fn(
                &Config,
                &Arc<dyn LlmProvider>,
                Option<&Arc<dyn agent_core::Tokenizer>>,
            ) -> anyhow::Result<Arc<dyn ContextStrategy>>
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
    #[cfg(feature = "search")]
    pub fn search(
        &mut self,
        name: &'static str,
        f: impl Fn(&Config) -> anyhow::Result<Arc<dyn agent_core::SearchBackend>>
            + Send
            + Sync
            + 'static,
    ) {
        self.searches.insert(name, Box::new(f));
    }
    #[cfg(feature = "git")]
    pub fn repo(
        &mut self,
        name: &'static str,
        f: impl Fn(&Config) -> anyhow::Result<Arc<dyn agent_core::RepoBackend>> + Send + Sync + 'static,
    ) {
        self.repos.insert(name, Box::new(f));
    }

    #[cfg(feature = "tokenizer")]
    pub fn tokenizer(
        &mut self,
        name: &'static str,
        f: impl Fn(&Config) -> anyhow::Result<Arc<dyn agent_core::Tokenizer>> + Send + Sync + 'static,
    ) {
        self.tokenizers.insert(name, Box::new(f));
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
        tokenizer: Option<&Arc<dyn agent_core::Tokenizer>>,
    ) -> anyhow::Result<Arc<dyn ContextStrategy>> {
        let f = self
            .contexts
            .get(name)
            .ok_or_else(|| unknown("context strategy", name, self.contexts.keys().copied()))?;
        f(cfg, provider, tokenizer)
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

    #[cfg(feature = "search")]
    pub fn build_search(
        &self,
        name: &str,
        cfg: &Config,
    ) -> anyhow::Result<Arc<dyn agent_core::SearchBackend>> {
        let f = self
            .searches
            .get(name)
            .ok_or_else(|| unknown("search backend", name, self.searches.keys().copied()))?;
        f(cfg)
    }

    #[cfg(feature = "git")]
    pub fn build_repo(
        &self,
        name: &str,
        cfg: &Config,
    ) -> anyhow::Result<Arc<dyn agent_core::RepoBackend>> {
        let f = self
            .repos
            .get(name)
            .ok_or_else(|| unknown("git backend", name, self.repos.keys().copied()))?;
        f(cfg)
    }

    #[cfg(feature = "tokenizer")]
    pub fn build_tokenizer(
        &self,
        name: &str,
        cfg: &Config,
    ) -> anyhow::Result<Arc<dyn agent_core::Tokenizer>> {
        let f = self
            .tokenizers
            .get(name)
            .ok_or_else(|| unknown("tokenizer", name, self.tokenizers.keys().copied()))?;
        f(cfg)
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

    // --- context strategies (each budgets with the injected tokenizer) ---
    #[cfg(feature = "context-sliding-window")]
    r.context("sliding-window", |cfg, _provider, tokenizer| {
        Ok(Arc::new(agent_context::SlidingWindow::new(
            tokenizer.cloned(),
            cfg.provider.model.clone(),
        )) as Arc<dyn ContextStrategy>)
    });
    #[cfg(feature = "context-summarizing")]
    r.context("summarizing-window", |cfg, provider, tokenizer| {
        Ok(Arc::new(
            agent_context::SummarizingWindow::new(provider.clone(), cfg.agent.keep_recent_tokens)
                .with_tokenizer(tokenizer.cloned(), cfg.provider.model.clone()),
        ) as Arc<dyn ContextStrategy>)
    });

    // --- tokenizer seam (accurate counts + cost, parity spec 23) ---
    #[cfg(feature = "tokenizer")]
    r.tokenizer("approx", |_cfg| {
        Ok(Arc::new(agent_tokenizer::ApproxTokenizer::new()) as Arc<dyn agent_core::Tokenizer>)
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
    r.policy("allow-list", |cfg| {
        // Allow only the tool+arg patterns in `[policy] allow`; deny the rest.
        // An empty list denies everything (fail safe).
        let rules = cfg
            .policy
            .allow
            .iter()
            .map(|r| (r.tool.clone(), r.arg.clone()))
            .collect();
        Ok(Arc::new(crate::policy::AllowList::new(rules)) as Arc<dyn Policy>)
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
        // `bash` is wired by the builder (it needs the config-selected Sandbox
        // backend + metrics), not a plain registry factory. See builder.rs.
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
    #[cfg(feature = "tool-patch")]
    r.tool("apply_patch", |_cfg| {
        Ok(Arc::new(agent_tools::ApplyPatchTool) as Arc<dyn Tool>)
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

    // --- search backends (the SearchBackend seam) ---
    #[cfg(feature = "search")]
    {
        r.search("tantivy", |cfg| {
            let (root, index_dir) = search_paths(cfg, "tantivy")?;
            Ok(
                Arc::new(agent_search::TantivyBackend::open(root, index_dir)?)
                    as Arc<dyn agent_core::SearchBackend>,
            )
        });
        #[cfg(feature = "grpc")]
        r.search("grpc", |cfg| {
            let ep = grpc_client_endpoint(&cfg.grpc.search.endpoint, agent_grpc::constants::SEARCH);
            Ok(Arc::new(agent_grpc::client::GrpcSearch::connect(&ep)?)
                as Arc<dyn agent_core::SearchBackend>)
        });
    }

    // --- git backends (the RepoBackend seam) ---
    // The built-in local backend is wired in `crate::git::build_repo` (it needs
    // the session id, which the config-only factory can't carry). The remote
    // `= "grpc"` client backend registers here.
    #[cfg(all(feature = "git", feature = "grpc"))]
    r.repo("grpc", |cfg| {
        let ep = grpc_client_endpoint(&cfg.grpc.repo.endpoint, agent_grpc::constants::REPO);
        Ok(Arc::new(agent_grpc::client::GrpcRepo::connect(&ep)?)
            as Arc<dyn agent_core::RepoBackend>)
    });

    // --- gRPC seam clients (a remote seam is just another impl, selected by
    //     `= "grpc"`; endpoint from `[grpc]`, defaulting to the generated ports) ---
    #[cfg(feature = "grpc")]
    {
        r.provider("grpc", |cfg| {
            let ep =
                grpc_client_endpoint(&cfg.grpc.provider.endpoint, agent_grpc::constants::PROVIDER);
            // Capabilities are config-derived (no eager round-trip) — the real model
            // lives behind the gateway; this just informs the loop.
            let caps = agent_core::ModelCapabilities {
                supports_tools: true,
                context_window: cfg.agent.context_window,
                supports_response_format: false,
            };
            Ok(
                Arc::new(agent_grpc::client::GrpcProvider::connect(&ep, caps)?)
                    as Arc<dyn LlmProvider>,
            )
        });
        r.memory("grpc", |cfg, _provider| {
            let ep = grpc_client_endpoint(&cfg.grpc.memory.endpoint, agent_grpc::constants::MEMORY);
            Ok(Arc::new(agent_grpc::client::GrpcMemory::connect(&ep)?) as Arc<dyn MemoryStore>)
        });
        r.context("grpc", |cfg, _provider, _tokenizer| {
            let ep =
                grpc_client_endpoint(&cfg.grpc.context.endpoint, agent_grpc::constants::CONTEXT);
            Ok(Arc::new(agent_grpc::client::GrpcContext::connect(&ep)?)
                as Arc<dyn ContextStrategy>)
        });
        r.policy("grpc", |cfg| {
            let ep = grpc_client_endpoint(&cfg.grpc.policy.endpoint, agent_grpc::constants::POLICY);
            Ok(Arc::new(agent_grpc::client::GrpcPolicy::connect(&ep)?) as Arc<dyn Policy>)
        });
    }
}

/// Resolve the repo root + on-disk index directory for a search backend: the
/// repo root is discovered from the cwd, and the index lives under
/// `<root>/.agent-seddon/index/<backend>` unless `[search] index_dir` overrides it.
#[cfg(feature = "search")]
fn search_paths(
    cfg: &Config,
    backend: &str,
) -> anyhow::Result<(std::path::PathBuf, std::path::PathBuf)> {
    let root = agent_search::repo_root(&std::env::current_dir()?);
    let index_dir = if cfg.search.index_dir.is_empty() {
        agent_search::default_index_dir(&root, backend)
    } else {
        std::path::PathBuf::from(&cfg.search.index_dir).join(backend)
    };
    Ok((root, index_dir))
}

/// Resolve a `[grpc]` client endpoint: the configured string, or a loopback TCP
/// default on the seam's generated port. Set the config to `unix:/path` for UDS.
#[cfg(feature = "grpc")]
fn grpc_client_endpoint(
    configured: &str,
    default: agent_grpc::constants::SeamEndpoint,
) -> agent_grpc::Endpoint {
    if configured.is_empty() {
        agent_grpc::Endpoint::parse(&format!("127.0.0.1:{}", default.tcp_port))
    } else {
        agent_grpc::Endpoint::parse(configured)
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
            // `bash` is builder-wired (needs the Sandbox backend), not in the
            // registry; `read_file`/`write_file` remain plain factories.
            let names: Vec<&str> = r.tool_names().collect();
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

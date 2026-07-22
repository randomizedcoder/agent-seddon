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
use agent_metrics::Metrics;
use anyhow::{anyhow, Context};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Everything a seam factory may need, in one place.
///
/// Every factory takes `&FactoryCtx` and nothing else. That is deliberate: the
/// signatures used to differ per seam (`Fn(&Config)`, plus the provider for
/// memory, plus the tokenizer for context), and each new requirement broke all
/// of them. Adding a field here is backward-compatible, so the next seam that
/// needs something new does not force another workspace-wide edit.
///
/// `provider` and `tokenizer` are optional because of build ORDER, not because
/// they are unimportant: the provider is built first (so it cannot see itself),
/// and the tokenizer is built after it but before the context strategy. Use the
/// [`FactoryCtx::provider`] / [`FactoryCtx::tokenizer`] accessors, which report a
/// clear error rather than panicking if a seam asks for something not yet built.
pub struct FactoryCtx<'a> {
    /// The parsed configuration.
    pub cfg: &'a Config,
    /// The shared metrics registry, so a factory can build a metered impl.
    pub metrics: &'a Metrics,
    /// The already-built (metered) provider — absent while building the provider.
    pub built_provider: Option<&'a Arc<dyn LlmProvider>>,
    /// The already-built (metered) tokenizer — absent until it is built.
    pub built_tokenizer: Option<&'a Arc<dyn agent_core::Tokenizer>>,
    /// The already-built embedder, so the vector search backend and
    /// `agent --serve-embed` share ONE instance rather than each constructing
    /// their own (a real embedder loads a model).
    #[cfg(feature = "semantic-search")]
    pub built_embedder: Option<&'a Arc<dyn agent_core::Embedder>>,
    /// The registry itself, so a **composing** factory can build its children by
    /// config name (the `router` provider builds its candidates this way). The
    /// borrow is immutable and re-entrant: `build_*` takes `&self`, so a factory
    /// it invoked may call back into it.
    pub registry: Option<&'a Registry>,
}

impl<'a> FactoryCtx<'a> {
    /// A context with only config + metrics (the common case).
    pub fn new(cfg: &'a Config, metrics: &'a Metrics) -> Self {
        Self {
            cfg,
            metrics,
            built_provider: None,
            built_tokenizer: None,
            #[cfg(feature = "semantic-search")]
            built_embedder: None,
            registry: None,
        }
    }
    /// Let a composing factory build children by name.
    pub fn with_registry(mut self, r: &'a Registry) -> Self {
        self.registry = Some(r);
        self
    }
    pub fn with_provider(mut self, p: &'a Arc<dyn LlmProvider>) -> Self {
        self.built_provider = Some(p);
        self
    }
    pub fn with_tokenizer(mut self, t: Option<&'a Arc<dyn agent_core::Tokenizer>>) -> Self {
        self.built_tokenizer = t;
        self
    }

    /// Inject the already-built embedder (see [`FactoryCtx::built_embedder`]).
    #[cfg(feature = "semantic-search")]
    pub fn with_embedder(mut self, e: Option<&'a Arc<dyn agent_core::Embedder>>) -> Self {
        self.built_embedder = e;
        self
    }
    /// The built provider, or a clear error naming the ordering constraint.
    pub fn provider(&self) -> anyhow::Result<&'a Arc<dyn LlmProvider>> {
        self.built_provider.ok_or_else(|| {
            anyhow!("this seam needs the provider, which is not built yet at this point")
        })
    }
    /// The built tokenizer, if one is configured and already built.
    pub fn tokenizer(&self) -> Option<&'a Arc<dyn agent_core::Tokenizer>> {
        self.built_tokenizer
    }
    /// The registry, for a factory that composes other seams by config name.
    pub fn registry(&self) -> anyhow::Result<&'a Registry> {
        self.registry.ok_or_else(|| {
            anyhow!("this seam composes other seams but was built without a registry handle")
        })
    }
}

type ProviderFactory =
    Box<dyn Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn LlmProvider>> + Send + Sync>;
type ContextFactory =
    Box<dyn Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn ContextStrategy>> + Send + Sync>;
type PolicyFactory = Box<dyn Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn Policy>> + Send + Sync>;
type MemoryFactory =
    Box<dyn Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn MemoryStore>> + Send + Sync>;
type EpisodicFactory =
    Box<dyn Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn EpisodicStore>> + Send + Sync>;
type SemanticFactory =
    Box<dyn Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn SemanticStore>> + Send + Sync>;
type ToolFactory = Box<dyn Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn Tool>> + Send + Sync>;
#[cfg(feature = "search")]
type SearchFactory = Box<
    dyn Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn agent_core::SearchBackend>> + Send + Sync,
>;
#[cfg(feature = "forge")]
type ForgeFactory =
    Box<dyn Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn agent_core::Forge>> + Send + Sync>;
#[cfg(feature = "web-search")]
type WebSearchFactory =
    Box<dyn Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn agent_core::WebSearch>> + Send + Sync>;
#[cfg(feature = "git")]
type RepoFactory =
    Box<dyn Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn agent_core::RepoBackend>> + Send + Sync>;
#[cfg(feature = "tokenizer")]
type TokenizerFactory =
    Box<dyn Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn agent_core::Tokenizer>> + Send + Sync>;

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
    #[cfg(feature = "forge")]
    forges: BTreeMap<&'static str, ForgeFactory>,
    #[cfg(feature = "web-search")]
    web_searches: BTreeMap<&'static str, WebSearchFactory>,
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
        f: impl Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn LlmProvider>> + Send + Sync + 'static,
    ) {
        self.providers.insert(name, Box::new(f));
    }
    pub fn context(
        &mut self,
        name: &'static str,
        f: impl Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn ContextStrategy>> + Send + Sync + 'static,
    ) {
        self.contexts.insert(name, Box::new(f));
    }
    pub fn policy(
        &mut self,
        name: &'static str,
        f: impl Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn Policy>> + Send + Sync + 'static,
    ) {
        self.policies.insert(name, Box::new(f));
    }
    pub fn memory(
        &mut self,
        name: &'static str,
        f: impl Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn MemoryStore>> + Send + Sync + 'static,
    ) {
        self.memories.insert(name, Box::new(f));
    }
    pub fn episodic(
        &mut self,
        name: &'static str,
        f: impl Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn EpisodicStore>> + Send + Sync + 'static,
    ) {
        self.episodics.insert(name, Box::new(f));
    }
    pub fn semantic(
        &mut self,
        name: &'static str,
        f: impl Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn SemanticStore>> + Send + Sync + 'static,
    ) {
        self.semantics.insert(name, Box::new(f));
    }
    pub fn tool(
        &mut self,
        name: &'static str,
        f: impl Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn Tool>> + Send + Sync + 'static,
    ) {
        self.tools.insert(name, Box::new(f));
    }
    #[cfg(feature = "search")]
    pub fn search(
        &mut self,
        name: &'static str,
        f: impl Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn agent_core::SearchBackend>>
            + Send
            + Sync
            + 'static,
    ) {
        self.searches.insert(name, Box::new(f));
    }
    /// Register a forge backend (parity spec 27).
    #[cfg(feature = "forge")]
    pub fn forge(
        &mut self,
        name: &'static str,
        f: impl Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn agent_core::Forge>>
            + Send
            + Sync
            + 'static,
    ) {
        self.forges.insert(name, Box::new(f));
    }

    /// Register a web-search backend (parity spec 12).
    #[cfg(feature = "web-search")]
    pub fn web_search(
        &mut self,
        name: &'static str,
        f: impl Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn agent_core::WebSearch>>
            + Send
            + Sync
            + 'static,
    ) {
        self.web_searches.insert(name, Box::new(f));
    }
    #[cfg(feature = "git")]
    pub fn repo(
        &mut self,
        name: &'static str,
        f: impl Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn agent_core::RepoBackend>>
            + Send
            + Sync
            + 'static,
    ) {
        self.repos.insert(name, Box::new(f));
    }

    #[cfg(feature = "tokenizer")]
    pub fn tokenizer(
        &mut self,
        name: &'static str,
        f: impl Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn agent_core::Tokenizer>>
            + Send
            + Sync
            + 'static,
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

    pub fn build_provider(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn LlmProvider>> {
        let f = self
            .providers
            .get(name)
            .ok_or_else(|| unknown("provider", name, self.providers.keys().copied()))?;
        f(ctx)
    }
    pub fn build_context(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn ContextStrategy>> {
        let f = self
            .contexts
            .get(name)
            .ok_or_else(|| unknown("context strategy", name, self.contexts.keys().copied()))?;
        f(ctx)
    }
    pub fn build_policy(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn Policy>> {
        let f = self
            .policies
            .get(name)
            .ok_or_else(|| unknown("policy", name, self.policies.keys().copied()))?;
        f(ctx)
    }
    pub fn build_memory(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn MemoryStore>> {
        let f = self
            .memories
            .get(name)
            .ok_or_else(|| unknown("memory backend", name, self.memories.keys().copied()))?;
        f(ctx)
    }
    pub fn build_episodic(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn EpisodicStore>> {
        let f = self
            .episodics
            .get(name)
            .ok_or_else(|| unknown("episodic backend", name, self.episodics.keys().copied()))?;
        f(ctx)
    }
    pub fn build_semantic(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn SemanticStore>> {
        let f = self
            .semantics
            .get(name)
            .ok_or_else(|| unknown("semantic backend", name, self.semantics.keys().copied()))?;
        f(ctx)
    }
    pub fn build_tool(&self, name: &str, ctx: &FactoryCtx<'_>) -> anyhow::Result<Arc<dyn Tool>> {
        let f = self
            .tools
            .get(name)
            .ok_or_else(|| unknown("tool", name, self.tools.keys().copied()))?;
        f(ctx)
    }

    /// All registered tool names (used when `[tools] enabled` is empty ⇒ all).
    pub fn tool_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.tools.keys().copied()
    }

    #[cfg(feature = "search")]
    pub fn build_search(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn agent_core::SearchBackend>> {
        let f = self
            .searches
            .get(name)
            .ok_or_else(|| unknown("search backend", name, self.searches.keys().copied()))?;
        f(ctx)
    }

    #[cfg(feature = "forge")]
    pub fn build_forge(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn agent_core::Forge>> {
        let f = self
            .forges
            .get(name)
            .ok_or_else(|| unknown("forge backend", name, self.forges.keys().copied()))?;
        f(ctx)
    }

    /// Names of every registered web-search backend.
    #[cfg(feature = "web-search")]
    pub fn web_search_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.web_searches.keys().copied()
    }
    #[cfg(feature = "web-search")]
    pub fn build_web_search(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn agent_core::WebSearch>> {
        let f = self.web_searches.get(name).ok_or_else(|| {
            unknown(
                "web-search backend",
                name,
                self.web_searches.keys().copied(),
            )
        })?;
        f(ctx)
    }
    #[cfg(feature = "git")]
    pub fn build_repo(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn agent_core::RepoBackend>> {
        let f = self
            .repos
            .get(name)
            .ok_or_else(|| unknown("git backend", name, self.repos.keys().copied()))?;
        f(ctx)
    }

    #[cfg(feature = "tokenizer")]
    pub fn build_tokenizer(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn agent_core::Tokenizer>> {
        let f = self
            .tokenizers
            .get(name)
            .ok_or_else(|| unknown("tokenizer", name, self.tokenizers.keys().copied()))?;
        f(ctx)
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

    // --- context strategies (each budgets with the injected ctx.tokenizer()) ---
    #[cfg(feature = "context-sliding-window")]
    r.context("sliding-window", |ctx| {
        Ok(Arc::new(agent_context::SlidingWindow::new(
            ctx.tokenizer().cloned(),
            ctx.cfg.provider.model.clone(),
        )) as Arc<dyn ContextStrategy>)
    });
    #[cfg(feature = "context-summarizing")]
    r.context("summarizing-window", |ctx| {
        Ok(Arc::new(
            agent_context::SummarizingWindow::new(
                ctx.provider()?.clone(),
                ctx.cfg.agent.keep_recent_tokens,
            )
            .with_tokenizer(ctx.tokenizer().cloned(), ctx.cfg.provider.model.clone()),
        ) as Arc<dyn ContextStrategy>)
    });

    // --- tokenizer seam (accurate counts + cost, parity spec 23) ---
    #[cfg(feature = "tokenizer")]
    r.tokenizer("approx", |_ctx| {
        Ok(Arc::new(agent_tokenizer::ApproxTokenizer::new()) as Arc<dyn agent_core::Tokenizer>)
    });
    // One tokenizer for a fleet: identical counts everywhere, so budget and
    // compaction decisions stay consistent across agents.
    #[cfg(feature = "grpc")]
    r.tokenizer("grpc", |ctx| {
        let ep = grpc_client_endpoint(
            &ctx.cfg.grpc.tokenizer.endpoint,
            agent_grpc::constants::TOKENIZER,
        );
        Ok(Arc::new(agent_grpc::client::GrpcTokenizer::connect(&ep)?)
            as Arc<dyn agent_core::Tokenizer>)
    });

    // --- policies (always available; they live in agent-runtime) ---
    r.policy("auto-approve", |_ctx| {
        tracing::warn!(
            "policy=auto-approve: every tool call (including `bash`) runs WITHOUT \
             confirmation. Only use this on trusted goals/inputs — a prompt-injected \
             model can reach arbitrary code execution."
        );
        Ok(Arc::new(crate::policy::AutoApprove) as Arc<dyn Policy>)
    });
    r.policy("interactive", |_ctx| {
        Ok(Arc::new(crate::policy::Interactive) as Arc<dyn Policy>)
    });
    r.policy("allow-list", |ctx| {
        // Allow only the tool+arg patterns in `[policy] allow`; deny the rest.
        // An empty list denies everything (fail safe).
        let rules = ctx
            .cfg
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
        // backend), not a plain registry factory. See builder.rs.
        r.tool("read_file", |_ctx| {
            Ok(Arc::new(agent_tools::ReadFileTool) as Arc<dyn Tool>)
        });
        r.tool("write_file", |_ctx| {
            Ok(Arc::new(agent_tools::WriteFileTool) as Arc<dyn Tool>)
        });
    }
    #[cfg(feature = "tool-edit")]
    r.tool("edit", |_ctx| {
        Ok(Arc::new(agent_tools::EditTool) as Arc<dyn Tool>)
    });
    #[cfg(feature = "tool-patch")]
    r.tool("apply_patch", |_ctx| {
        Ok(Arc::new(agent_tools::ApplyPatchTool) as Arc<dyn Tool>)
    });
    #[cfg(feature = "tool-search")]
    {
        r.tool("grep", |_ctx| {
            Ok(Arc::new(agent_tools::GrepTool) as Arc<dyn Tool>)
        });
        r.tool("find", |_ctx| {
            Ok(Arc::new(agent_tools::FindTool) as Arc<dyn Tool>)
        });
        r.tool("ls", |_ctx| {
            Ok(Arc::new(agent_tools::LsTool) as Arc<dyn Tool>)
        });
    }

    // --- router: a provider that composes other providers (parity spec 25) ---
    //
    // A COMPOSING factory: it builds its candidates back through the registry,
    // which is why `FactoryCtx` carries a registry handle. Each candidate is an
    // ordinary provider — including a `grpc` client — so one router can span
    // local and remote providers.
    #[cfg(feature = "provider-router")]
    r.provider("router", |ctx| {
        let cfg = &ctx.cfg.router;
        if cfg.providers.is_empty() {
            anyhow::bail!(
                "[router] providers must list at least one provider name when \
                 `[agent] provider = \"router\"`"
            );
        }
        let registry = ctx.registry()?;
        let mut candidates = Vec::new();
        for name in &cfg.providers {
            if name == "router" {
                // Guard the obvious footgun: a router listing itself would
                // recurse until the stack blows.
                anyhow::bail!("[router] providers must not include `router` itself");
            }
            let provider = registry
                .build_provider(name, ctx)
                .with_context(|| format!("building router candidate `{name}`"))?;
            candidates.push(agent_providers::Candidate {
                name: name.clone(),
                provider: crate::metered::provider(provider, ctx.metrics.clone(), name),
            });
        }
        let metrics = ctx.metrics.clone();
        let router = agent_providers::Router::new(
            candidates,
            agent_providers::RoutePolicy::parse(&cfg.policy),
        )?
        .with_breaker(
            cfg.failure_threshold,
            cfg.cooldown_secs.saturating_mul(1_000),
        )
        .with_observer(Arc::new(move |ev| {
            crate::metered::record_route_event(&metrics, ev);
        }));
        Ok(Arc::new(router) as Arc<dyn LlmProvider>)
    });

    // --- forge backends (the Forge seam, parity spec 27) ---
    #[cfg(feature = "forge-github")]
    r.forge("github", |ctx| {
        let cfg = &ctx.cfg.forge;
        if cfg.owner.is_empty() || cfg.repo.is_empty() {
            anyhow::bail!("[forge] owner and repo must be set for the github backend");
        }
        Ok(Arc::new(agent_forge::GitHubForge::new(
            if cfg.base_url.is_empty() {
                "https://api.github.com".to_string()
            } else {
                cfg.base_url.clone()
            },
            cfg.owner.clone(),
            cfg.repo.clone(),
            resolve_ws_key(&cfg.token, &cfg.token_env),
            cfg.timeout_secs,
            cfg.max_retries,
        )?) as Arc<dyn agent_core::Forge>)
    });
    #[cfg(feature = "forge-gitlab")]
    r.forge("gitlab", |ctx| {
        let cfg = &ctx.cfg.forge;
        if cfg.project.is_empty() {
            anyhow::bail!("[forge] project must be set for the gitlab backend");
        }
        Ok(Arc::new(agent_forge::GitLabForge::new(
            if cfg.base_url.is_empty() {
                "https://gitlab.com/api/v4".to_string()
            } else {
                cfg.base_url.clone()
            },
            cfg.project.clone(),
            resolve_ws_key(&cfg.token, &cfg.token_env),
            cfg.timeout_secs,
            cfg.max_retries,
        )?) as Arc<dyn agent_core::Forge>)
    });

    // --- web-search backends (the WebSearch seam, parity spec 12) ---
    //
    // These are ORDINARY factory lines. Each needs config + the API key + retry
    // settings and nothing else, so nothing has to be special-cased in the
    // builder — the whole point of `FactoryCtx`.
    #[cfg(feature = "websearch-brave")]
    // Search behind one process: the API keys live there, so an agent can
    // search without ever holding one.
    #[cfg(feature = "grpc")]
    r.web_search("grpc", |ctx| {
        let ep = grpc_client_endpoint(
            &ctx.cfg.grpc.web_search.endpoint,
            agent_grpc::constants::WEB_SEARCH,
        );
        Ok(Arc::new(agent_grpc::client::GrpcWebSearch::connect(&ep)?)
            as Arc<dyn agent_core::WebSearch>)
    });
    r.web_search("brave", |ctx| {
        let cfg = &ctx.cfg.web_search;
        Ok(Arc::new(agent_web_search::BraveSearch::new(
            agent_web_search::HttpSearchConfig {
                endpoint: if cfg.brave_endpoint.is_empty() {
                    "https://api.search.brave.com/res/v1/web/search".to_string()
                } else {
                    cfg.brave_endpoint.clone()
                },
                api_key: resolve_ws_key(&cfg.brave_api_key, &cfg.brave_api_key_env),
                timeout_secs: cfg.timeout_secs,
                max_retries: cfg.max_retries,
            },
        )?) as Arc<dyn agent_core::WebSearch>)
    });
    #[cfg(feature = "websearch-searxng")]
    r.web_search("searxng", |ctx| {
        let cfg = &ctx.cfg.web_search;
        if cfg.searxng_endpoint.is_empty() {
            anyhow::bail!("[web_search] searxng_endpoint must be set to use the searxng backend");
        }
        Ok(Arc::new(agent_web_search::SearxngSearch::new(
            agent_web_search::HttpSearchConfig {
                endpoint: cfg.searxng_endpoint.clone(),
                api_key: String::new(),
                timeout_secs: cfg.timeout_secs,
                max_retries: cfg.max_retries,
            },
        )?) as Arc<dyn agent_core::WebSearch>)
    });

    // --- search backends (the SearchBackend seam) ---
    #[cfg(feature = "semantic-search")]
    // The vector backend meters its own Embedder, which is why it used to be
    // special-cased in `search.rs` instead of living here; `FactoryCtx` carries
    // `Metrics`, so it is now an ordinary factory like every other backend.
    r.search("vector", crate::search::build_vector);
    #[cfg(feature = "search")]
    {
        r.search("tantivy", |ctx| {
            let (root, index_dir) = search_paths(ctx.cfg, "tantivy")?;
            Ok(
                Arc::new(agent_search::TantivyBackend::open(root, index_dir)?)
                    as Arc<dyn agent_core::SearchBackend>,
            )
        });
        #[cfg(feature = "grpc")]
        r.search("grpc", |ctx| {
            let ep =
                grpc_client_endpoint(&ctx.cfg.grpc.search.endpoint, agent_grpc::constants::SEARCH);
            Ok(Arc::new(agent_grpc::client::GrpcSearch::connect(&ep)?)
                as Arc<dyn agent_core::SearchBackend>)
        });
    }

    // --- git backends (the RepoBackend seam) ---
    // The built-in local backend is wired in `crate::git::build_repo` (it needs
    // the session id, which the config-only factory can't carry). The remote
    // `= "grpc"` client backend registers here.
    #[cfg(all(feature = "git", feature = "grpc"))]
    r.repo("grpc", |ctx| {
        let ep = grpc_client_endpoint(&ctx.cfg.grpc.repo.endpoint, agent_grpc::constants::REPO);
        Ok(Arc::new(agent_grpc::client::GrpcRepo::connect(&ep)?)
            as Arc<dyn agent_core::RepoBackend>)
    });

    // --- gRPC seam clients (a remote seam is just another impl, selected by
    //     `= "grpc"`; endpoint from `[grpc]`, defaulting to the generated ports) ---
    #[cfg(feature = "grpc")]
    {
        r.provider("grpc", |ctx| {
            let ep = grpc_client_endpoint(
                &ctx.cfg.grpc.provider.endpoint,
                agent_grpc::constants::PROVIDER,
            );
            // Capabilities are config-derived (no eager round-trip) — the real model
            // lives behind the gateway; this just informs the loop.
            let caps = agent_core::ModelCapabilities {
                supports_tools: true,
                context_window: ctx.cfg.agent.context_window,
                supports_response_format: false,
                supports_vision: ctx.cfg.provider.supports_vision,
            };
            Ok(
                Arc::new(agent_grpc::client::GrpcProvider::connect(&ep, caps)?)
                    as Arc<dyn LlmProvider>,
            )
        });
        r.memory("grpc", |ctx| {
            let ep =
                grpc_client_endpoint(&ctx.cfg.grpc.memory.endpoint, agent_grpc::constants::MEMORY);
            Ok(Arc::new(agent_grpc::client::GrpcMemory::connect(&ep)?) as Arc<dyn MemoryStore>)
        });
        r.context("grpc", |ctx| {
            let ep = grpc_client_endpoint(
                &ctx.cfg.grpc.context.endpoint,
                agent_grpc::constants::CONTEXT,
            );
            Ok(Arc::new(agent_grpc::client::GrpcContext::connect(&ep)?)
                as Arc<dyn ContextStrategy>)
        });
        r.policy("grpc", |ctx| {
            let ep =
                grpc_client_endpoint(&ctx.cfg.grpc.policy.endpoint, agent_grpc::constants::POLICY);
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
pub(crate) fn grpc_client_endpoint(
    configured: &str,
    default: agent_grpc::constants::SeamEndpoint,
) -> agent_grpc::Endpoint {
    if configured.is_empty() {
        agent_grpc::Endpoint::parse(&format!("127.0.0.1:{}", default.tcp_port))
    } else {
        agent_grpc::Endpoint::parse(configured)
    }
}

/// Resolve a web-search API key: inline value first, then the named env var.
/// The key is never logged or echoed — see `agent-web-search`.
#[cfg(any(feature = "web-search", feature = "forge"))]
fn resolve_ws_key(inline: &str, env_var: &str) -> String {
    if !inline.is_empty() {
        return inline.to_string();
    }
    if env_var.is_empty() {
        return String::new();
    }
    std::env::var(env_var).unwrap_or_default()
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
        // The vector backend used to be special-cased in `search.rs` because a
        // factory had no `Metrics` handle. It is an ordinary registry entry now.
        #[cfg(feature = "semantic-search")]
        assert!(
            r.searches.contains_key("vector"),
            "vector must be a plain registry factory"
        );
    }

    /// A seam that asks for the provider before it exists gets a clear error,
    /// not a panic — the ordering constraint is the price of one uniform
    /// signature, so it must fail legibly.
    #[test]
    fn negative_provider_before_it_is_built_errors_clearly() {
        let cfg = crate::config::Config::minimal_for_test();
        let metrics = Metrics::new();
        let ctx = FactoryCtx::new(&cfg, &metrics);
        let err = match ctx.provider() {
            Ok(_) => panic!("no provider is attached; this must fail"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("not built yet"), "unhelpful error: {err}");
        assert!(ctx.tokenizer().is_none());
    }

    /// Once attached, the ctx hands the provider back.
    #[test]
    fn positive_ctx_exposes_the_built_provider() {
        let cfg = crate::config::Config::minimal_for_test();
        let metrics = Metrics::new();
        let p: Arc<dyn LlmProvider> = Arc::new(agent_testkit::ScriptedProvider::new(vec![
            agent_testkit::final_turn("ok"),
        ]));
        let ctx = FactoryCtx::new(&cfg, &metrics).with_provider(&p);
        assert!(ctx.provider().is_ok());
    }

    #[test]
    fn unknown_key_lists_known_names() {
        let r = Registry::with_builtins();
        let cfg = crate::config::Config::minimal_for_test();
        let metrics = Metrics::new();
        let err = r
            .build_policy("nope", &FactoryCtx::new(&cfg, &metrics))
            .map(|_| ())
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown policy `nope`"));
        assert!(err.contains("auto-approve"));
    }
}

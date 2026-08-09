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

/// A boxed seam factory: given the shared [`FactoryCtx`], produce the (metered)
/// impl. Generic over the seam's unsized trait object (`dyn LlmProvider`,
/// `dyn Tool`, …), so every seam shares this one alias rather than a per-seam
/// `XFactory` type.
type Factory<T /* the seam's unsized trait object */> =
    Box<dyn Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<T>> + Send + Sync>;

/// Name → factory maps for every swappable seam. Keys are `&'static str` and the
/// maps are ordered so error messages list known names deterministically.
#[derive(Default)]
pub struct Registry {
    providers: BTreeMap<&'static str, Factory<dyn LlmProvider>>,
    contexts: BTreeMap<&'static str, Factory<dyn ContextStrategy>>,
    policies: BTreeMap<&'static str, Factory<dyn Policy>>,
    memories: BTreeMap<&'static str, Factory<dyn MemoryStore>>,
    episodics: BTreeMap<&'static str, Factory<dyn EpisodicStore>>,
    semantics: BTreeMap<&'static str, Factory<dyn SemanticStore>>,
    tools: BTreeMap<&'static str, Factory<dyn Tool>>,
    #[cfg(feature = "search")]
    searches: BTreeMap<&'static str, Factory<dyn agent_core::SearchBackend>>,
    #[cfg(feature = "verifier")]
    verifiers: BTreeMap<&'static str, Factory<dyn agent_core::Verifier>>,
    #[cfg(feature = "forge")]
    forges: BTreeMap<&'static str, Factory<dyn agent_core::Forge>>,
    #[cfg(feature = "web-search")]
    web_searches: BTreeMap<&'static str, Factory<dyn agent_core::WebSearch>>,
    #[cfg(feature = "git")]
    repos: BTreeMap<&'static str, Factory<dyn agent_core::RepoBackend>>,
    #[cfg(feature = "tokenizer")]
    tokenizers: BTreeMap<&'static str, Factory<dyn agent_core::Tokenizer>>,
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
    #[cfg(feature = "verifier")]
    pub fn verifier(
        &mut self,
        name: &'static str,
        f: impl Fn(&FactoryCtx<'_>) -> anyhow::Result<Arc<dyn agent_core::Verifier>>
            + Send
            + Sync
            + 'static,
    ) {
        self.verifiers.insert(name, Box::new(f));
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
        build_from(&self.providers, "provider", name, ctx)
    }
    pub fn build_context(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn ContextStrategy>> {
        build_from(&self.contexts, "context strategy", name, ctx)
    }
    pub fn build_policy(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn Policy>> {
        build_from(&self.policies, "policy", name, ctx)
    }
    pub fn build_memory(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn MemoryStore>> {
        build_from(&self.memories, "memory backend", name, ctx)
    }
    pub fn build_episodic(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn EpisodicStore>> {
        build_from(&self.episodics, "episodic backend", name, ctx)
    }
    pub fn build_semantic(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn SemanticStore>> {
        build_from(&self.semantics, "semantic backend", name, ctx)
    }
    pub fn build_tool(&self, name: &str, ctx: &FactoryCtx<'_>) -> anyhow::Result<Arc<dyn Tool>> {
        build_from(&self.tools, "tool", name, ctx)
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
        build_from(&self.searches, "search backend", name, ctx)
    }
    #[cfg(feature = "verifier")]
    pub fn build_verifier(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn agent_core::Verifier>> {
        build_from(&self.verifiers, "verifier", name, ctx)
    }

    #[cfg(feature = "forge")]
    pub fn build_forge(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn agent_core::Forge>> {
        build_from(&self.forges, "forge backend", name, ctx)
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
        build_from(&self.web_searches, "web-search backend", name, ctx)
    }
    #[cfg(feature = "git")]
    pub fn build_repo(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn agent_core::RepoBackend>> {
        build_from(&self.repos, "git backend", name, ctx)
    }

    #[cfg(feature = "tokenizer")]
    pub fn build_tokenizer(
        &self,
        name: &str,
        ctx: &FactoryCtx<'_>,
    ) -> anyhow::Result<Arc<dyn agent_core::Tokenizer>> {
        build_from(&self.tokenizers, "tokenizer", name, ctx)
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

/// Look up `name` in a seam's factory map and invoke it, or return an
/// [`unknown`] error listing the registered names. Shared by every `build_*`
/// method so the lookup/error pattern lives in one place.
fn build_from<T: ?Sized>(
    map: &BTreeMap<&'static str, Factory<T>>,
    kind: &str,
    name: &str,
    ctx: &FactoryCtx<'_>,
) -> anyhow::Result<Arc<T>> {
    let f = map
        .get(name)
        .ok_or_else(|| unknown(kind, name, map.keys().copied()))?;
    f(ctx)
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
    // Mode-aware compaction (adaptive-cognition 02): summarizing between switches,
    // a destination-lens reshape on one. The runtime arms it via `on_mode_switch`
    // when the classifier (increment 01) decides a switch.
    #[cfg(feature = "context-mode-aware")]
    r.context("mode-aware-window", |ctx| {
        Ok(Arc::new(
            agent_context::ModeAwareWindow::new(
                ctx.provider()?.clone(),
                ctx.cfg.agent.keep_recent_tokens,
            )
            .with_tokenizer(ctx.tokenizer().cloned(), ctx.cfg.provider.model.clone())
            // Point the destination-mode lens at operator overrides under
            // `<prompts>/lens/` (docs/design/portal); compiled defaults otherwise.
            .with_lens_dir(Some(&ctx.cfg.prompts.dir)),
        ) as Arc<dyn ContextStrategy>)
    });

    // --- tokenizer seam (accurate counts + cost, parity spec 23) ---
    #[cfg(feature = "tokenizer")]
    r.tokenizer("approx", |_ctx| {
        Ok(Arc::new(agent_tokenizer::ApproxTokenizer::new()) as Arc<dyn agent_core::Tokenizer>)
    });
    // Real OpenAI-family BPE counts (offline, vendored ranks); unrecognised models
    // fall back to `approx` inside the backend. See parity spec 23.
    #[cfg(feature = "tokenizer-tiktoken")]
    r.tokenizer("tiktoken", |_ctx| {
        Ok(Arc::new(agent_tokenizer::TiktokenTokenizer::new()?) as Arc<dyn agent_core::Tokenizer>)
    });
    // Count with a local model's `tokenizer.json` (Qwen/GLM/Llama/…). Paths come
    // from `[tokenizer.hf]`; a missing/oversized/unparseable file is skipped inside
    // the backend and those models fall back to `approx`. See parity spec 23.
    #[cfg(feature = "tokenizer-hf")]
    r.tokenizer("hf", |ctx| {
        let hf = &ctx.cfg.tokenizer.hf;
        let mut files: Vec<(String, std::path::PathBuf)> = hf
            .models
            .iter()
            .map(|(prefix, path)| (prefix.clone(), std::path::PathBuf::from(path)))
            .collect();
        // The catch-all default is keyed by the empty prefix (sorts last).
        if let Some(default) = &hf.default {
            files.push((String::new(), std::path::PathBuf::from(default)));
        }
        Ok(Arc::new(agent_tokenizer::HfTokenizer::new(&files)) as Arc<dyn agent_core::Tokenizer>)
    });
    // Count via a provider's `messages/count_tokens` endpoint (network + key). The
    // key is resolved inline > env and never logged; construction fails closed if
    // base_url/key are unset. See parity spec 23.
    #[cfg(feature = "tokenizer-provider")]
    r.tokenizer("provider", |ctx| {
        let p = &ctx.cfg.tokenizer.provider;
        let key = resolve_ws_key(&p.api_key, &p.api_key_env);
        Ok(Arc::new(agent_tokenizer::ProviderTokenizer::new(
            &p.base_url,
            &key,
            &p.version,
            p.timeout_secs,
        )?) as Arc<dyn agent_core::Tokenizer>)
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

    // --- verifiers (the tool-call correctness gate; off unless selected) ---
    #[cfg(feature = "verifier")]
    r.verifier("schema", |_ctx| {
        // Deterministic, model-free: validate a call's arguments against the
        // tool's JSON Schema. See docs/design/tool-call-verification.md.
        Ok(Arc::new(agent_verifier::SchemaVerifier::new()) as Arc<dyn agent_core::Verifier>)
    });
    #[cfg(feature = "verifier")]
    r.verifier("llm", |ctx| {
        // Model-backed: ask the built provider to judge the call's correctness. Uses
        // the same provider as the loop (self-verification); a distinct verifier model
        // is a follow-up (per-member providers under the `ensemble` backend).
        let provider = ctx.provider()?.clone();
        Ok(Arc::new(agent_verifier::LlmVerifier::new(provider)) as Arc<dyn agent_core::Verifier>)
    });
    #[cfg(feature = "verifier")]
    r.verifier("ensemble", |ctx| {
        // A COMPOSING factory (like the `router` provider): build each member back
        // through the registry, so an ensemble can span schema + llm + a `grpc` member.
        let members = &ctx.cfg.verifier.members;
        if members.is_empty() {
            anyhow::bail!(
                "[verifier] members must list at least one verifier name when \
                 `[verifier] backend = \"ensemble\"`"
            );
        }
        let registry = ctx.registry()?;
        let mut built = Vec::new();
        for name in members {
            if name == "ensemble" {
                anyhow::bail!("[verifier] members must not include `ensemble` itself");
            }
            built.push(
                registry
                    .build_verifier(name, ctx)
                    .with_context(|| format!("building ensemble member `{name}`"))?,
            );
        }
        Ok(Arc::new(agent_verifier::Ensemble::new(built)) as Arc<dyn agent_core::Verifier>)
    });

    // --- memory backends (whole-store + independently-swappable layers) ---
    #[cfg(feature = "memory-file")]
    {
        r.memory("file", crate::builder::file_memory);
        r.episodic("file", crate::builder::file_episodic);
        r.semantic("file", crate::builder::file_semantic);
        // The layers, independently remotable: the append-only log can live on
        // one host and the vector store on another, which is the reason they
        // are separate traits at all.
        #[cfg(feature = "grpc")]
        {
            r.episodic("grpc", |ctx| {
                let ep = grpc_client_endpoint(
                    &ctx.cfg.grpc.episodic.endpoint,
                    agent_grpc::constants::EPISODIC,
                );
                Ok(Arc::new(agent_grpc::client::GrpcEpisodic::connect(&ep)?)
                    as Arc<dyn agent_core::EpisodicStore>)
            });
            r.semantic("grpc", |ctx| {
                let ep = grpc_client_endpoint(
                    &ctx.cfg.grpc.semantic.endpoint,
                    agent_grpc::constants::SEMANTIC,
                );
                Ok(Arc::new(agent_grpc::client::GrpcSemantic::connect(&ep)?)
                    as Arc<dyn agent_core::SemanticStore>)
            });
        }
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

    // --- task-router: metadata-driven, declaratively-routed generator (model-router 02) ---
    //
    // Like `router` it composes providers, but picks by running the declarative
    // `[route]` policy over each upstream's live capabilities + configured
    // tags/tier/cost against the request's requirements. An inline `endpoint` builds
    // an OpenAI-compatible provider (no secret in the config — key via
    // api_key_file/env); an empty endpoint resolves the name through the registry.
    #[cfg(feature = "provider-router")]
    r.provider("task-router", |ctx| {
        let cfg = &ctx.cfg.route;
        if cfg.upstreams.is_empty() {
            anyhow::bail!(
                "[route] upstreams must list at least one upstream when \
                 `[agent] provider = \"task-router\"`"
            );
        }
        let registry = ctx.registry()?;
        let mut upstreams = Vec::new();
        for u in &cfg.upstreams {
            if u.name == "task-router" {
                anyhow::bail!("[route] upstreams must not include `task-router` itself");
            }
            let provider: Arc<dyn LlmProvider> = if u.endpoint.is_empty() {
                registry
                    .build_provider(&u.name, ctx)
                    .with_context(|| format!("building route upstream `{}`", u.name))?
            } else {
                Arc::new(crate::builder::build_route_upstream(
                    u,
                    ctx.cfg.agent.context_window,
                )?) as Arc<dyn LlmProvider>
            };
            let tier = agent_core::PoolTier::parse(&u.tier).unwrap_or(agent_core::PoolTier::Medium);
            upstreams.push(agent_providers::RouterUpstream {
                id: u.name.clone(),
                tags: u.tags.clone(),
                tier,
                input_cost: u.input_cost.unwrap_or(0.0),
                provider: crate::metered::provider(provider, ctx.metrics.clone(), &u.name),
            });
        }
        let metrics = ctx.metrics.clone();
        let router =
            agent_providers::TaskRouter::new(upstreams, crate::builder::build_route_policy(cfg))?
                .with_breaker(
                    cfg.failure_threshold,
                    cfg.cooldown_secs.saturating_mul(1_000),
                )
                .with_observer(Arc::new(move |ev| {
                    crate::metered::record_route_event(&metrics, ev);
                }));
        Ok(Arc::new(router) as Arc<dyn LlmProvider>)
    });

    // --- consensus: generator × critic response gate (cognition-graph 01) ---
    //
    // Composing factory: `[consensus] generator/critic` each resolve like a route
    // upstream — a `[[route.upstreams]]` entry by name (inline endpoint synthesized
    // secret-safely, empty endpoint via the registry) — else as a registry provider
    // type. The critic should be a different model family; same-name is allowed but
    // warned (it defeats the self-preference mitigation).
    #[cfg(feature = "provider-consensus")]
    r.provider("consensus", |ctx| {
        let cfg = &ctx.cfg.consensus;
        if cfg.generator.is_empty() || cfg.critic.is_empty() {
            anyhow::bail!(
                "[consensus] generator and critic must both be set when \
                 `[agent] provider = \"consensus\"`"
            );
        }
        if cfg.generator == cfg.critic {
            tracing::warn!(
                upstream = %cfg.generator,
                "[consensus] generator and critic are the SAME upstream — \
                 self-critique loses the cross-family bias mitigation"
            );
        }
        let resolve = |name: &str| -> anyhow::Result<Arc<dyn LlmProvider>> {
            if name == "consensus" {
                anyhow::bail!("[consensus] must not reference `consensus` itself");
            }
            let built: Arc<dyn LlmProvider> =
                match ctx.cfg.route.upstreams.iter().find(|u| u.name == name) {
                    Some(u) if !u.endpoint.is_empty() => Arc::new(
                        crate::builder::build_route_upstream(u, ctx.cfg.agent.context_window)?,
                    ),
                    _ => ctx
                        .registry()?
                        .build_provider(name, ctx)
                        .with_context(|| format!("building consensus member `{name}`"))?,
                };
            Ok(crate::metered::provider(built, ctx.metrics.clone(), name))
        };
        let generator = resolve(&cfg.generator)?;
        let critic = resolve(&cfg.critic)?;

        let mut gate = agent_providers::GateCfg {
            max_rounds: cfg.max_rounds,
            critic_max_tokens: cfg.critic_max_tokens,
            max_alternatives: cfg.max_alternatives,
            ..agent_providers::GateCfg::default()
        };
        gate.scope = match cfg.scope.as_str() {
            "" | "final" => agent_providers::GateScope::Final,
            "every-iteration" => agent_providers::GateScope::EveryIteration,
            other => anyhow::bail!("[consensus] scope: unknown value `{other}`"),
        };
        gate.on_exhaustion = match cfg.on_exhaustion.as_str() {
            "" | "deliver-with-note" => agent_providers::Exhaustion::DeliverWithNote,
            "fail" => agent_providers::Exhaustion::Fail,
            other => anyhow::bail!("[consensus] on_exhaustion: unknown value `{other}`"),
        };
        if !cfg.rubric_file.is_empty() {
            let text = std::fs::read_to_string(&cfg.rubric_file)
                .with_context(|| format!("[consensus] rubric_file `{}`", cfg.rubric_file))?;
            gate.rubric = Some(text);
        }

        let metrics = ctx.metrics.clone();
        let provider = agent_providers::ConsensusProvider::new(generator, critic)
            .with_cfg(gate)
            .with_observer(Arc::new(move |o: &agent_providers::GateOutcome| {
                crate::metered::record_gate_outcome(&metrics, o);
            }));
        Ok(Arc::new(provider) as Arc<dyn LlmProvider>)
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

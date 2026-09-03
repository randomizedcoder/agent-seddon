//! The agent loop — the one place that ties the seams together.
//!
//! It depends only on the `agent-core` traits; every concrete component was
//! chosen by the factory in `builder.rs`. The loop shape is deliberately
//! ordinary (DESIGN.md §2): assemble → complete → dispatch tools → record →
//! compact → repeat until the model stops asking for tools.

use agent_core::{
    CompletionRequest, CompletionResponse, ContextBlock, ContextInput, ContextStrategy, Decision,
    LlmProvider, MemoryEvent, MemoryStore, Message, Observation, Policy, RecallQuery, Role,
    TokenBudget, Tool, ToolContext, ToolRegistry, ToolSchema, WorkingSet,
};
use agent_metrics::{Metrics, SessionMetrics};
use futures_util::StreamExt;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::Instrument;

#[derive(Clone)]
pub struct Settings {
    pub max_iterations: usize,
    pub max_tokens: u32,
    pub temperature: f32,
    pub context_window: u32,
    pub reserve_output: u32,
    pub system_prompt: String,
    /// Echo streamed assistant text live to stderr.
    pub stream: bool,
    /// Run a turn's parallel-safe tool calls concurrently.
    pub parallel_tools: bool,
    /// Per-tool wall-clock timeout (seconds); a hung tool becomes an error
    /// observation rather than freezing the loop. `0` disables (e.g. relying on
    /// `bash`'s own timeout).
    pub tool_timeout_secs: u64,
    pub recall_limit: usize,
    pub cwd: PathBuf,
    /// Model name, used as a metrics label.
    pub model: String,
    /// Per-run id, stamped on every recorded event (empty when telemetry is off).
    pub session_id: String,
    /// Always-injected user context (context.d/prepend and /append).
    pub context_prepend: Vec<ContextBlock>,
    pub context_append: Vec<ContextBlock>,
    /// Auto-detect a code-review task mid-conversation and inject grounded facts
    /// (docs/design/code-review/). Off unless `[review] in_loop = true`.
    pub review_in_loop: bool,
    /// Byte budget for the rendered review context (`[review] context_budget_bytes`).
    pub review_context_budget: usize,
    /// A candidate mode must reach this confidence before a switch is considered
    /// (`[mode] confidence_floor`). See docs/design/adaptive-cognition/01-mode.md.
    pub mode_confidence_floor: f32,
    /// Consecutive turns a non-decisive candidate mode must win to switch
    /// (`[mode] hysteresis`); a decisive deterministic signal switches immediately.
    pub mode_hysteresis: usize,
    /// Overload admission cap for served gRPC seams (`[grpc] max_in_flight`): concurrent
    /// in-flight requests before shedding with `RESOURCE_EXHAUSTED`. `0` = unbounded.
    pub grpc_max_in_flight: usize,
}

pub struct Agent {
    provider: Arc<dyn LlmProvider>,
    tools: ToolRegistry,
    memory: Arc<dyn MemoryStore>,
    context: Arc<dyn ContextStrategy>,
    policy: Arc<dyn Policy>,
    metrics: Metrics,
    settings: Settings,
    /// Live agent-session observation, **one sink per session** keyed by session id
    /// (docs/design/multi-session/03-hazards.md, hazard B). Each `Session` draws its
    /// own [`crate::SessionEvents`] from here at construction and publishes into that;
    /// a served `AgentSessionService` reads this registry via
    /// [`Self::session_source_registry`] and selects by `session_id`, so concurrent
    /// tenants never share a stream. Observe-only.
    events: Arc<crate::SessionEventsRegistry>,
    /// The composed search backend, if the `search` seam is wired. Held so it can
    /// be hosted over gRPC (`agent --serve-search`); the loop reaches search
    /// through the `search` *tool*, not this field.
    search: Option<Arc<dyn agent_core::SearchBackend>>,
    /// The composed code-graph backend (the `AstBackend` seam), if wired. Held so it
    /// can be hosted over gRPC (`agent --serve-ast`); the loop reaches it through the
    /// `find_*` tools.
    ast: Option<Arc<dyn agent_core::AstBackend>>,
    /// The git backend, if the `git` seam is wired. Held so it can be hosted over
    /// gRPC (`agent --serve-git`); the loop reaches it through the `git_*` tools.
    repo: Option<Arc<dyn agent_core::RepoBackend>>,
    /// The health-checked LLM pool, if `[pool] members` is configured. Used by the
    /// review classifier's vote and hosted over gRPC (`agent --serve-llm-pool`).
    llm_pool: Option<Arc<dyn agent_core::LlmPool>>,
    /// The task-mode classifier, if the `review` seam is wired. Detects a review
    /// task for the in-loop hand-off.
    task_classifier: Option<Arc<dyn agent_core::TaskClassifier>>,
    /// The dimensional-memory store, if the `dimensions` seam is wired
    /// (adaptive-cognition 03). Runs a cheap per-turn summarize pass and answers
    /// the mode-switch "pull in fresh" recall; hosted over gRPC
    /// (`agent --serve-dimension`).
    dimension_store: Option<Arc<dyn agent_core::DimensionStore>>,
    /// The prompt-management store, if the `prompt` seam is wired
    /// (docs/design/portal). Operator/portal-facing — the loop does not consume it;
    /// held only so it can be hosted over gRPC (`agent --serve-prompt`).
    prompt_store: Option<Arc<dyn agent_core::PromptStore>>,
    /// The provider-registry control plane (model-router 03), held for
    /// `--serve-provider-registry`; the loop itself does not consume it until
    /// the registry-backed router (increment 04).
    provider_registry: Option<Arc<dyn agent_core::ProviderRegistry>>,
    /// Situational system-prompt fragments selected by the current mode
    /// (docs/design/prompts/). Unlike `prompt_store`, the loop **does** consume this:
    /// each turn it selects the fragments whose tags match the situation and injects
    /// them as a system message. Defaults to a no-op resolver (no `prompts` dir ⇒ no
    /// fragments ⇒ byte-identical to today).
    system_fragments: agent_context::system_fragments::SystemFragments,
    /// The metrics proxy, if the `metrics-proxy` seam is wired (docs/design/portal).
    /// Portal-facing PromQL→Prometheus proxy; held only so it can be hosted over
    /// gRPC (`agent --serve-metrics-proxy`). Not consumed by the loop.
    metrics_proxy: Option<Arc<dyn agent_core::MetricsProxy>>,
    /// The review fact collector, if the `review` seam is wired. Produces the
    /// grounded `ReviewFacts`; hosted over gRPC (`agent --serve-fact-collector`).
    review_collector: Option<Arc<dyn agent_core::ReviewCollector>>,
    /// The composed content scanner, if the `scanner` seam is wired. Held so it
    /// can be hosted over gRPC (`agent --serve-scanner`); the loop reaches it
    /// through the policy guard and the `skill_write` / `session_export` tools.
    scanner: Option<Arc<dyn agent_core::Scanner>>,
    /// The tool-call verifier, if the `verifier` seam is wired. See
    /// docs/design/tool-call-verification.md.
    verifier: Option<Arc<dyn agent_core::Verifier>>,
    /// `[verifier] mode = "enforce"`: a `Revise`/`Deny` verdict blocks the call and
    /// feeds its message back. Default (`false`) is shadow — verdict observed only.
    verifier_enforce: bool,
    /// The tokenizer, if wired. Held so it can be hosted over gRPC
    /// (`agent --serve-tokenizer`); the loop reaches it through the context
    /// strategy's budget calculation.
    tokenizer: Option<Arc<dyn agent_core::Tokenizer>>,
    /// The web transport, if wired. Held so it can be hosted over gRPC
    /// (`agent --serve-web`); the loop reaches it through the `web_fetch` tool
    /// and the `@url` reference route.
    web: Option<Arc<dyn agent_core::WebBackend>>,
    /// The composed web-search dispatch (cache + fusion), if wired. Held so
    /// `agent --serve-web-search` hosts the composite rather than one backend.
    web_search: Option<Arc<dyn agent_core::WebSearch>>,
    /// The sandbox backing `bash`, if wired. Held so it can be hosted over gRPC
    /// (`agent --serve-sandbox`).
    sandbox: Option<Arc<dyn agent_core::Sandbox>>,
    /// The pty backend, if wired. Held so it can be hosted over gRPC
    /// (`agent --serve-pty`).
    pty: Option<Arc<dyn agent_core::Pty>>,
    /// The forge backend, if wired. Held so it can be hosted over gRPC
    /// (`agent --serve-forge`); the loop reaches it through the `forge` tool.
    forge: Option<Arc<dyn agent_core::Forge>>,
    /// The task tracker, if wired. Held so it can be hosted over gRPC
    /// (`agent --serve-tasks`); the loop reaches it through `todo_write`.
    tasks: Option<Arc<dyn agent_core::TaskTracker>>,
    /// The LSP backend, if wired. Held so it can be hosted over gRPC
    /// (`agent --serve-lsp`); the loop reaches it through the `lsp` tool.
    lsp: Option<Arc<dyn agent_core::LspBackend>>,
    /// The memory layers, when `[memory] semantic` composes them. Held so each
    /// can be hosted over gRPC (`agent --serve-episodic` / `--serve-semantic`);
    /// the loop reaches memory through the composed facade.
    episodic: Option<Arc<dyn agent_core::EpisodicStore>>,
    semantic: Option<Arc<dyn agent_core::SemanticStore>>,
    /// The embedder, if wired. Held so it can be hosted over gRPC
    /// (`agent --serve-embed`); the loop reaches it through the vector search
    /// backend.
    ///
    /// NOT feature-gated, deliberately: `agent-cli` has no `semantic-search`
    /// feature of its own, so a `#[cfg]` here would be always-false there and
    /// would silently disable `--serve-embed`. `None` when the feature is off,
    /// exactly like `search` and `repo`.
    embedder: Option<Arc<dyn agent_core::Embedder>>,
    /// The (metered) structured-output validator, if the `structured` seam is
    /// wired. Reached via [`Agent::complete_structured`] (parity spec 16).
    #[cfg(feature = "structured")]
    validator: Option<Arc<dyn agent_core::OutputSchema>>,
    /// The (metered) session-history store, if the `session` seam is wired. Reached
    /// via [`Agent::checkpoint`] / [`Agent::restore_checkpoint`] (parity spec 19).
    #[cfg(feature = "session")]
    session_store: Option<Arc<dyn agent_core::SessionStore>>,
    /// The (metered) `@`-reference resolver, if the `reference` seam is wired.
    /// Reached via [`Agent::resolve_references`] (parity spec 17).
    #[cfg(feature = "reference")]
    reference: Option<Arc<dyn agent_core::ReferenceResolver>>,
    /// Token budget a prompt's `@`-mentions may expand into (0 ⇒ unbounded).
    #[cfg(feature = "reference")]
    reference_budget: usize,
    /// Lifecycle hooks fired from the loop (parity spec 22). Empty by default,
    /// and every dispatch short-circuits when empty, so an agent without hooks
    /// pays nothing.
    hooks: agent_core::HookRegistry,
    /// Checkpoint the working set after each completed turn (parity spec 19).
    #[cfg(feature = "session")]
    auto_checkpoint: bool,
    /// The scheduler, if wired — held so the `--scheduler` driver can tick it
    /// (parity spec 28).
    #[cfg(feature = "scheduler")]
    scheduler: Option<Arc<agent_scheduler::LocalScheduler>>,
    /// The digest ledger the per-session background distiller writes and instant
    /// compaction reads (cognition-graph 02). `None` ⇒ distillation is off.
    digests: Option<Arc<dyn agent_core::DigestStore>>,
    /// `(summary_max_tokens, facts_max_tokens)` for the distiller's completions.
    distill_tokens: (u32, u32),
    /// `(summary, facts)` — which distill kinds run. `(true, true)` under the
    /// TOML wiring; a cognition graph enables only the kinds whose background
    /// nodes exist.
    pub(crate) distill_kinds: (bool, bool),
    /// Role routing (`[digest] provider` / a distill node's `provider`): the
    /// provider the distiller uses. `None` = the main provider — distillation
    /// is background cache work, so a cheap/local model fits here.
    pub(crate) distill_provider: Option<Arc<dyn LlmProvider>>,
    /// The cognition-graph document store (cognition-graph 04). `None` ⇒ the
    /// graph-less built-in behavior.
    graph: Option<Arc<dyn agent_core::GraphStore>>,
}

impl Agent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        tools: ToolRegistry,
        memory: Arc<dyn MemoryStore>,
        context: Arc<dyn ContextStrategy>,
        policy: Arc<dyn Policy>,
        metrics: Metrics,
        settings: Settings,
    ) -> Self {
        let events = Arc::new(crate::SessionEventsRegistry::new(settings.context_window));
        Self {
            provider,
            tools,
            memory,
            context,
            policy,
            metrics,
            settings,
            events,
            search: None,
            ast: None,
            repo: None,
            llm_pool: None,
            task_classifier: None,
            dimension_store: None,
            prompt_store: None,
            provider_registry: None,
            system_fragments: agent_context::system_fragments::SystemFragments::defaults(),
            metrics_proxy: None,
            review_collector: None,
            scanner: None,
            verifier: None,
            verifier_enforce: false,
            tokenizer: None,
            web: None,
            web_search: None,
            sandbox: None,
            pty: None,
            forge: None,
            tasks: None,
            lsp: None,
            episodic: None,
            semantic: None,
            embedder: None,
            #[cfg(feature = "structured")]
            validator: None,
            #[cfg(feature = "session")]
            session_store: None,
            #[cfg(feature = "reference")]
            reference: None,
            #[cfg(feature = "reference")]
            reference_budget: 0,
            hooks: agent_core::HookRegistry::new(),
            #[cfg(feature = "session")]
            auto_checkpoint: false,
            #[cfg(feature = "scheduler")]
            scheduler: None,
            digests: None,
            distill_tokens: (512, 256),
            distill_kinds: (true, true),
            distill_provider: None,
            graph: None,
        }
    }

    /// Attach the digest ledger + distiller budgets (cognition-graph 02): every
    /// delivered response gets background-summarized into `store`.
    pub fn with_digests(
        mut self,
        store: Arc<dyn agent_core::DigestStore>,
        summary_max_tokens: u32,
        facts_max_tokens: u32,
    ) -> Self {
        self.digests = Some(store);
        self.distill_tokens = (summary_max_tokens, facts_max_tokens);
        self
    }

    /// Attach the cognition-graph document store (cognition-graph 04).
    pub fn with_graph(mut self, store: Arc<dyn agent_core::GraphStore>) -> Self {
        self.graph = Some(store);
        self
    }

    /// Restrict which distill kinds run (cognition-graph 04: with a non-empty
    /// graph, a kind runs only if its background node exists).
    pub fn with_distill_kinds(mut self, summary: bool, facts: bool) -> Self {
        self.distill_kinds = (summary, facts);
        self
    }

    /// Route the distiller's summary/facts calls to a dedicated provider
    /// (`[digest] provider` — role routing).
    pub fn with_distill_provider(mut self, p: Arc<dyn LlmProvider>) -> Self {
        self.distill_provider = Some(p);
        self
    }

    /// Attach the scheduler (parity spec 28).
    #[cfg(feature = "scheduler")]
    pub fn with_scheduler(mut self, s: Arc<agent_scheduler::LocalScheduler>) -> Self {
        self.scheduler = Some(s);
        self
    }

    /// The scheduler, if wired.
    #[cfg(feature = "scheduler")]
    pub fn scheduler(&self) -> Option<Arc<agent_scheduler::LocalScheduler>> {
        self.scheduler.clone()
    }

    /// The scheduler as the bare seam, for `agent --serve-scheduler`.
    ///
    /// Separate from [`Agent::scheduler`] because driving jobs needs the
    /// concrete type: `tick_with` takes the executor closure and is deliberately
    /// NOT on the `Scheduler` trait, since a job's executor is this agent. So a
    /// remote client can manage the registry (schedule/list/cancel/history) but
    /// only the process that owns the scheduler can fire its jobs.
    #[cfg(feature = "scheduler")]
    pub fn scheduler_seam(&self) -> Option<Arc<dyn agent_core::Scheduler>> {
        self.scheduler
            .clone()
            .map(|s| s as Arc<dyn agent_core::Scheduler>)
    }

    /// Fire every due job once, running each as a fresh headless turn.
    ///
    /// Returns how many ran. The executor is supplied here rather than stored by
    /// the scheduler, which is what keeps agent and scheduler from owning each
    /// other.
    #[cfg(feature = "scheduler")]
    pub async fn tick_scheduler(self: &Arc<Self>) -> usize {
        let Some(s) = &self.scheduler else { return 0 };
        // Each due job runs as a fresh headless session; clone the `Arc` backend into
        // the executor so the returned future owns it (no borrow of `self`).
        let this = Arc::clone(self);
        s.tick_with(move |goal| {
            let this = Arc::clone(&this);
            async move {
                this.run(&goal)
                    .await
                    .map_err(|e| agent_core::Error::Scheduler(e.to_string()))
            }
        })
        .await
    }

    /// Checkpoint automatically after each completed turn (parity spec 19).
    #[cfg(feature = "session")]
    pub fn with_auto_checkpoint(mut self, yes: bool) -> Self {
        self.auto_checkpoint = yes;
        self
    }

    /// Attach lifecycle hooks (parity spec 22).
    pub fn with_hooks(mut self, hooks: agent_core::HookRegistry) -> Self {
        self.hooks = hooks;
        self
    }

    /// Attach the composed search backend (so `--serve-search` can host it).
    pub fn with_search(mut self, search: Arc<dyn agent_core::SearchBackend>) -> Self {
        self.search = Some(search);
        self
    }

    /// Attach the composed code-graph backend (so `--serve-ast` can host it).
    pub fn with_ast(mut self, ast: Arc<dyn agent_core::AstBackend>) -> Self {
        self.ast = Some(ast);
        self
    }

    /// Attach the structured-output validator (parity spec 16).
    #[cfg(feature = "structured")]
    pub fn with_validator(mut self, validator: Arc<dyn agent_core::OutputSchema>) -> Self {
        self.validator = Some(validator);
        self
    }

    /// Run a schema-constrained completion with a bounded one-shot repair loop:
    /// attach `schema`, validate the model's JSON, and repair up to `max_repairs`
    /// times before erroring. Steers natively when the provider supports it, else
    /// injects the schema into the prompt. See `docs/components/structured-output.md`.
    #[cfg(feature = "structured")]
    pub async fn complete_structured(
        &self,
        request: agent_core::CompletionRequest,
        schema: &serde_json::Value,
        max_repairs: usize,
    ) -> anyhow::Result<serde_json::Value> {
        let validator = self
            .validator
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("structured output is not configured"))?;
        Ok(crate::structured::complete_structured(
            self.provider.as_ref(),
            validator.as_ref(),
            request,
            schema,
            max_repairs,
            &self.metrics,
        )
        .await?)
    }

    /// Attach the session-history store (parity spec 19).
    #[cfg(feature = "session")]
    pub fn with_session_store(mut self, store: Arc<dyn agent_core::SessionStore>) -> Self {
        self.session_store = Some(store);
        self
    }

    /// The session-history store, if wired (for `agent --serve-session`).
    #[cfg(feature = "session")]
    pub fn session_store(&self) -> Option<Arc<dyn agent_core::SessionStore>> {
        self.session_store.clone()
    }

    /// Checkpoint `ws` as an immutable, content-addressed entry on `session`'s
    /// current branch. See `docs/components/session.md`.
    #[cfg(feature = "session")]
    pub async fn checkpoint(
        &self,
        session: &str,
        ws: &WorkingSet,
        label: &str,
    ) -> anyhow::Result<agent_core::CheckpointId> {
        let store = self
            .session_store
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("session history is not configured"))?;
        Ok(store.checkpoint(session, ws, label).await?)
    }

    /// Rehydrate the working set stored at a checkpoint id.
    #[cfg(feature = "session")]
    pub async fn restore_checkpoint(
        &self,
        id: &agent_core::CheckpointId,
    ) -> anyhow::Result<WorkingSet> {
        let store = self
            .session_store
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("session history is not configured"))?;
        Ok(store.restore(id).await?)
    }

    /// The branch tree for `session` (every checkpoint reachable from a head).
    #[cfg(feature = "session")]
    pub async fn list_checkpoints(
        &self,
        session: &str,
    ) -> anyhow::Result<Vec<agent_core::CheckpointMeta>> {
        let store = self
            .session_store
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("session history is not configured"))?;
        Ok(store.list(session).await?)
    }

    /// Attach the `@`-reference resolver + its token budget (parity spec 17).
    #[cfg(feature = "reference")]
    pub fn with_reference_resolver(
        mut self,
        resolver: Arc<dyn agent_core::ReferenceResolver>,
        budget_tokens: usize,
    ) -> Self {
        self.reference = Some(resolver);
        self.reference_budget = budget_tokens;
        self
    }

    /// The `@`-reference resolver, if wired (for `agent --serve-reference`).
    #[cfg(feature = "reference")]
    pub fn reference_resolver(&self) -> Option<Arc<dyn agent_core::ReferenceResolver>> {
        self.reference.clone()
    }

    /// Expand a prompt's `@file`/`@dir`/`@symbol`/`@url` mentions into context
    /// blocks, using the configured token budget. Returns an empty resolution when
    /// no resolver is wired, so callers can fold this in unconditionally. See
    /// `docs/components/reference.md`.
    #[cfg(feature = "reference")]
    pub async fn resolve_references(&self, prompt: &str) -> agent_core::Resolution {
        match &self.reference {
            Some(r) => r.resolve(prompt, self.reference_budget).await,
            None => agent_core::Resolution {
                blocks: vec![],
                warnings: vec![],
                blocked: false,
            },
        }
    }

    /// Attach the git backend (so `--serve-git` can host it).
    pub fn with_repo(mut self, repo: Arc<dyn agent_core::RepoBackend>) -> Self {
        self.repo = Some(repo);
        self
    }

    /// Attach the health-checked LLM pool.
    pub fn with_llm_pool(mut self, pool: Arc<dyn agent_core::LlmPool>) -> Self {
        self.llm_pool = Some(pool);
        self
    }

    /// Attach the task-mode classifier.
    pub fn with_task_classifier(mut self, c: Arc<dyn agent_core::TaskClassifier>) -> Self {
        self.task_classifier = Some(c);
        self
    }

    /// Attach the prompt-management store (docs/design/portal), so it can be hosted
    /// over gRPC. Not consumed by the loop.
    pub fn with_provider_registry(mut self, r: Arc<dyn agent_core::ProviderRegistry>) -> Self {
        self.provider_registry = Some(r);
        self
    }

    pub fn with_prompt_store(mut self, p: Arc<dyn agent_core::PromptStore>) -> Self {
        self.prompt_store = Some(p);
        self
    }

    /// Attach the situational system-fragment resolver (docs/design/prompts/). The
    /// loop consumes this: it selects the fragments matching the current mode and
    /// injects them as a system message. A no-op resolver (the default) changes
    /// nothing.
    pub fn with_system_fragments(
        mut self,
        sf: agent_context::system_fragments::SystemFragments,
    ) -> Self {
        self.system_fragments = sf;
        self
    }

    /// Attach the metrics proxy (docs/design/portal), so it can be hosted over gRPC.
    /// Not consumed by the loop.
    pub fn with_metrics_proxy(mut self, m: Arc<dyn agent_core::MetricsProxy>) -> Self {
        self.metrics_proxy = Some(m);
        self
    }

    /// Attach the dimensional-memory store (adaptive-cognition 03).
    pub fn with_dimension_store(mut self, d: Arc<dyn agent_core::DimensionStore>) -> Self {
        self.dimension_store = Some(d);
        self
    }

    /// Attach the review fact collector.
    pub fn with_review_collector(mut self, r: Arc<dyn agent_core::ReviewCollector>) -> Self {
        self.review_collector = Some(r);
        self
    }

    /// Run a single goal to completion (one-shot): open a session and send it.
    pub async fn run(self: &Arc<Self>, goal: &str) -> anyhow::Result<String> {
        self.session().send(goal).await
    }

    /// Open a multi-turn session whose working set persists across `send` calls.
    /// The built seams, for hosting one over gRPC (`agent --serve-<seam>`). These
    /// expose the same `Arc`/registry the loop uses, so a serve process reuses the
    /// config-selected impl (e.g. a real `anthropic` provider behind a gateway).
    pub fn provider(&self) -> Arc<dyn LlmProvider> {
        self.provider.clone()
    }
    pub fn memory(&self) -> Arc<dyn MemoryStore> {
        self.memory.clone()
    }
    /// The metrics registry the loop and seams record into — the same instance the
    /// `/metrics` endpoint serves, so a serve process can wire cross-cutting counters
    /// (e.g. the gRPC admission-layer shed count) into the scraped output.
    pub fn metrics(&self) -> Metrics {
        self.metrics.clone()
    }
    pub fn context(&self) -> Arc<dyn ContextStrategy> {
        self.context.clone()
    }
    pub fn policy(&self) -> Arc<dyn Policy> {
        self.policy.clone()
    }
    pub fn tools(&self) -> ToolRegistry {
        self.tools.clone()
    }
    /// The composed search backend, if wired (for `agent --serve-search`).
    pub fn search(&self) -> Option<Arc<dyn agent_core::SearchBackend>> {
        self.search.clone()
    }
    /// The composed code-graph backend, if wired (for `agent --serve-ast`).
    pub fn ast(&self) -> Option<Arc<dyn agent_core::AstBackend>> {
        self.ast.clone()
    }
    /// The git backend, if wired (for `agent --serve-git`).
    /// The content scanner, if wired (for `agent --serve-scanner`).
    pub fn scanner(&self) -> Option<Arc<dyn agent_core::Scanner>> {
        self.scanner.clone()
    }

    /// The tokenizer, if wired (for `agent --serve-tokenizer`).
    pub fn tokenizer(&self) -> Option<Arc<dyn agent_core::Tokenizer>> {
        self.tokenizer.clone()
    }

    /// Attach the tokenizer so it can be served.
    pub fn with_tokenizer_seam(mut self, t: Option<Arc<dyn agent_core::Tokenizer>>) -> Self {
        self.tokenizer = t;
        self
    }

    /// The web transport, if wired (for `agent --serve-web`).
    pub fn web(&self) -> Option<Arc<dyn agent_core::WebBackend>> {
        self.web.clone()
    }

    /// Attach the web transport so it can be served.
    pub fn with_web(mut self, w: Option<Arc<dyn agent_core::WebBackend>>) -> Self {
        self.web = w;
        self
    }

    /// The composed web-search dispatch, if wired (for `--serve-web-search`).
    pub fn web_search(&self) -> Option<Arc<dyn agent_core::WebSearch>> {
        self.web_search.clone()
    }

    /// Attach the web-search dispatch so it can be served.
    pub fn with_web_search(mut self, w: Option<Arc<dyn agent_core::WebSearch>>) -> Self {
        self.web_search = w;
        self
    }

    /// The sandbox, if wired (for `agent --serve-sandbox`).
    pub fn sandbox(&self) -> Option<Arc<dyn agent_core::Sandbox>> {
        self.sandbox.clone()
    }

    /// Attach the sandbox so it can be served.
    pub fn with_sandbox(mut self, s: Option<Arc<dyn agent_core::Sandbox>>) -> Self {
        self.sandbox = s;
        self
    }

    /// The pty backend, if wired (for `agent --serve-pty`).
    pub fn pty(&self) -> Option<Arc<dyn agent_core::Pty>> {
        self.pty.clone()
    }

    /// Attach the pty backend so it can be served.
    pub fn with_pty(mut self, p: Option<Arc<dyn agent_core::Pty>>) -> Self {
        self.pty = p;
        self
    }

    /// The forge backend, if wired (for `agent --serve-forge`).
    pub fn forge(&self) -> Option<Arc<dyn agent_core::Forge>> {
        self.forge.clone()
    }

    /// Attach the forge backend so it can be served.
    pub fn with_forge(mut self, f: Option<Arc<dyn agent_core::Forge>>) -> Self {
        self.forge = f;
        self
    }

    /// The task tracker, if wired (for `agent --serve-tasks`).
    pub fn tasks(&self) -> Option<Arc<dyn agent_core::TaskTracker>> {
        self.tasks.clone()
    }

    /// Attach the task tracker so it can be served.
    pub fn with_tasks(mut self, t: Option<Arc<dyn agent_core::TaskTracker>>) -> Self {
        self.tasks = t;
        self
    }

    /// The LSP backend, if wired (for `agent --serve-lsp`).
    pub fn lsp(&self) -> Option<Arc<dyn agent_core::LspBackend>> {
        self.lsp.clone()
    }

    /// Attach the LSP backend so it can be served.
    pub fn with_lsp(mut self, l: Option<Arc<dyn agent_core::LspBackend>>) -> Self {
        self.lsp = l;
        self
    }

    /// The episodic layer, when the memory is layered (for `--serve-episodic`).
    pub fn episodic(&self) -> Option<Arc<dyn agent_core::EpisodicStore>> {
        self.episodic.clone()
    }

    /// The semantic layer, when the memory is layered (for `--serve-semantic`).
    pub fn semantic(&self) -> Option<Arc<dyn agent_core::SemanticStore>> {
        self.semantic.clone()
    }

    /// Attach the composed memory layers so each can be served individually.
    pub fn with_memory_layers(
        mut self,
        episodic: Option<Arc<dyn agent_core::EpisodicStore>>,
        semantic: Option<Arc<dyn agent_core::SemanticStore>>,
    ) -> Self {
        self.episodic = episodic;
        self.semantic = semantic;
        self
    }

    /// The embedder, if wired (for `agent --serve-embed`).
    pub fn embedder(&self) -> Option<Arc<dyn agent_core::Embedder>> {
        self.embedder.clone()
    }

    /// Attach the embedder so it can be served.
    pub fn with_embedder(mut self, e: Option<Arc<dyn agent_core::Embedder>>) -> Self {
        self.embedder = e;
        self
    }

    /// Attach the composed content scanner (parity spec 18).
    pub fn with_scanner(mut self, s: Arc<dyn agent_core::Scanner>) -> Self {
        self.scanner = Some(s);
        self
    }

    /// Attach a tool-call verifier (the `verifier` seam). `enforce = false` runs
    /// it in shadow (verdict logged/counted, behaviour unchanged); `true` blocks a
    /// `Revise`/`Deny`'d call and feeds its message back to the model.
    pub fn with_verifier(mut self, v: Arc<dyn agent_core::Verifier>, enforce: bool) -> Self {
        self.verifier = Some(v);
        self.verifier_enforce = enforce;
        self
    }

    pub fn repo(&self) -> Option<Arc<dyn agent_core::RepoBackend>> {
        self.repo.clone()
    }

    pub fn llm_pool(&self) -> Option<Arc<dyn agent_core::LlmPool>> {
        self.llm_pool.clone()
    }

    pub fn task_classifier(&self) -> Option<Arc<dyn agent_core::TaskClassifier>> {
        self.task_classifier.clone()
    }

    pub fn dimension_store(&self) -> Option<Arc<dyn agent_core::DimensionStore>> {
        self.dimension_store.clone()
    }

    /// The digest ledger, if `[digest] store` is configured (`--serve-digest`).
    pub fn digest_store(&self) -> Option<Arc<dyn agent_core::DigestStore>> {
        self.digests.clone()
    }

    /// The cognition-graph document store, if `[graph] store` is configured
    /// (`--serve-graph`).
    pub fn graph_store(&self) -> Option<Arc<dyn agent_core::GraphStore>> {
        self.graph.clone()
    }

    pub fn prompt_store(&self) -> Option<Arc<dyn agent_core::PromptStore>> {
        self.prompt_store.clone()
    }

    /// The provider-registry store, if `[registry] store` is configured
    /// (`--serve-provider-registry`).
    pub fn provider_registry(&self) -> Option<Arc<dyn agent_core::ProviderRegistry>> {
        self.provider_registry.clone()
    }

    pub fn metrics_proxy(&self) -> Option<Arc<dyn agent_core::MetricsProxy>> {
        self.metrics_proxy.clone()
    }

    /// The overload admission cap for served seams (`[grpc] max_in_flight`); `0` =
    /// unbounded. Read by the serve path when building the router.
    pub fn grpc_max_in_flight(&self) -> usize {
        self.settings.grpc_max_in_flight
    }

    /// The registry a served `AgentSessionService` reads (docs/design/portal +
    /// multi-session/03-hazards.md). Always available; a serve-only process simply
    /// resolves no session until a loop registers one. The service selects by
    /// `session_id` (empty ⇒ the sole live session, for the single-session observer).
    pub fn session_source_registry(&self) -> Arc<dyn agent_core::SessionSourceRegistry> {
        self.events.clone()
    }

    pub fn review_collector(&self) -> Option<Arc<dyn agent_core::ReviewCollector>> {
        self.review_collector.clone()
    }

    /// Collect grounded review facts for the working tree and return them rendered
    /// as a context block. Called by the loop when it has *entered* review mode
    /// (the mode decision is made once, generally — see `Session::detect_mode`).
    /// `None` when the collector isn't wired or collection fails (best-effort).
    #[cfg(feature = "review")]
    async fn review_collect(&self) -> Option<String> {
        let collector = self.review_collector.as_ref()?;
        match collector
            .collect(&agent_core::ReviewTarget::WorkingTree)
            .await
        {
            Ok(facts) => {
                tracing::info!("entering review mode: injecting grounded facts");
                // Record the run — triggered by in-loop detection (`auto`).
                self.record_review(agent_core::ReviewRecord::from_facts(&facts, "auto"))
                    .await;
                Some(agent_review::render_facts_with(
                    &facts,
                    self.settings.review_context_budget,
                ))
            }
            Err(e) => {
                tracing::warn!("review fact collection failed: {e}");
                None
            }
        }
    }

    /// Best-effort removal of this session's disposable worktrees, so an aborted or
    /// finished run doesn't leave them orphaned on disk. Call it on every exit path
    /// (success, error, Ctrl-C). `worktree_list` is scoped to this session's run
    /// directory, so it never disturbs a concurrent session. Logs failures; the
    /// method itself never errors (cleanup must not mask the real outcome).
    pub async fn cleanup(&self) {
        let Some(repo) = &self.repo else { return };
        let worktrees = match repo.worktree_list().await {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(error = %e, "worktree list failed during cleanup");
                return;
            }
        };
        for wt in worktrees {
            if let Err(e) = repo.worktree_remove(&wt.id).await {
                tracing::warn!(id = %wt.id, error = %e, "worktree cleanup failed");
            }
        }
    }

    /// Open a session under the process's default identity (the local user + this
    /// run's session id). The single-session CLI/REPL entry point.
    pub fn session(self: &Arc<Self>) -> Session {
        self.session_with(self.default_session_key())
    }

    /// Open a session under an explicit `(user, session)` identity — the multi-tenant
    /// entry point used by the [`SessionManager`] / portal, so many sessions run over
    /// one shared backend (docs/design/multi-session/02-runtime-split.md).
    pub fn session_with(self: &Arc<Self>, id: agent_core::SessionKey) -> Session {
        // This session's own event sink (created on first reference), so its
        // `Subscribe`rs never see a concurrent session's events (hazard B).
        let events = self.events.get_or_create(id.session.as_str());
        // Per-tenant metrics recorder: the curated loop-level families carry this
        // session's `(session, user)` label (docs/design/multi-session/06-observability.md).
        let session_metrics = self
            .metrics
            .for_session(id.session.as_str(), id.user.as_str());
        Session {
            agent: self.clone(),
            id,
            events,
            session_metrics,
            working: WorkingSet::default(),
            budget: TokenBudget {
                max_context_tokens: self.settings.context_window,
                reserve_output: self.settings.reserve_output,
            },
            tool_ctx: ToolContext {
                cwd: self.settings.cwd.clone(),
            },
            tool_schemas: self.tools.describe_all(),
            started: false,
            pending_context: Vec::new(),
            current_mode: agent_core::TaskMode::default(),
            switch_history: std::collections::VecDeque::new(),
            pending_switch: None,
            situational_present: false,
            agreed_seq: 0,
            distiller: None,
        }
    }

    /// The process default identity: the local user + this run's session id (or
    /// `local` when telemetry is off and no id was minted).
    fn default_session_key(&self) -> agent_core::SessionKey {
        let sess = if self.settings.session_id.is_empty() {
            agent_core::UserId::LOCAL.to_string()
        } else {
            self.settings.session_id.clone()
        };
        agent_core::SessionKey::local(sess)
    }

    /// The configured model name (for display, e.g. a `/model` command).
    pub fn model(&self) -> &str {
        &self.settings.model
    }

    /// Registered tool names, sorted.
    pub fn tool_names(&self) -> Vec<String> {
        self.tools
            .describe_all()
            .into_iter()
            .map(|s| s.name)
            .collect()
    }

    /// The core iteration loop over an existing working set: model call → tool
    /// dispatch → record → compact, until the model stops asking for tools (or
    /// `max_iterations`). Mutates `working` in place and returns the final answer.
    // A core private loop with several genuinely-distinct per-turn params (state,
    // budget, tool ctx, the armed switch, and the two per-session observation sinks
    // `events`/`metrics`); bundling them only to satisfy the arg-count lint would
    // obscure more than it helps.
    #[allow(clippy::too_many_arguments)]
    async fn run_loop(
        &self,
        working: &mut WorkingSet,
        budget: &TokenBudget,
        tool_ctx: &ToolContext,
        tool_schemas: &[ToolSchema],
        pending_switch: &mut Option<(agent_core::TaskMode, agent_core::TaskMode)>,
        mode: agent_core::TaskMode,
        events: &crate::SessionEvents,
        metrics: &SessionMetrics,
    ) -> anyhow::Result<String> {
        let model = self.settings.model.as_str();
        // Back-to-back truncated completions (finish_reason = output-cap) seen so
        // far; any productive turn clears it. Bounds the continue-on-truncation
        // recovery so a perpetually-truncating model fails fast, not at the
        // max_iterations ceiling.
        let mut consecutive_truncations = 0u32;
        for iter in 1..=self.settings.max_iterations {
            metrics.on_iteration();
            events.publish(agent_core::SessionEvent::IterationStart { iter: iter as u32 });
            if !self.hooks.is_empty() {
                self.hooks.pre_turn(working).await;
            }
            // Capability gate: a model without vision must never be sent an image
            // block — one unsupported block errors the entire request, losing the
            // turn. Degrade to an explicit note instead (parity spec 26).
            let mut messages = working.messages.clone();
            if !self.provider.capabilities().supports_vision {
                let mut dropped = 0usize;
                for m in &mut messages {
                    dropped += m
                        .strip_media("[media omitted: the selected model does not support images]");
                }
                if dropped > 0 {
                    self.metrics.on_content_blocks_dropped(dropped as u64);
                    tracing::debug!(dropped, "stripped media for a non-vision model");
                }
            }
            for m in &messages {
                for b in &m.content {
                    self.metrics.on_content_block(b.modality());
                }
            }
            let req = CompletionRequest {
                messages,
                tools: tool_schemas.to_vec(),
                max_tokens: self.settings.max_tokens,
                temperature: self.settings.temperature,
                // The main loop uses free-text completions; structured output is a
                // separate helper path (parity spec 16).
                response_format: None,
                // The per-request routing signals (model-router 02b): the turn's
                // classified task mode + the Main role. Ignored by non-routing
                // providers; a `task-router` provider routes on them.
                route: Some(agent_core::RouteHint {
                    task_mode: Some(mode),
                    role: Some(agent_core::RouteRole::Main),
                    ..Default::default()
                }),
            };

            let call_start = Instant::now();
            let msg_count = working.messages.len();
            // `stream=true` uses the provider's incremental stream (with a live
            // echo); `stream=false` is the buffered path (an escape hatch for
            // servers that misbehave on SSE).
            let resp = if self.settings.stream {
                self.complete_streaming(req, events)
                    .instrument(tracing::info_span!("provider.stream", iter, model))
                    .await?
            } else {
                self.provider
                    .complete(req)
                    .instrument(tracing::info_span!("provider.complete", iter, model))
                    .await?
            };
            metrics.on_api_call(
                model,
                &resp.finish_reason,
                call_start.elapsed().as_secs_f64(),
            );
            let assistant = resp.message.clone();
            working.messages.push(assistant.clone());
            self.record("assistant", assistant.clone()).await;
            if !self.hooks.is_empty() {
                self.hooks.post_turn(&assistant).await;
            }

            // The buffered path (`stream=false`) emits no incremental TokenDeltas,
            // so a live subscriber (the portal Agent view) would see the run
            // scaffolding but never the answer. Publish the assistant's text as a
            // single delta so it renders, mirroring the streaming echo. (The
            // streaming path already published its deltas as content arrived.)
            // Reasoning models keep their chain-of-thought in a separate field the
            // provider drops, so only the answer is surfaced.
            if !self.settings.stream && events.has_subscribers() {
                let text = assistant.content_text();
                if !text.is_empty() {
                    events.publish(agent_core::SessionEvent::TokenDelta { text });
                }
            }

            if let Some(u) = &resp.usage {
                tracing::info!(
                    iter,
                    finish = %resp.finish_reason,
                    tool_calls = assistant.tool_calls.len(),
                    prompt_tokens = u.prompt_tokens,
                    completion_tokens = u.completion_tokens,
                    "model turn"
                );
                metrics.add_tokens(model, u.prompt_tokens as u64, u.completion_tokens as u64);
                metrics.set_context(u.prompt_tokens as i64, msg_count as i64);
                events.publish(agent_core::SessionEvent::ContextUpdate {
                    prompt_tokens: u.prompt_tokens,
                    context_window: self.settings.context_window,
                    messages: msg_count as u32,
                });
                // Prompt-cache token split (Anthropic/OpenAI report these) + USD cost
                // from the price table — the accounting half of the tokenizer seam.
                metrics.add_cache_tokens(
                    model,
                    u.cache_read_tokens as u64,
                    u.cache_write_tokens as u64,
                );
                #[cfg(feature = "tokenizer")]
                {
                    let prices = agent_tokenizer::PriceTable::builtin();
                    let (cost, _status) = agent_core::calculate_cost(model, u, &prices);
                    metrics.add_cost(
                        model,
                        cost.input,
                        cost.output,
                        cost.cache_read,
                        cost.cache_write,
                    );
                }
                self.record_usage(iter as u32, u).await;
            }

            // A completion with no tool calls is the final answer — UNLESS the
            // provider truncated it at the output-token cap (finish_reason =
            // length/max_tokens). A truncated turn was cut off mid-emission, often
            // mid tool call (e.g. a large file write), so it parses to zero tool
            // calls; returning it would score a cut-off fragment as a successful
            // answer and silently drop the pending action. Instead, nudge the model
            // to finish and continue the loop. A model that keeps truncating fails
            // after MAX_CONSECUTIVE_TRUNCATIONS so the caller records a DNF rather
            // than a fake answer.
            if assistant.tool_calls.is_empty() {
                if is_truncated_finish(&resp.finish_reason) {
                    consecutive_truncations += 1;
                    if consecutive_truncations > MAX_CONSECUTIVE_TRUNCATIONS {
                        anyhow::bail!(
                            "model truncated {consecutive_truncations} responses in a row at the \
                             output-token limit without completing a tool call or a final answer"
                        );
                    }
                    tracing::warn!(
                        iter,
                        consecutive_truncations,
                        finish = %resp.finish_reason,
                        "completion truncated at max_tokens with no tool call — nudging to continue \
                         (a truncated response is not a final answer)"
                    );
                    working.messages.push(Message::user(TRUNCATION_NUDGE));
                    continue;
                }
                self.memory.distill().await.ok();
                return Ok(assistant.content_text());
            }
            // A productive (tool-call) turn clears the truncation streak.
            consecutive_truncations = 0;

            // Dispatch the requested tool calls. Authorization runs sequentially
            // (interactive prompts must not interleave); execution runs
            // concurrently when enabled and every requested tool is parallel-safe.
            // Results are appended in original call order for a deterministic
            // transcript.
            let mut decisions = Vec::with_capacity(assistant.tool_calls.len());
            for call in &assistant.tool_calls {
                // Record the outcome (and deny reason) onto the span *from inside*
                // the instrumented future, so the fields land while the span is
                // still open — an allow/deny audit trail in the trace tree.
                let span = tracing::info_span!(
                    "policy.authorize",
                    iter,
                    tool = %call.name,
                    decision = tracing::field::Empty,
                    reason = tracing::field::Empty,
                );
                let dec = async {
                    let d = self.policy.authorize(call).await;
                    let s = tracing::Span::current();
                    match &d {
                        Decision::Allow => s.record("decision", "allow"),
                        Decision::Deny(r) => {
                            s.record("decision", "deny");
                            s.record("reason", r.as_str())
                        }
                    };
                    d
                }
                .instrument(span)
                .await;
                // A `pre_tool` hook can veto a call the policy allowed — the
                // interventional point. It runs AFTER the policy so a hook can
                // only ever narrow permission, never widen it.
                let dec = match dec {
                    Decision::Allow if !self.hooks.is_empty() => {
                        match self.hooks.pre_tool(call).await {
                            agent_core::HookOutcome::Continue => Decision::Allow,
                            agent_core::HookOutcome::Deny(reason) => {
                                tracing::info!(tool = %call.name, %reason, "denied by hook");
                                Decision::Deny(reason)
                            }
                        }
                    }
                    other => other,
                };
                decisions.push(dec);
            }

            // Tool-call verification: judge each allowed call. In SHADOW the
            // verdict is only observed (logged + counted); in ENFORCE a Revise/Deny
            // blocks the call and its message is fed back to the model so it can
            // reissue a corrected one. `verifier_feedback[i]` is Some(message) for a
            // call blocked in enforce mode. See docs/design/tool-call-verification.md.
            let mut verifier_feedback: Vec<Option<String>> = vec![None; assistant.tool_calls.len()];
            // Parallel to `verifier_feedback`: the record for each verified call,
            // its `call_errored` outcome filled in after the tool runs (below),
            // then written to `agent_verifications`.
            let mut verifications: Vec<Option<agent_core::VerificationRecord>> =
                vec![None; assistant.tool_calls.len()];
            if let Some(verifier) = &self.verifier {
                let mode = if self.verifier_enforce {
                    "enforce"
                } else {
                    "shadow"
                };
                let verifier_cfg =
                    format!("{{\"name\":\"{}\",\"mode\":\"{}\"}}", verifier.name(), mode);
                // Best-effort goal: the first user message. `run_loop` does not
                // carry the goal explicitly, and the only shipped verifier
                // (schema) does not use it — so this avoids a signature change.
                let goal = working
                    .messages
                    .iter()
                    .find(|m| matches!(m.role, agent_core::Role::User))
                    .map(agent_core::Message::content_text)
                    .unwrap_or_default();
                let goal_hash = agent_core::fnv1a_hex(goal.as_bytes());
                for (i, (call, dec)) in assistant.tool_calls.iter().zip(&decisions).enumerate() {
                    if !matches!(dec, Decision::Allow) {
                        continue; // only verify the calls that would actually run
                    }
                    // A `verifier` span with recorded fields, mirroring
                    // `policy.authorize` — the verdict lands on the span while it is
                    // open, giving an audit trail in the trace and a testable field.
                    let span = tracing::info_span!(
                        "verifier",
                        iter,
                        tool = %call.name,
                        verifier = tracing::field::Empty,
                        verdict = tracing::field::Empty,
                    );
                    let ctx = agent_core::VerifyCtx {
                        call,
                        goal: &goal,
                        history: &working.messages,
                        tool_schema: tool_schemas.iter().find(|s| s.name == call.name),
                    };
                    let started = Instant::now();
                    let report = async {
                        let r = verifier.verify(&ctx).await;
                        let s = tracing::Span::current();
                        s.record("verifier", r.model.as_str());
                        s.record("verdict", verdict_label(&r.verdict));
                        r
                    }
                    .instrument(span)
                    .await;
                    let latency_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;

                    self.metrics
                        .on_verifier(&report.model, verdict_label(&report.verdict), mode);

                    // Capture the analytics row now (verdict known); `call_errored`
                    // is filled after the tool runs, in the result loop below.
                    verifications[i] = Some(agent_core::VerificationRecord {
                        tool_name: call.name.clone(),
                        args_hash: agent_core::fnv1a_hex(
                            serde_json::to_string(&call.arguments)
                                .unwrap_or_default()
                                .as_bytes(),
                        ),
                        goal_hash: goal_hash.clone(),
                        // Coarse phase-1 task_type: the tool name. A real taxonomy
                        // is a phase-2 open question (see the design doc).
                        task_type: call.name.clone(),
                        verifier_model: report.model.clone(),
                        verifier_cfg: verifier_cfg.clone(),
                        verdict: verdict_label(&report.verdict).to_string(),
                        confidence: report.clamped_confidence(),
                        latency_ms,
                        cached: false,
                        call_errored: None,
                        revised_after: None,
                        task_succeeded: None,
                    });

                    match &report.verdict {
                        agent_core::VerifyVerdict::Allow => {}
                        agent_core::VerifyVerdict::Revise(h) => {
                            if self.verifier_enforce {
                                verifier_feedback[i] = Some(format!(
                                    "the `{}` call was not run — the verifier asks you to \
                                     revise it: {h}",
                                    call.name
                                ));
                            } else {
                                tracing::info!(
                                    confidence = report.clamped_confidence() as f64,
                                    hint = %h,
                                    "verifier (shadow) would ask for a revision"
                                );
                            }
                        }
                        agent_core::VerifyVerdict::Deny(r) => {
                            if self.verifier_enforce {
                                verifier_feedback[i] = Some(format!(
                                    "the `{}` call was blocked by the verifier: {r}",
                                    call.name
                                ));
                            } else {
                                tracing::info!(
                                    confidence = report.clamped_confidence() as f64,
                                    reason = %r,
                                    "verifier (shadow) would deny"
                                );
                            }
                        }
                    }
                }
            }

            let parallel = self.settings.parallel_tools
                && assistant
                    .tool_calls
                    .iter()
                    .all(|c| self.tools.get(&c.name).is_none_or(|t| t.parallel_safe()));

            // Publish tool-call starts to any live observer before execution
            // (docs/design/portal). Guarded so an unobserved run does not stringify
            // the args. Sequential + pre-execution, so ordering is deterministic even
            // though the calls themselves may run in parallel below.
            if events.has_subscribers() {
                for call in &assistant.tool_calls {
                    events.publish(agent_core::SessionEvent::ToolCallStart {
                        name: call.name.clone(),
                        args: call.arguments.to_string(),
                    });
                }
            }

            let tool_timeout = self.settings.tool_timeout_secs;
            let futures = assistant
                .tool_calls
                .iter()
                .zip(&decisions)
                .zip(&verifier_feedback)
                .map(|((call, dec), vfb)| {
                    let tools = &self.tools;
                    let cwd = tool_ctx.cwd.clone();
                    // A call the verifier blocked (enforce mode) does not run — its
                    // feedback message is produced in the result loop below.
                    let blocked = vfb.is_some();
                    let span = tracing::info_span!("tool.execute", iter, tool = %call.name);
                    async move {
                        if blocked {
                            return None;
                        }
                        match dec {
                            Decision::Deny(_) => None,
                            Decision::Allow => Some(match tools.get(&call.name) {
                                // Guarded: a hung tool times out and a panicking tool
                                // is isolated — either way an error observation, so
                                // one bad tool never freezes or crashes the loop.
                                Some(tool) => {
                                    run_tool_guarded(
                                        tool,
                                        call.arguments.clone(),
                                        cwd,
                                        tool_timeout,
                                    )
                                    .await
                                }
                                None => Observation::error(format!("unknown tool `{}`", call.name)),
                            }),
                        }
                    }
                    .instrument(span)
                });

            let mut observations: Vec<Option<Observation>> = if parallel {
                futures_util::future::join_all(futures).await
            } else {
                let mut v = Vec::with_capacity(assistant.tool_calls.len());
                for f in futures {
                    v.push(f.await);
                }
                v
            };

            for (i, call) in assistant.tool_calls.iter().enumerate() {
                // A verifier-blocked call (enforce mode) never executed, so it has
                // no observation — handle it before the policy/observation match to
                // avoid the `expect` below. Its feedback message flows through the
                // same push + record path as any tool result.
                // The verification outcome proxy: `Some(is_error)` for a call that
                // actually ran, `None` for one blocked (by the verifier or policy)
                // and so never executed.
                let mut call_errored: Option<bool> = None;
                let msg = if let Some(feedback) = &verifier_feedback[i] {
                    metrics.on_tool(&call.name, "verifier_blocked");
                    Message::tool(&call.id, feedback.clone())
                } else {
                    match &decisions[i] {
                        Decision::Deny(reason) => {
                            metrics.on_tool(&call.name, "denied");
                            Message::tool(&call.id, format!("denied by policy: {reason}"))
                        }
                        Decision::Allow => {
                            let observation = observations[i]
                                .take()
                                .expect("allowed tool call has an observation");
                            call_errored = Some(observation.is_error);
                            metrics.on_tool(
                                &call.name,
                                if observation.is_error { "error" } else { "ok" },
                            );
                            tracing::info!(
                                tool = %call.name,
                                is_error = observation.is_error,
                                // Total payload, not just the text: a tool returning
                                // an image would otherwise look ~free in telemetry.
                                bytes = observation_bytes(&observation),
                                media = observation.blocks.len(),
                                "tool result"
                            );
                            if !self.hooks.is_empty() {
                                self.hooks.post_tool(call, &observation).await;
                            }
                            // Move the blocks through rather than flattening to text,
                            // so a tool that produced an image (a screenshot, a PNG
                            // read off disk) reaches the next request intact.
                            Message::tool_with_blocks(&call.id, observation.into_blocks())
                        }
                    }
                };
                // Publish the settled outcome to any live observer. `ok` is false
                // for a denied/blocked call (no observation) too. duration_ms is
                // best-effort 0 here — precise per-tool latency is in
                // `agent_tool_exec_seconds` (reachable via the MetricsProxy seam).
                if events.has_subscribers() {
                    let ok = call_errored.map(|e| !e).unwrap_or(false);
                    events.publish(agent_core::SessionEvent::ToolCallResult {
                        name: call.name.clone(),
                        ok,
                        duration_ms: 0,
                    });
                }
                working.messages.push(msg.clone());
                self.record("tool", msg).await;

                // Emit the verification record for this call, now that its outcome
                // proxy is known. Absent for a call the verifier never judged
                // (policy-denied). Best-effort telemetry, like `record`.
                if let Some(mut rec) = verifications[i].take() {
                    rec.call_errored = call_errored;
                    self.record_verification(iter as u32, rec).await;
                }
            }

            // Keep the working set within budget before the next turn.
            let before = working.messages.len();
            let tokens_before = agent_context::bench_estimate_tokens(&working.messages);
            // The armed switch (if any) is consumed on this first compact; later
            // iterations pass `None` (an ordinary budget compaction).
            self.context
                .compact(working, budget, pending_switch.take())
                .instrument(tracing::info_span!("context.compact", iter))
                .await?;
            if !self.hooks.is_empty() && before != working.messages.len() {
                self.hooks
                    .on_compact(&agent_core::CompactionInfo {
                        messages_before: before,
                        messages_after: working.messages.len(),
                        tokens_before,
                        tokens_after: agent_context::bench_estimate_tokens(&working.messages),
                    })
                    .await;
            }
        }

        self.memory.distill().await.ok();
        anyhow::bail!(
            "reached max_iterations ({}) without a final answer",
            self.settings.max_iterations
        )
    }

    /// Consume the provider's chunk stream into a single `CompletionResponse`,
    /// echoing assistant text to stderr as it arrives.
    async fn complete_streaming(
        &self,
        req: CompletionRequest,
        events: &crate::SessionEvents,
    ) -> anyhow::Result<CompletionResponse> {
        let mut stream = self.provider.stream(req).await?;
        let mut content = String::new();
        let mut tool_calls = Vec::new();
        let mut finish_reason = String::from("stop");
        let mut usage = None;
        let mut echoed = false;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if !chunk.delta_text.is_empty() {
                eprint!("{}", chunk.delta_text);
                let _ = std::io::stderr().flush();
                echoed = true;
                // Publish the delta to any live observer (docs/design/portal). Guard
                // on subscribers so the unobserved hot path allocates nothing extra.
                if events.has_subscribers() {
                    events.publish(agent_core::SessionEvent::TokenDelta {
                        text: chunk.delta_text.clone(),
                    });
                }
                content.push_str(&chunk.delta_text);
            }
            if let Some(tc) = chunk.tool_call {
                tool_calls.push(tc);
            }
            if let Some(fr) = chunk.finish_reason {
                finish_reason = fr;
            }
            if let Some(u) = chunk.usage {
                usage = Some(u);
            }
        }
        if echoed {
            eprintln!();
        }

        Ok(CompletionResponse {
            message: Message {
                role: Role::Assistant,
                // The streaming path accumulates text deltas only.
                content: if content.is_empty() {
                    Vec::new()
                } else {
                    vec![agent_core::ContentBlock::text(content)]
                },
                tool_calls,
                tool_call_id: None,
            },
            finish_reason,
            usage,
        })
    }

    async fn record(&self, kind: &str, message: Message) {
        self.append_event(MemoryEvent {
            kind: kind.to_string(),
            message,
            ts_ms: now_ms(),
            session_id: self.settings.session_id.clone(),
            usage: None,
            iter: None,
            verification: None,
            review: None,
            dimensional: None,
        })
        .await;
    }

    /// Record a per-turn token-usage event (routed to `agent_usage` by the sink).
    async fn record_usage(&self, iter: u32, usage: &agent_core::Usage) {
        self.append_event(MemoryEvent {
            kind: "usage".to_string(),
            message: Message::assistant(String::new()),
            ts_ms: now_ms(),
            session_id: self.settings.session_id.clone(),
            usage: Some(usage.clone()),
            iter: Some(iter),
            verification: None,
            review: None,
            dimensional: None,
        })
        .await;
    }

    /// Record one tool-call verification (routed to `agent_verifications` by the
    /// sink). Telemetry-only, like [`record_usage`](Self::record_usage): a
    /// dropped sink loses only the analytics row, never the loop.
    async fn record_verification(&self, iter: u32, rec: agent_core::VerificationRecord) {
        self.append_event(MemoryEvent {
            kind: "verification".to_string(),
            message: Message::assistant(String::new()),
            ts_ms: now_ms(),
            session_id: self.settings.session_id.clone(),
            usage: None,
            iter: Some(iter),
            verification: Some(rec),
            review: None,
            dimensional: None,
        })
        .await;
    }

    /// Record one code-review run (routed to `agent_reviews` by the sink) and fire
    /// the review-run metrics. Telemetry-only, like [`record_verification`]: a
    /// dropped sink loses only the analytics row, never the review. `mode_via` names
    /// how the review was triggered (`explicit` for `agent --review`, `auto` in-loop).
    pub async fn record_review(&self, rec: agent_core::ReviewRecord) {
        let outcome = if rec
            .collectors
            .iter()
            .any(|c| c.status == agent_core::CollectStatus::Failed)
        {
            "partial"
        } else {
            "ok"
        };
        self.metrics.on_review_run(
            &rec.project,
            &rec.mode_via,
            outcome,
            rec.total_ms as f64 / 1000.0,
        );
        if rec.total_ms > 0 {
            self.metrics
                .on_review_parallelism(rec.sum_work_ms as f64 / rec.total_ms as f64);
        }
        tracing::info!(
            changed_files = rec.changed_files,
            findings = rec.findings,
            total_ms = rec.total_ms,
            critical = %rec.critical_path,
            "review recorded"
        );
        self.append_event(MemoryEvent {
            kind: "review".to_string(),
            message: Message::assistant(String::new()),
            ts_ms: now_ms(),
            session_id: self.settings.session_id.clone(),
            usage: None,
            iter: None,
            verification: None,
            review: Some(rec),
            dimensional: None,
        })
        .await;
    }

    async fn append_event(&self, event: MemoryEvent) {
        if let Err(e) = self.memory.append(event).await {
            tracing::warn!("episodic append failed: {e}");
        }
    }
}

// Multi-session pool — see agent/session_manager.rs (re-exported below).
mod session_manager;
pub use session_manager::*;
// The per-conversation `Session` handle + its turn loop — see agent/session.rs
// (re-exported below so `agent_runtime::Session` is unchanged).
mod session;
pub use session::Session;

/// Decide whether a per-turn verdict switches the session mode, updating the
/// hysteresis `history` in place. Returns `Some(new_mode)` on a switch. Pure (no
/// I/O) so the switch policy is unit-tested directly. Rules: the candidate must
/// differ from `current` and clear `floor`; then a *decisive* verdict (confidence
/// ≥ 0.9, i.e. a deterministic prefilter hit) switches immediately, while a weaker
/// (vote) verdict needs the same candidate to win `hysteresis` consecutive turns.
fn decide_switch(
    current: agent_core::TaskMode,
    candidate: agent_core::TaskMode,
    confidence: f32,
    floor: f32,
    hysteresis: usize,
    history: &mut std::collections::VecDeque<agent_core::TaskMode>,
) -> Option<agent_core::TaskMode> {
    let hysteresis = hysteresis.max(1);
    // No change, or too weak to consider: reset the streak and stay.
    if candidate == current || confidence < floor {
        history.clear();
        return None;
    }
    // Track consecutive proposals of this candidate (bounded to `hysteresis`).
    history.push_back(candidate);
    while history.len() > hysteresis {
        history.pop_front();
    }
    let streak = history
        .iter()
        .rev()
        .take_while(|m| **m == candidate)
        .count();
    let decisive = confidence >= 0.9;
    if !decisive && streak < hysteresis {
        return None;
    }
    history.clear();
    Some(candidate)
}

/// How many recent working messages the per-turn dimension pass reviews.
const DIMENSION_WINDOW: usize = 16;
/// How many bullets a per-dimension recall pulls in on a switch.
const DIMENSION_RECALL_LIMIT: usize = 5;

/// Wrap the recent working-set tail as synthetic `MemoryEvent`s for the per-step
/// dimension pass (adaptive-cognition 03) — "what just happened" this turn.
fn recent_events(messages: &[Message], n: usize) -> Vec<MemoryEvent> {
    let start = messages.len().saturating_sub(n);
    messages[start..]
        .iter()
        .map(|m| MemoryEvent {
            kind: "step".to_string(),
            message: m.clone(),
            ts_ms: 0,
            session_id: String::new(),
            usage: None,
            iter: None,
            verification: None,
            review: None,
            dimensional: None,
        })
        .collect()
}

/// The dimensions worth recalling when *entering* a mode — the before/after
/// table's "pull in fresh" column (docs/design/adaptive-cognition/02-compaction.md
/// + 03-memory.md). `Other` pulls nothing.
fn recall_dims_for(mode: agent_core::TaskMode) -> &'static [&'static str] {
    use agent_core::TaskMode::*;
    match mode {
        Implement => &["coding", "testing"],
        Debug => &["coding", "testing"],
        Review => &["coding", "git", "project"],
        Design => &["project", "docs"],
        Explain => &["user"],
        Other => &[],
    }
}

/// Which classification stage produced a verdict, from its `reason` prefix — a
/// metric label (`prefilter` | `vote` | `failsafe`), never the raw prompt.
fn classify_via(reason: &str) -> &'static str {
    if reason.starts_with("deterministic") {
        "prefilter"
    } else if reason.starts_with("pool vote") {
        "vote"
    } else {
        "failsafe"
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The nudge appended after a truncated turn so the model resumes where it was
/// cut off (e.g. finishes a large file-write tool call) rather than restarting.
const TRUNCATION_NUDGE: &str = "Your previous message was cut off at the output-token \
limit before it was complete. Continue exactly where you left off and finish it — if you \
were emitting a tool call (for example writing a file), send the whole tool call this time.";

/// How many back-to-back truncated completions the loop will nudge through
/// before giving up. A truncated response is never a final answer, so we let
/// the model continue — but a model that truncates *every* turn must fail
/// honestly (the caller records a DNF) instead of spinning to `max_iterations`
/// burning tokens on a response that never lands.
const MAX_CONSECUTIVE_TRUNCATIONS: u32 = 3;

/// Whether a provider's `finish_reason` means the completion was truncated at
/// the output-token cap rather than finished. OpenAI-family servers report
/// `"length"`; Anthropic reports `"max_tokens"`. The value is provider-
/// controlled (untrusted), so match case-insensitively and treat anything else
/// as a normal stop — the fail-safe direction (a mislabelled stop is returned,
/// never silently looped).
fn is_truncated_finish(finish_reason: &str) -> bool {
    finish_reason.eq_ignore_ascii_case("length") || finish_reason.eq_ignore_ascii_case("max_tokens")
}

/// Total bytes an observation carries — its text plus any media payload. Used
/// for telemetry so an image-bearing result isn't reported as near-zero.
/// The metric/span label for a verifier verdict (a bounded enum, never free text).
fn verdict_label(v: &agent_core::VerifyVerdict) -> &'static str {
    match v {
        agent_core::VerifyVerdict::Allow => "allow",
        agent_core::VerifyVerdict::Revise(_) => "revise",
        agent_core::VerifyVerdict::Deny(_) => "deny",
    }
}

fn observation_bytes(o: &agent_core::Observation) -> usize {
    o.content.len()
        + o.blocks
            .iter()
            .map(|b| match b {
                agent_core::ContentBlock::Text { text } => text.len(),
                agent_core::ContentBlock::Image { data, .. }
                | agent_core::ContentBlock::Document { data, .. } => data.len(),
            })
            .sum::<usize>()
}

/// Run a tool with a wall-clock timeout **and** panic isolation, always returning
/// an [`Observation`] — a hung or panicking tool becomes an error observation fed
/// back to the model, so one bad tool never freezes or crashes the loop.
///
/// The tool runs on its own task, so a panic surfaces as a `JoinError` rather than
/// unwinding the loop's task / aborting the process. On timeout the task is aborted
/// so the hung work actually stops. `timeout_secs == 0` disables the timeout (e.g.
/// when `bash`'s own timeout is the intended bound).
async fn run_tool_guarded(
    tool: Arc<dyn Tool>,
    args: serde_json::Value,
    cwd: PathBuf,
    timeout_secs: u64,
) -> Observation {
    let handle = tokio::spawn(async move { tool.execute(args, &ToolContext { cwd }).await });

    let outcome = if timeout_secs == 0 {
        handle.await
    } else {
        let abort = handle.abort_handle();
        match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), handle).await {
            Ok(joined) => joined,
            Err(_elapsed) => {
                abort.abort();
                return Observation::error(format!(
                    "tool timed out after {timeout_secs}s and was aborted"
                ));
            }
        }
    };

    match outcome {
        Ok(Ok(obs)) => obs,
        Ok(Err(e)) => Observation::error(format!("tool errored: {e}")),
        Err(join_err) if join_err.is_panic() => {
            Observation::error("tool panicked (isolated; the run continues)")
        }
        Err(join_err) => Observation::error(format!("tool task failed: {join_err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{TaskMode, ToolCall};
    use agent_testkit::{
        final_turn, tool_turn, truncated_turn, truncated_turn_with, EchoTool, FnProvider,
        RecordingMemory, ScriptedProvider, StaticContext,
    };
    use rstest::rstest;
    use serde_json::json;
    use std::collections::VecDeque;

    // ---- mode switch decision (hysteresis) ------------------------------

    #[test]
    fn positive_decisive_verdict_switches_immediately() {
        let mut h = VecDeque::new();
        // A deterministic prefilter hit (0.95) switches on the first turn.
        let to = decide_switch(TaskMode::Other, TaskMode::Review, 0.95, 0.6, 2, &mut h);
        assert_eq!(to, Some(TaskMode::Review));
        assert!(h.is_empty(), "history cleared on a switch");
    }

    #[test]
    fn negative_same_mode_never_switches() {
        let mut h = VecDeque::new();
        let to = decide_switch(TaskMode::Debug, TaskMode::Debug, 0.99, 0.6, 2, &mut h);
        assert_eq!(to, None);
    }

    #[test]
    fn negative_below_floor_never_switches() {
        let mut h = VecDeque::new();
        let to = decide_switch(TaskMode::Other, TaskMode::Implement, 0.5, 0.6, 2, &mut h);
        assert_eq!(to, None);
    }

    #[test]
    fn boundary_weak_vote_needs_consecutive_turns() {
        // A weak (vote-level) verdict below the decisive bar must win `hysteresis`
        // consecutive turns before it switches — one turn is not enough.
        let mut h = VecDeque::new();
        assert_eq!(
            decide_switch(TaskMode::Other, TaskMode::Implement, 0.7, 0.6, 2, &mut h),
            None,
            "first weak turn does not switch"
        );
        assert_eq!(
            decide_switch(TaskMode::Other, TaskMode::Implement, 0.7, 0.6, 2, &mut h),
            Some(TaskMode::Implement),
            "second consecutive weak turn switches"
        );
    }

    #[test]
    fn corner_alternating_weak_proposals_do_not_switch() {
        // Thrash guard: two different weak candidates in a row build no streak.
        let mut h = VecDeque::new();
        assert_eq!(
            decide_switch(TaskMode::Other, TaskMode::Implement, 0.7, 0.6, 2, &mut h),
            None
        );
        assert_eq!(
            decide_switch(TaskMode::Other, TaskMode::Debug, 0.7, 0.6, 2, &mut h),
            None
        );
    }

    // ---- pure helpers ------------------------------------------------------

    #[test]
    fn recent_events_takes_last_n_saturating() {
        let msgs = vec![Message::user("a"), Message::user("b"), Message::user("c")];
        let last2 = recent_events(&msgs, 2);
        assert_eq!(last2.len(), 2);
        assert_eq!(last2[0].message.content_text(), "b");
        assert_eq!(last2[1].message.content_text(), "c");
        assert_eq!(last2[0].kind, "step");
        // n larger than the slice saturates to the whole slice; n == 0 is empty.
        assert_eq!(recent_events(&msgs, 99).len(), 3);
        assert!(recent_events(&msgs, 0).is_empty());
        assert!(recent_events(&[], 5).is_empty());
    }

    #[rstest]
    #[case::implement(TaskMode::Implement, &["coding", "testing"][..])]
    #[case::debug(TaskMode::Debug, &["coding", "testing"][..])]
    #[case::review(TaskMode::Review, &["coding", "git", "project"][..])]
    #[case::design(TaskMode::Design, &["project", "docs"][..])]
    #[case::explain(TaskMode::Explain, &["user"][..])]
    #[case::other_pulls_nothing(TaskMode::Other, &[][..])]
    fn recall_dims_for_each_mode(#[case] mode: TaskMode, #[case] want: &[&str]) {
        assert_eq!(recall_dims_for(mode), want);
    }

    #[rstest]
    #[case::prefilter("deterministic prefilter matched", "prefilter")]
    #[case::vote("pool vote 3/5 for review", "vote")]
    #[case::failsafe("kept current mode on a tie", "failsafe")]
    #[case::empty_is_failsafe("", "failsafe")]
    fn classify_via_labels_by_reason_prefix(#[case] reason: &str, #[case] want: &str) {
        assert_eq!(classify_via(reason), want);
    }

    // ---- mode detection integration (detect_mode -> record_mode_switch) ----

    /// A classifier that always votes one mode at a fixed confidence.
    struct FixedClassifier(TaskMode, f32);
    #[async_trait::async_trait]
    impl agent_core::TaskClassifier for FixedClassifier {
        fn name(&self) -> &str {
            "fixed"
        }
        async fn classify(&self, _ctx: &agent_core::ClassifyCtx<'_>) -> agent_core::ModeVerdict {
            agent_core::ModeVerdict {
                mode: self.0,
                confidence: self.1,
                reason: "deterministic test verdict".into(),
            }
        }
    }

    fn agent_with_classifier(
        mode: TaskMode,
        confidence: f32,
        memory: RecordingMemory,
    ) -> Arc<Agent> {
        Arc::new(
            Agent::new(
                Arc::new(FnProvider::new(|_req: &CompletionRequest| final_turn("ok"))),
                ToolRegistry::new(),
                Arc::new(memory),
                Arc::new(StaticContext),
                Arc::new(crate::policy::AutoApprove),
                Metrics::new(),
                settings(false),
            )
            .with_task_classifier(Arc::new(FixedClassifier(mode, confidence))),
        )
    }

    // A decisive verdict switches the session mode and records the switch.
    #[tokio::test]
    async fn positive_decisive_classification_switches_mode() {
        let memory = RecordingMemory::new();
        let agent = agent_with_classifier(TaskMode::Review, 0.95, memory.clone());
        let mut session = agent.session();
        assert_eq!(session.send("please review this").await.unwrap(), "ok");
        let switches: Vec<String> = memory
            .events()
            .into_iter()
            .filter(|e| e.kind == "mode_switch")
            .map(|e| e.message.content_text())
            .collect();
        assert!(
            switches.iter().any(|c| c.contains("review")),
            "expected a recorded switch into review, got: {switches:?}"
        );
    }

    // A below-floor verdict is not trusted: the mode stays put, nothing is recorded.
    #[tokio::test]
    async fn negative_low_confidence_does_not_switch_mode() {
        let memory = RecordingMemory::new();
        // 0.5 is below the 0.6 floor in `settings()`.
        let agent = agent_with_classifier(TaskMode::Review, 0.5, memory.clone());
        let mut session = agent.session();
        assert_eq!(session.send("maybe review?").await.unwrap(), "ok");
        assert!(
            !memory.events().into_iter().any(|e| e.kind == "mode_switch"),
            "a below-floor verdict must not switch the mode"
        );
    }

    // ---- buffered turn surfaces its answer to a live subscriber -------------

    fn agent_for_buffered(answer: &'static str) -> Arc<Agent> {
        Arc::new(Agent::new(
            Arc::new(FnProvider::new(move |_req: &CompletionRequest| {
                final_turn(answer)
            })),
            ToolRegistry::new(),
            Arc::new(RecordingMemory::new()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            settings(false), // stream = false → the buffered path
        ))
    }

    // The buffered path (`stream=false`) emits no incremental deltas, so without an
    // explicit publish a subscriber (the portal Agent view) would see the run
    // scaffolding but never the answer. A subscribed session must receive the
    // assistant's text as a `TokenDelta`.
    #[tokio::test]
    async fn positive_buffered_turn_publishes_answer_to_subscriber() {
        use agent_core::SessionSource;
        use futures_util::StreamExt;

        let key = agent_core::SessionKey::parse("u", "s-buffered").unwrap();
        let agent = agent_for_buffered("BUFFERED_ANSWER");
        // Subscribe BEFORE the run so `has_subscribers()` holds during the turn.
        let sink = agent.events.get_or_create(key.session.as_str());
        let mut stream = sink.subscribe();
        assert!(sink.has_subscribers());

        let mut session = agent.session_with(key.clone());
        assert_eq!(session.send("q").await.unwrap(), "BUFFERED_ANSWER");

        let mut saw_answer = false;
        while let Ok(Some(ev)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), stream.next()).await
        {
            if let agent_core::SessionEvent::TokenDelta { text } = ev {
                if text.contains("BUFFERED_ANSWER") {
                    saw_answer = true;
                    break;
                }
            }
        }
        assert!(
            saw_answer,
            "a buffered turn must publish its answer as a TokenDelta for live observers"
        );
    }

    // Guard the subscriber check: a buffered turn with NO subscriber must not
    // publish (the unobserved hot path stays allocation-free). The snapshot's
    // context is untouched by a delta, so we simply assert the run still returns
    // its answer without a subscriber attached.
    #[tokio::test]
    async fn boundary_buffered_turn_without_subscriber_still_answers() {
        let agent = agent_for_buffered("NO_SUB");
        let mut session = agent.session();
        assert_eq!(session.send("q").await.unwrap(), "NO_SUB");
    }

    // ---- dimensional memory (dimension_pass + recall_for_mode) --------------

    /// A DimensionStore that returns fixed summaries + recall items.
    struct FixtureDimensions {
        summaries: Vec<agent_core::DimensionSummary>,
        recall: Vec<agent_core::MemoryItem>,
    }
    #[async_trait::async_trait]
    impl agent_core::DimensionStore for FixtureDimensions {
        async fn summarize_step(
            &self,
            _events: &[MemoryEvent],
        ) -> agent_core::Result<Vec<agent_core::DimensionSummary>> {
            Ok(self.summaries.clone())
        }
        async fn recall_dimension(
            &self,
            _dimension: &str,
            _limit: usize,
        ) -> agent_core::Result<Vec<agent_core::MemoryItem>> {
            Ok(self.recall.clone())
        }
    }

    // After a successful turn, the dimensional summarize pass files each accepted
    // summary as a `kind = "dimension"` episodic event.
    #[tokio::test]
    async fn dimension_pass_records_summaries_after_a_turn() {
        let memory = RecordingMemory::new();
        let dims = FixtureDimensions {
            summaries: vec![agent_core::DimensionSummary {
                dimension: "coding".into(),
                summary: "wrote a parser".into(),
                is_new: false,
            }],
            recall: vec![],
        };
        let agent = Arc::new(
            Agent::new(
                Arc::new(FnProvider::new(|_req: &CompletionRequest| final_turn("ok"))),
                ToolRegistry::new(),
                Arc::new(memory.clone()),
                Arc::new(StaticContext),
                Arc::new(crate::policy::AutoApprove),
                Metrics::new(),
                settings(false),
            )
            .with_dimension_store(Arc::new(dims)),
        );
        agent.run("do work").await.unwrap();
        let dim_events: Vec<String> = memory
            .events()
            .into_iter()
            .filter(|e| e.kind == "dimension")
            .map(|e| e.message.content_text())
            .collect();
        assert!(
            dim_events.iter().any(|c| c.contains("coding")),
            "dimension summary not recorded: {dim_events:?}"
        );
    }

    // Entering a mode pulls that mode's dimensions back in as a fresh system block.
    #[tokio::test]
    async fn mode_switch_recalls_dimensional_history() {
        let dims = FixtureDimensions {
            summaries: vec![],
            recall: vec![agent_core::MemoryItem {
                source: "dim:coding".into(),
                content: "a prior coding note".into(),
            }],
        };
        let agent = Arc::new(
            Agent::new(
                Arc::new(FnProvider::new(|_req: &CompletionRequest| final_turn("ok"))),
                ToolRegistry::new(),
                Arc::new(RecordingMemory::new()),
                Arc::new(StaticContext),
                Arc::new(crate::policy::AutoApprove),
                Metrics::new(),
                settings(false),
            )
            .with_task_classifier(Arc::new(FixedClassifier(TaskMode::Review, 0.95)))
            .with_dimension_store(Arc::new(dims)),
        );
        let mut session = agent.session();
        session.send("review this").await.unwrap();
        let joined: String = session
            .messages()
            .iter()
            .map(Message::content_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("Relevant history for review mode"),
            "recalled dimensional block missing from the working set"
        );
        assert!(joined.contains("a prior coding note"));
    }

    // ---- SessionManager lifecycle ------------------------------------------

    fn base_agent() -> Arc<Agent> {
        Arc::new(Agent::new(
            Arc::new(FnProvider::new(|_req: &CompletionRequest| final_turn("ok"))),
            ToolRegistry::new(),
            Arc::new(RecordingMemory::new()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            settings(false),
        ))
    }
    fn key(user: &str, sess: &str) -> agent_core::SessionKey {
        agent_core::SessionKey::parse(user, sess).unwrap()
    }

    #[tokio::test]
    async fn session_manager_get_or_create_is_idempotent() {
        let mgr = SessionManager::new(base_agent());
        assert!(mgr.is_empty());
        let a = mgr.get_or_create(key("alice", "s1"));
        let b = mgr.get_or_create(key("alice", "s1"));
        assert!(a.same_session(&b), "same key must return the same session");
        assert_eq!(mgr.len(), 1);
        mgr.get_or_create(key("alice", "s2"));
        assert_eq!(mgr.len(), 2);
    }

    #[tokio::test]
    async fn session_manager_total_cap_rejects_new_but_not_existing() {
        let mgr = SessionManager::new(base_agent()).with_limits(1, 0);
        assert!(mgr.admit(key("alice", "s1")).is_ok());
        // A brand-new id beyond the cap is rejected (an amplification-spray guard)...
        assert!(matches!(
            mgr.admit(key("alice", "s2")),
            Err(OpenError::TotalLimit(1))
        ));
        // ...but re-entering an already-live session is never throttled.
        assert!(mgr.admit(key("alice", "s1")).is_ok());
    }

    #[tokio::test]
    async fn session_manager_per_user_cap_is_isolated() {
        let mgr = SessionManager::new(base_agent()).with_limits(0, 1);
        assert!(mgr.admit(key("alice", "s1")).is_ok());
        assert!(matches!(
            mgr.admit(key("alice", "s2")),
            Err(OpenError::PerUserLimit(1))
        ));
        // A different user has its own budget.
        assert!(mgr.admit(key("bob", "s1")).is_ok());
    }

    #[tokio::test]
    async fn session_manager_touch_and_remove() {
        let mgr = SessionManager::new(base_agent());
        mgr.get_or_create(key("alice", "s1"));
        assert!(mgr.touch(&key("alice", "s1")));
        assert!(!mgr.touch(&key("alice", "nope")), "absent key is not live");
        mgr.remove(&key("alice", "s1"));
        assert!(mgr.is_empty());
        mgr.remove(&key("alice", "s1")); // absent remove is a no-op
    }

    #[tokio::test]
    async fn session_manager_reap_idle() {
        let mgr = SessionManager::new(base_agent());
        mgr.get_or_create(key("alice", "s1"));
        // Nothing is idle beyond a long threshold.
        assert_eq!(mgr.reap_idle(std::time::Duration::from_secs(3600)), 0);
        assert_eq!(mgr.len(), 1);
        // Everything is idle beyond zero → reaped.
        assert_eq!(mgr.reap_idle(std::time::Duration::ZERO), 1);
        assert!(mgr.is_empty());
    }

    #[tokio::test]
    async fn session_registry_open_close_heartbeat() {
        use agent_core::SessionRegistry;
        let mgr = SessionManager::new(base_agent());
        let sid = mgr.open("alice").await.unwrap();
        assert_eq!(mgr.len(), 1);
        let k = key("alice", sid.as_str());
        assert!(mgr.heartbeat(&k).await.is_ok());
        mgr.close(&k).await.unwrap();
        assert!(mgr.is_empty());
        // Heartbeat on a reaped/absent session errors; close stays best-effort Ok.
        assert!(mgr.heartbeat(&k).await.is_err());
        assert!(mgr.close(&k).await.is_ok());
    }

    #[tokio::test]
    async fn session_registry_open_enforces_cap_and_rejects_bad_user() {
        use agent_core::SessionRegistry;
        let mgr = SessionManager::new(base_agent()).with_limits(1, 0);
        assert!(mgr.open("alice").await.is_ok());
        // Beyond the total cap → Overloaded (maps to gRPC RESOURCE_EXHAUSTED).
        let err = mgr.open("bob").await.unwrap_err();
        assert!(
            matches!(err, agent_core::Error::Overloaded(_)),
            "got {err:?}"
        );
        // Adversarial: a user id that isn't a safe path segment is rejected as Config.
        let mgr2 = SessionManager::new(base_agent());
        let bad = mgr2.open("../etc").await.unwrap_err();
        assert!(matches!(bad, agent_core::Error::Config(_)), "got {bad:?}");
    }

    // ---- seam accessors (--serve-<seam>) -----------------------------------

    // Every accessor is a clone-and-return; call them all on a base agent so the
    // lines are exercised and none panics. The confirmed-optional seams are None
    // when unwired.
    #[test]
    fn seam_accessors_are_callable_on_a_base_agent() {
        let agent = base_agent();
        let _ = agent.provider();
        let _ = agent.memory();
        let _ = agent.metrics();
        let _ = agent.context();
        let _ = agent.policy();
        let _ = agent.tools();
        let _ = agent.grpc_max_in_flight();
        let _ = agent.semantic();
        let _ = agent.episodic();
        let _ = agent.session_source_registry();
        let _ = agent.repo();
        let _ = agent.tokenizer();
        let _ = agent.web();
        let _ = agent.web_search();
        let _ = agent.sandbox();
        let _ = agent.pty();
        let _ = agent.forge();
        let _ = agent.tasks();
        let _ = agent.lsp();
        let _ = agent.embedder();
        let _ = agent.llm_pool();
        let _ = agent.metrics_proxy();
        let _ = agent.prompt_store();
        let _ = agent.dimension_store();
        let _ = agent.task_classifier();
        // Confirmed-optional, unwired ⇒ None.
        assert!(agent.search().is_none());
        assert!(agent.scanner().is_none());
        assert!(agent.review_collector().is_none());
    }

    // ---- tool-call verifier (enforce vs shadow) ----------------------------

    struct FixedVerifier(agent_core::VerifyVerdict);
    #[async_trait::async_trait]
    impl agent_core::Verifier for FixedVerifier {
        fn name(&self) -> &str {
            "fixed-verifier"
        }
        async fn verify(&self, _ctx: &agent_core::VerifyCtx<'_>) -> agent_core::VerifierReport {
            agent_core::VerifierReport {
                verdict: self.0.clone(),
                confidence: 0.9,
                model: "fixed".into(),
            }
        }
    }

    fn agent_with_verifier(
        verdict: agent_core::VerifyVerdict,
        enforce: bool,
        memory: RecordingMemory,
    ) -> Arc<Agent> {
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(EchoTool));
        Arc::new(
            Agent::new(
                Arc::new(ScriptedProvider::new(vec![
                    tool_turn(vec![ToolCall {
                        id: "t0".into(),
                        name: "echo".into(),
                        arguments: json!({"val": "ran"}),
                    }]),
                    final_turn("done"),
                ])),
                tools,
                Arc::new(memory),
                Arc::new(StaticContext),
                Arc::new(crate::policy::AutoApprove),
                Metrics::new(),
                settings(false),
            )
            .with_verifier(Arc::new(FixedVerifier(verdict)), enforce),
        )
    }

    fn tool_result_texts(memory: &RecordingMemory) -> Vec<String> {
        memory
            .events()
            .into_iter()
            .filter(|e| e.kind == "tool")
            .map(|e| e.message.content_text())
            .collect()
    }

    // Enforce + Deny: the call never runs; the block message is fed back.
    #[tokio::test]
    async fn verifier_enforce_deny_blocks_the_call() {
        let memory = RecordingMemory::new();
        let agent = agent_with_verifier(
            agent_core::VerifyVerdict::Deny("nope".into()),
            true,
            memory.clone(),
        );
        assert_eq!(agent.run("go").await.unwrap(), "done");
        let msgs = tool_result_texts(&memory);
        assert!(
            msgs.iter().any(|c| c.contains("blocked by the verifier")),
            "expected a verifier block, got: {msgs:?}"
        );
        assert!(
            !msgs.iter().any(|c| c.contains("ran")),
            "the tool ran despite an enforce-mode deny"
        );
    }

    // Enforce + Revise: the call is skipped and the revise hint is fed back.
    #[tokio::test]
    async fn verifier_enforce_revise_feeds_back_and_skips() {
        let memory = RecordingMemory::new();
        let agent = agent_with_verifier(
            agent_core::VerifyVerdict::Revise("tighten the args".into()),
            true,
            memory.clone(),
        );
        assert_eq!(agent.run("go").await.unwrap(), "done");
        let msgs = tool_result_texts(&memory);
        assert!(
            msgs.iter()
                .any(|c| c.contains("asks you to revise") && c.contains("tighten the args")),
            "expected a revise hint, got: {msgs:?}"
        );
        assert!(
            !msgs.iter().any(|c| c.contains("ran")),
            "the tool ran despite an enforce-mode revise"
        );
    }

    // Shadow: a Deny verdict is only observed — the call still runs.
    #[tokio::test]
    async fn verifier_shadow_observes_but_allows_the_call() {
        let memory = RecordingMemory::new();
        let agent = agent_with_verifier(
            agent_core::VerifyVerdict::Deny("nope".into()),
            false,
            memory.clone(),
        );
        assert_eq!(agent.run("go").await.unwrap(), "done");
        let msgs = tool_result_texts(&memory);
        assert!(
            msgs.iter().any(|c| c.contains("ran")),
            "shadow mode must not block the call, got: {msgs:?}"
        );
    }

    // ---- review-in-loop (collect grounded facts on entering Review) ---------

    #[cfg(feature = "review")]
    struct FixtureReview;
    #[cfg(feature = "review")]
    #[async_trait::async_trait]
    impl agent_core::ReviewCollector for FixtureReview {
        fn name(&self) -> &str {
            "fixture-review"
        }
        async fn collect(
            &self,
            _target: &agent_core::ReviewTarget,
        ) -> agent_core::Result<agent_core::ReviewFacts> {
            Ok(agent_core::ReviewFacts::default())
        }
    }

    #[cfg(feature = "review")]
    #[tokio::test]
    async fn review_in_loop_collects_on_switch_to_review() {
        let memory = RecordingMemory::new();
        let mut s = settings(false);
        s.review_in_loop = true;
        let agent = Arc::new(
            Agent::new(
                Arc::new(FnProvider::new(|_req: &CompletionRequest| final_turn("ok"))),
                ToolRegistry::new(),
                Arc::new(memory.clone()),
                Arc::new(StaticContext),
                Arc::new(crate::policy::AutoApprove),
                Metrics::new(),
                s,
            )
            .with_task_classifier(Arc::new(FixedClassifier(TaskMode::Review, 0.95)))
            .with_review_collector(Arc::new(FixtureReview)),
        );
        let mut session = agent.session();
        session.send("review this").await.unwrap();
        // Entering review recorded a review run...
        assert!(
            memory.events().into_iter().any(|e| e.kind == "review"),
            "entering review must record a review run"
        );
        // ...and injected the rendered grounded-facts block into the working set.
        let joined: String = session
            .messages()
            .iter()
            .map(Message::content_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("Grounded review facts"),
            "review facts were not injected into the working set"
        );
    }

    // ---- Session state methods (load / add_context / is_started / compact) --

    #[tokio::test]
    async fn session_load_sets_started_and_messages() {
        let agent = base_agent();
        let mut session = agent.session();
        assert!(!session.is_started());
        session.load(vec![Message::system("s"), Message::user("hi")]);
        assert!(session.is_started());
        assert_eq!(session.messages().len(), 2);
    }

    #[tokio::test]
    async fn session_add_context_queues_before_start_then_applies_after() {
        let agent = base_agent();
        let mut session = agent.session();
        // Before the first turn: queued, not yet in the working set.
        session.add_context("skill body".into());
        assert!(
            session.messages().is_empty(),
            "queued before the first turn"
        );
        session.send("go").await.unwrap();
        // The queued block was drained into the working set during assembly.
        let joined: String = session
            .messages()
            .iter()
            .map(Message::content_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("skill body"), "queued context not applied");
        // After start, add_context applies immediately.
        let before = session.messages().len();
        session.add_context("more".into());
        assert_eq!(session.messages().len(), before + 1);
    }

    #[tokio::test]
    async fn session_compact_runs_cleanly() {
        let agent = base_agent();
        let mut session = agent.session();
        session.send("go").await.unwrap();
        // StaticContext never compacts, but the manual `/compact` path must run.
        session.compact().await.unwrap();
    }

    // ---- @-reference expansion ---------------------------------------------

    #[cfg(feature = "reference")]
    struct FixtureResolver;
    #[cfg(feature = "reference")]
    #[async_trait::async_trait]
    impl agent_core::ReferenceResolver for FixtureResolver {
        async fn resolve(&self, _prompt: &str, _budget: usize) -> agent_core::Resolution {
            agent_core::Resolution {
                blocks: vec![agent_core::ContextBlock {
                    source: "@file.rs".into(),
                    content: "fn main() {}".into(),
                }],
                warnings: vec!["one reference was truncated".into()],
                blocked: false,
            }
        }
    }

    #[cfg(feature = "reference")]
    #[tokio::test]
    async fn reference_expansion_injects_resolved_blocks_on_continuation() {
        let agent = Arc::new(
            Agent::new(
                Arc::new(FnProvider::new(|_req: &CompletionRequest| final_turn("ok"))),
                ToolRegistry::new(),
                Arc::new(RecordingMemory::new()),
                Arc::new(StaticContext),
                Arc::new(crate::policy::AutoApprove),
                Metrics::new(),
                settings(false),
            )
            .with_reference_resolver(Arc::new(FixtureResolver), 1000),
        );
        let mut session = agent.session();
        session.send("first").await.unwrap();
        // On a continuation turn, resolved references are injected as system context
        // ahead of the new user message.
        session.send("see @file.rs").await.unwrap();
        let joined: String = session
            .messages()
            .iter()
            .map(Message::content_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("fn main() {}"),
            "the resolved reference block was not injected"
        );
    }

    // ---- hooks (pre_turn fires) --------------------------------------------

    struct CountingHook(Arc<std::sync::atomic::AtomicUsize>);
    #[async_trait::async_trait]
    impl agent_core::Hook for CountingHook {
        fn name(&self) -> &str {
            "counting"
        }
        async fn pre_turn(&self, _working: &WorkingSet) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn hooks_pre_turn_fires_each_iteration() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut hooks = agent_core::HookRegistry::new();
        hooks.register(Arc::new(CountingHook(calls.clone())));
        let agent = Arc::new(
            Agent::new(
                Arc::new(FnProvider::new(|_req: &CompletionRequest| final_turn("ok"))),
                ToolRegistry::new(),
                Arc::new(RecordingMemory::new()),
                Arc::new(StaticContext),
                Arc::new(crate::policy::AutoApprove),
                Metrics::new(),
                settings(false),
            )
            .with_hooks(hooks),
        );
        agent.run("go").await.unwrap();
        assert!(
            calls.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "the pre_turn hook did not fire"
        );
    }

    /// Emits three tool calls on the first turn, then a final answer. The
    /// `EchoTool` sleeps per `sleep_ms`, so completion order differs from call
    /// order (t0 sleeps longest yet is requested first).
    fn seq_provider() -> ScriptedProvider {
        ScriptedProvider::new(vec![
            tool_turn(vec![
                ToolCall {
                    id: "t0".into(),
                    name: "echo".into(),
                    arguments: json!({"sleep_ms": 40, "val": "a"}),
                },
                ToolCall {
                    id: "t1".into(),
                    name: "echo".into(),
                    arguments: json!({"sleep_ms": 0, "val": "b"}),
                },
                ToolCall {
                    id: "t2".into(),
                    name: "echo".into(),
                    arguments: json!({"sleep_ms": 15, "val": "c"}),
                },
            ]),
            final_turn("done"),
        ])
    }

    fn settings(parallel: bool) -> Settings {
        Settings {
            max_iterations: 5,
            max_tokens: 100,
            temperature: 0.0,
            context_window: 100_000,
            reserve_output: 1000,
            system_prompt: "sys".into(),
            stream: false,
            parallel_tools: parallel,
            tool_timeout_secs: 30,
            recall_limit: 0,
            cwd: std::env::temp_dir(),
            model: "m".into(),
            session_id: String::new(),
            context_prepend: vec![],
            context_append: vec![],
            review_in_loop: false,
            review_context_budget: 24_000,
            mode_confidence_floor: 0.6,
            mode_hysteresis: 2,
            grpc_max_in_flight: 0,
        }
    }

    async fn run_with(parallel: bool) -> Vec<String> {
        let memory = RecordingMemory::new();
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(EchoTool));
        let agent = Arc::new(Agent::new(
            Arc::new(seq_provider()),
            tools,
            Arc::new(memory.clone()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            settings(parallel),
        ));
        let out = agent.run("go").await.unwrap();
        assert_eq!(out, "done");
        memory.tool_order()
    }

    #[tokio::test]
    async fn tool_results_preserve_call_order_when_parallel() {
        // t0 sleeps longest yet is first: order must still be t0, t1, t2.
        assert_eq!(run_with(true).await, vec!["t0", "t1", "t2"]);
    }

    #[tokio::test]
    async fn tool_results_preserve_call_order_when_sequential() {
        assert_eq!(run_with(false).await, vec!["t0", "t1", "t2"]);
    }

    /// A policy that denies exactly one tool name — drives the loop's deny branch.
    struct DenyNamed(&'static str);
    #[async_trait::async_trait]
    impl agent_core::Policy for DenyNamed {
        async fn authorize(&self, call: &ToolCall) -> agent_core::Decision {
            if call.name == self.0 {
                agent_core::Decision::Deny("blocked in test".into())
            } else {
                agent_core::Decision::Allow
            }
        }
    }

    #[tokio::test]
    async fn denied_tool_is_not_run_and_is_reported() {
        let memory = RecordingMemory::new();
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(EchoTool));
        let provider = ScriptedProvider::new(vec![
            tool_turn(vec![ToolCall {
                id: "t0".into(),
                name: "echo".into(),
                arguments: json!({"val": "secret"}),
            }]),
            final_turn("done"),
        ]);
        let agent = Arc::new(Agent::new(
            Arc::new(provider),
            tools,
            Arc::new(memory.clone()),
            Arc::new(StaticContext),
            Arc::new(DenyNamed("echo")),
            Metrics::new(),
            settings(false),
        ));
        let out = agent.run("go").await.unwrap();
        assert_eq!(out, "done"); // a denial adapts; it does not abort the run

        let tool_msgs: Vec<String> = memory
            .events()
            .into_iter()
            .filter(|e| e.kind == "tool")
            .map(|e| e.message.content_text())
            .collect();
        // The recorded tool result is the denial, and EchoTool never ran (it would
        // otherwise have echoed "secret" back as the result).
        assert!(
            tool_msgs
                .iter()
                .any(|c| c.contains("denied by policy: blocked in test")),
            "no denial recorded: {tool_msgs:?}"
        );
        assert!(
            !tool_msgs.iter().any(|c| c.contains("secret")),
            "tool ran despite deny: {tool_msgs:?}"
        );
    }

    #[tokio::test]
    async fn session_keeps_history_across_turns() {
        // Answers with the number of user messages it sees, proving the working
        // set carried over from the previous turn.
        let counting = FnProvider::new(|req: &CompletionRequest| {
            let users = req.messages.iter().filter(|m| m.role == Role::User).count();
            final_turn(users.to_string())
        });
        let agent = Arc::new(Agent::new(
            Arc::new(counting),
            ToolRegistry::new(),
            Arc::new(RecordingMemory::new()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            settings(false),
        ));
        let mut session = agent.session();
        // Turn 1 sees one user message; turn 2 sees two → history persisted.
        assert_eq!(session.send("hi").await.unwrap(), "1");
        assert_eq!(session.send("more").await.unwrap(), "2");
        // system + user + assistant (turn 1) + user + assistant (turn 2).
        assert!(session.messages().len() >= 5);
    }

    // ---- loop-dispatch coverage (doc 06) -----------------------------------

    fn tool_call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: json!({}),
        }
    }

    /// A tool that always errors — exercises `execute() == Err` → the
    /// `"tool errored: …"` observation.
    struct ErrTool;
    #[async_trait::async_trait]
    impl agent_core::Tool for ErrTool {
        fn name(&self) -> &str {
            "boom"
        }
        fn schema(&self) -> agent_core::ToolSchema {
            agent_core::ToolSchema {
                name: "boom".into(),
                description: "always fails".into(),
                parameters: json!({"type": "object"}),
            }
        }
        async fn execute(
            &self,
            _a: serde_json::Value,
            _c: &agent_core::ToolContext,
        ) -> agent_core::Result<Observation> {
            Err(agent_core::Error::Tool("kaboom".into()))
        }
    }

    /// A tool whose (already-capped) output carries the truncation marker — the
    /// loop must record it verbatim.
    struct BigTool;
    #[async_trait::async_trait]
    impl agent_core::Tool for BigTool {
        fn name(&self) -> &str {
            "big"
        }
        fn schema(&self) -> agent_core::ToolSchema {
            agent_core::ToolSchema {
                name: "big".into(),
                description: "big output".into(),
                parameters: json!({"type": "object"}),
            }
        }
        async fn execute(
            &self,
            _a: serde_json::Value,
            _c: &agent_core::ToolContext,
        ) -> agent_core::Result<Observation> {
            Ok(Observation::ok(format!(
                "{}\n...[output truncated]",
                "x".repeat(12_000)
            )))
        }
    }

    /// Tracks peak concurrent executions so a test can prove the loop honours
    /// `parallel_safe` (sequential when false, concurrent when true).
    struct ConcProbe {
        active: Arc<std::sync::atomic::AtomicUsize>,
        max: Arc<std::sync::atomic::AtomicUsize>,
        safe: bool,
    }
    #[async_trait::async_trait]
    impl agent_core::Tool for ConcProbe {
        fn name(&self) -> &str {
            "conc"
        }
        fn schema(&self) -> agent_core::ToolSchema {
            agent_core::ToolSchema {
                name: "conc".into(),
                description: "concurrency probe".into(),
                parameters: json!({"type": "object"}),
            }
        }
        fn parallel_safe(&self) -> bool {
            self.safe
        }
        async fn execute(
            &self,
            _a: serde_json::Value,
            _c: &agent_core::ToolContext,
        ) -> agent_core::Result<Observation> {
            use std::sync::atomic::Ordering::SeqCst;
            let now = self.active.fetch_add(1, SeqCst) + 1;
            self.max.fetch_max(now, SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            self.active.fetch_sub(1, SeqCst);
            Ok(Observation::ok("ok"))
        }
    }

    /// Run one tool turn (`calls`) then a final "done"; return the recorded
    /// `(tool_call_id, content)` events in order.
    async fn dispatch_events(
        tools: ToolRegistry,
        policy: Arc<dyn agent_core::Policy>,
        calls: Vec<ToolCall>,
    ) -> Vec<(String, String)> {
        let memory = RecordingMemory::new();
        let provider = ScriptedProvider::new(vec![tool_turn(calls), final_turn("done")]);
        let agent = Arc::new(Agent::new(
            Arc::new(provider),
            tools,
            Arc::new(memory.clone()),
            Arc::new(StaticContext),
            policy,
            Metrics::new(),
            settings(false),
        ));
        assert_eq!(agent.run("go").await.unwrap(), "done");
        memory
            .events()
            .into_iter()
            .filter(|e| e.kind == "tool")
            .map(|e| {
                (
                    e.message.tool_call_id.clone().unwrap_or_default(),
                    e.message.content_text(),
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn unknown_tool_becomes_error_observation() {
        let events = dispatch_events(
            ToolRegistry::new(),
            Arc::new(crate::policy::AutoApprove),
            vec![tool_call("t0", "nope")],
        )
        .await;
        assert_eq!(events.len(), 1);
        assert!(events[0].1.contains("unknown tool `nope`"), "{events:?}");
    }

    #[tokio::test]
    async fn tool_error_becomes_observation() {
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(ErrTool));
        let events = dispatch_events(
            tools,
            Arc::new(crate::policy::AutoApprove),
            vec![tool_call("t0", "boom")],
        )
        .await;
        // Wrapped as `tool errored: {e}`, where `e` is `Error::Tool`'s Display.
        assert!(
            events[0].1.contains("tool errored") && events[0].1.contains("kaboom"),
            "{events:?}"
        );
    }

    #[tokio::test]
    async fn oversized_output_cap_marker_is_recorded() {
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(BigTool));
        let events = dispatch_events(
            tools,
            Arc::new(crate::policy::AutoApprove),
            vec![tool_call("t0", "big")],
        )
        .await;
        assert!(
            events[0].1.ends_with("...[output truncated]"),
            "truncation marker not carried into the record"
        );
    }

    #[tokio::test]
    async fn loop_terminates_at_max_iterations() {
        // ScriptedProvider repeats its last response, so the loop is only ever
        // handed a tool request and never an empty-tool-calls (final) turn.
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(EchoTool));
        let provider = ScriptedProvider::new(vec![tool_turn(vec![tool_call("t0", "echo")])]);
        let mut s = settings(false);
        s.max_iterations = 3;
        let agent = Arc::new(Agent::new(
            Arc::new(provider),
            tools,
            Arc::new(RecordingMemory::new()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            s,
        ));
        let err = agent
            .run("go")
            .await
            .expect_err("should hit the iteration bound")
            .to_string();
        assert!(err.contains("max_iterations"), "{err}");
    }

    // ---- worktree cleanup on exit ------------------------------------------

    /// A scriptable `RepoBackend` for `Agent::cleanup`: `list` is what
    /// `worktree_list` returns (`None` ⇒ the list call errors), `fail_remove` names
    /// ids whose `worktree_remove` errors, and `removed` records every id cleanup
    /// *attempted*. Everything else is unimplemented (cleanup only touches
    /// `worktree_list` / `worktree_remove`).
    #[derive(Clone)]
    struct RecordingRepo {
        list: Option<Vec<String>>,
        fail_remove: Vec<String>,
        removed: Arc<std::sync::Mutex<Vec<String>>>,
    }
    impl RecordingRepo {
        fn new(list: Option<Vec<&str>>, fail_remove: Vec<&str>) -> Self {
            Self {
                list: list.map(|l| l.into_iter().map(String::from).collect()),
                fail_remove: fail_remove.into_iter().map(String::from).collect(),
                removed: Arc::new(std::sync::Mutex::new(Vec::new())),
            }
        }
    }
    #[async_trait::async_trait]
    impl agent_core::RepoBackend for RecordingRepo {
        async fn worktree_list(&self) -> agent_core::Result<Vec<agent_core::WorktreeHandle>> {
            let ids = self
                .list
                .clone()
                .ok_or_else(|| agent_core::Error::Repo("list failed".into()))?;
            Ok(ids
                .into_iter()
                .map(|id| agent_core::WorktreeHandle {
                    path: std::path::PathBuf::from(&id),
                    id,
                    head: agent_core::Oid("0".into()),
                    revision: agent_core::Revision("HEAD".into()),
                    writable: true,
                })
                .collect())
        }
        async fn worktree_remove(&self, id: &str) -> agent_core::Result<()> {
            self.removed.lock().unwrap().push(id.to_string());
            if self.fail_remove.iter().any(|f| f == id) {
                return Err(agent_core::Error::Repo(format!("remove `{id}` failed")));
            }
            Ok(())
        }
        // --- unused by cleanup ---
        async fn resolve(&self, _: &agent_core::Revision) -> agent_core::Result<agent_core::Oid> {
            unimplemented!()
        }
        async fn read_file(
            &self,
            _: &agent_core::Revision,
            _: &std::path::Path,
        ) -> agent_core::Result<agent_core::BlobContent> {
            unimplemented!()
        }
        async fn list_tree(
            &self,
            _: &agent_core::Revision,
            _: &std::path::Path,
            _: bool,
        ) -> agent_core::Result<Vec<agent_core::TreeEntry>> {
            unimplemented!()
        }
        async fn diff(
            &self,
            _: &agent_core::Revision,
            _: &agent_core::Revision,
            _: &[String],
        ) -> agent_core::Result<agent_core::DiffResult> {
            unimplemented!()
        }
        async fn grep(
            &self,
            _: &agent_core::Revision,
            _: &str,
            _: &[String],
            _: usize,
        ) -> agent_core::Result<Vec<agent_core::GrepHit>> {
            unimplemented!()
        }
        async fn log(
            &self,
            _: &agent_core::Revision,
            _: Option<&std::path::Path>,
            _: usize,
        ) -> agent_core::Result<Vec<agent_core::CommitInfo>> {
            unimplemented!()
        }
        async fn branches(&self) -> agent_core::Result<Vec<(String, agent_core::Oid)>> {
            unimplemented!()
        }
        async fn status(&self) -> agent_core::Result<agent_core::RepoStatus> {
            unimplemented!()
        }
        async fn fetch(&self) -> agent_core::Result<agent_core::RepoStatus> {
            unimplemented!()
        }
        async fn worktree_add(
            &self,
            _: &agent_core::WorktreeSpec,
        ) -> agent_core::Result<agent_core::WorktreeHandle> {
            unimplemented!()
        }
        async fn checkpoint(&self, _: &str, _: &str) -> agent_core::Result<agent_core::Checkpoint> {
            unimplemented!()
        }
        async fn push(&self, _: &agent_core::Checkpoint, _: &str) -> agent_core::Result<()> {
            unimplemented!()
        }
    }

    fn bare_agent() -> Agent {
        Agent::new(
            Arc::new(ScriptedProvider::new(vec![final_turn("x")])),
            ToolRegistry::new(),
            Arc::new(RecordingMemory::new()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            settings(false),
        )
    }

    /// `positive_`: the manager keys sessions by `(user, session)` — the same key
    /// reuses one session, distinct keys get distinct ones, and `remove` frees it.
    #[tokio::test]
    async fn session_manager_keys_reuses_and_removes() {
        let mgr = SessionManager::new(Arc::new(bare_agent()));
        assert!(mgr.is_empty());
        let alice = agent_core::SessionKey::parse("alice", "s1").unwrap();
        let bob = agent_core::SessionKey::parse("bob", "s1").unwrap();
        let a1 = mgr.get_or_create(alice.clone());
        let a2 = mgr.get_or_create(alice.clone());
        assert!(a1.same_session(&a2), "same key reuses the session");
        let b1 = mgr.get_or_create(bob.clone());
        assert!(!a1.same_session(&b1), "distinct keys → distinct sessions");
        assert_eq!(mgr.len(), 2);
        mgr.remove(&alice);
        assert_eq!(mgr.len(), 1);
        mgr.remove(&alice); // absent key is a no-op
        assert_eq!(mgr.len(), 1);
    }

    /// `positive_`: a manager-created session carries its `(user, session)` identity,
    /// and distinct sessions keep independent transcripts over the shared backend.
    #[tokio::test]
    async fn session_manager_sessions_are_independent() {
        let mgr = SessionManager::new(Arc::new(bare_agent()));
        let alice = mgr.get_or_create(agent_core::SessionKey::parse("alice", "s1").unwrap());
        let bob = mgr.get_or_create(agent_core::SessionKey::parse("bob", "s2").unwrap());
        // Each handle carries its own `(user, session)` identity (the manager built each
        // session with `session_with(key)`), and they are distinct live sessions.
        assert_eq!(alice.key().user.as_str(), "alice");
        assert_eq!(bob.key().user.as_str(), "bob");
        assert_eq!(bob.key().session.as_str(), "s2");
        assert!(!alice.same_session(&bob));
    }

    /// `adversarial_`: the capacity caps guard against a hostile new-id spray, per
    /// user and overall — but a client re-entering an *existing* session is never
    /// throttled, and the lazy `get_or_create` path never rejects.
    #[tokio::test]
    async fn adversarial_open_enforces_per_user_and_total_caps() {
        let mgr = SessionManager::new(Arc::new(bare_agent())).with_limits(3, 2);
        let k = |u, s| agent_core::SessionKey::parse(u, s).unwrap();

        // Alice may open up to 2 (per-user cap).
        assert!(mgr.admit(k("alice", "s1")).is_ok());
        assert!(mgr.admit(k("alice", "s2")).is_ok());
        assert_eq!(
            mgr.admit(k("alice", "s3")).map(|_| ()).unwrap_err(),
            OpenError::PerUserLimit(2),
            "3rd alice session is rejected"
        );
        // Re-opening an existing session is always allowed (not a new allocation).
        assert!(mgr.admit(k("alice", "s1")).is_ok());

        // Bob opens one; the next new session hits the *global* cap (3) before bob's
        // own per-user cap.
        assert!(mgr.admit(k("bob", "b1")).is_ok());
        assert_eq!(
            mgr.admit(k("bob", "b2")).map(|_| ()).unwrap_err(),
            OpenError::TotalLimit(3)
        );

        // The lazy correctness path never rejects (a split-deployment first-use).
        let _ = mgr.get_or_create(k("carol", "c1"));
        assert_eq!(mgr.len(), 4);
    }

    /// `positive_`/`adversarial_`: the `SessionRegistry` trait impl mints a fresh
    /// server-side id per `open`, `close` frees it, `heartbeat` touches it (and errors
    /// for an unknown session), and a capacity cap surfaces as `Overloaded`.
    #[tokio::test]
    async fn session_registry_open_mints_and_close_heartbeat() {
        use agent_core::SessionRegistry as _;
        let mgr = SessionManager::new(Arc::new(bare_agent())).with_limits(0, 1);

        // Each open mints a *distinct* id and pre-allocates the session.
        let id1 = mgr.open("alice").await.unwrap();
        assert_eq!(mgr.len(), 1);
        let key1 = agent_core::SessionKey::parse("alice", id1.as_str()).unwrap();
        assert!(mgr.touch(&key1), "the minted session is live");

        // Heartbeat touches a live session; an unknown one errors (client should re-open).
        mgr.heartbeat(&key1).await.unwrap();
        let ghost = agent_core::SessionKey::parse("alice", "ghost").unwrap();
        assert!(mgr.heartbeat(&ghost).await.is_err());

        // The per-user cap (1) rejects a *second* alice open as Overloaded.
        let err = mgr.open("alice").await.unwrap_err();
        assert!(
            matches!(err, agent_core::Error::Overloaded(_)),
            "capacity rejection: {err:?}"
        );

        // A malformed user is rejected before minting.
        assert!(mgr.open("../etc").await.is_err());

        // Close frees it; a second close is a no-op (best-effort).
        mgr.close(&key1).await.unwrap();
        assert_eq!(mgr.len(), 0);
        mgr.close(&key1).await.unwrap();
    }

    /// A provider whose `complete` parks until released — so a run stays *in-flight*
    /// (the session's `busy` flag set) while the test does something concurrently.
    /// `entered` fires (permit stored) the instant the loop reaches the provider, so the
    /// test can wait until the run is genuinely mid-turn without a sleep.
    struct GateProvider {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }
    #[async_trait::async_trait]
    impl LlmProvider for GateProvider {
        fn capabilities(&self) -> agent_core::ModelCapabilities {
            agent_core::ModelCapabilities {
                supports_tools: false,
                context_window: 1000,
                supports_response_format: false,
                supports_vision: false,
            }
        }
        async fn complete(
            &self,
            _req: agent_core::CompletionRequest,
        ) -> agent_core::Result<agent_core::CompletionResponse> {
            self.entered.notify_one();
            self.release.notified().await;
            Ok(final_turn("done"))
        }
    }

    fn gated_manager(
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    ) -> Arc<SessionManager> {
        let agent = Agent::new(
            Arc::new(GateProvider { entered, release }),
            ToolRegistry::new(),
            Arc::new(RecordingMemory::new()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            settings(false),
        );
        Arc::new(SessionManager::new(Arc::new(agent)))
    }

    /// `positive_`: `reap_idle` frees a quiet session but **skips one mid-turn** — the
    /// `busy` flag replaces the old `try_lock` probe. Driven by a real in-flight run.
    #[tokio::test]
    async fn positive_reap_skips_busy_and_removes_idle() {
        use std::time::Duration;
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let mgr = gated_manager(entered.clone(), release.clone());

        let busy = key("alice", "busy");
        let idle = key("bob", "idle");
        mgr.get_or_create(idle.clone()); // a quiet session
        let busy_handle = mgr.get_or_create(busy.clone());
        assert_eq!(mgr.len(), 2);

        // Drive a run on `busy` that parks in the provider; wait until it is mid-turn.
        let run = tokio::spawn(async move { busy_handle.run("go").await });
        entered.notified().await; // the run is now in the provider ⇒ busy = true

        // A zero window would reap everything idle, but the busy session is skipped.
        assert_eq!(
            mgr.reap_idle(Duration::ZERO),
            1,
            "only the idle one is reaped"
        );
        assert!(mgr.touch(&busy), "the busy session is still live");
        assert!(!mgr.touch(&idle), "the idle session was reaped");

        // Release the run; it finishes and the session goes idle again.
        release.notify_one();
        run.await.unwrap().unwrap();
        assert_eq!(
            mgr.reap_idle(Duration::ZERO),
            1,
            "now the finished run is reapable"
        );
    }

    /// `positive_`: `remove` aborts a session that is **mid-turn** — the run task is
    /// stopped (its result channel drops), not left running after the session is gone.
    #[tokio::test]
    async fn positive_remove_aborts_inflight_run() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let mgr = gated_manager(entered.clone(), release);

        let k = key("alice", "s1");
        let handle = mgr.get_or_create(k.clone());
        let run = tokio::spawn(async move { handle.run("go").await });
        entered.notified().await; // run is mid-turn

        mgr.remove(&k); // abort the actor out from under the in-flight run
        assert!(mgr.is_empty());
        // The parked run does not complete normally — its actor was aborted, so the
        // result channel drops and `run` returns an error (never hangs).
        assert!(
            run.await.unwrap().is_err(),
            "an aborted run resolves to Err"
        );
    }

    /// `boundary_`: a run records the curated families under its `(session, user)`
    /// label, and `SessionManager::remove` retires the session's gauge series so a
    /// dead session leaves no stale `agent_active` (docs/design/multi-session/06).
    #[tokio::test]
    async fn session_manager_remove_retires_session_metrics() {
        let mgr = SessionManager::new(Arc::new(bare_agent()));
        let key = agent_core::SessionKey::parse("alice", "s1").unwrap();
        mgr.get_or_create(key.clone()).run("hi").await.unwrap();

        // The run is attributed to this tenant.
        let text = mgr.backend().metrics().encode_text();
        assert!(
            text.lines().any(|l| l.starts_with("agent_runs_total{")
                && l.contains("session=\"s1\"")
                && l.contains("user=\"alice\"")),
            "run not tenant-labeled:\n{text}"
        );

        // After removal the gauge series is gone (retired).
        mgr.remove(&key);
        let after = mgr.backend().metrics().encode_text();
        assert!(
            !after
                .lines()
                .any(|l| l.starts_with("agent_active{session=\"s1\"")),
            "active gauge not retired on remove:\n{after}"
        );
    }

    /// Cleanup must remove **exactly** what `worktree_list` reports (it can't reach
    /// anything else — that's the session-scoping guarantee), keep going when a
    /// remove fails, and never panic when the list call errors. `list = None` models
    /// the list RPC failing; `fail` names ids whose remove errors. `expected` is the
    /// set of ids cleanup should *attempt*.
    #[rstest]
    #[case::positive_removes_all(Some(vec!["w0", "w1"]), vec![], vec!["w0", "w1"])]
    #[case::boundary_empty_list(Some(vec![]), vec![], vec![])]
    #[case::boundary_single(Some(vec!["only"]), vec![], vec!["only"])]
    #[case::negative_list_error_is_swallowed(None, vec![], vec![])]
    #[case::corner_partial_failure_continues(
        Some(vec!["w0", "w1", "w2"]), vec!["w1"], vec!["w0", "w1", "w2"])]
    #[case::corner_all_removes_fail(Some(vec!["w0", "w1"]), vec!["w0", "w1"], vec!["w0", "w1"])]
    #[tokio::test]
    async fn cleanup_cases(
        #[case] list: Option<Vec<&str>>,
        #[case] fail: Vec<&str>,
        #[case] expected: Vec<&str>,
    ) {
        let repo = RecordingRepo::new(list, fail);
        let agent = bare_agent().with_repo(Arc::new(repo.clone()));
        agent.cleanup().await; // must not panic on any input
        let got = repo.removed.lock().unwrap().clone();
        let want: Vec<String> = expected.into_iter().map(String::from).collect();
        assert_eq!(got, want);
    }

    #[tokio::test]
    async fn cleanup_is_a_noop_without_a_repo() {
        // No git backend wired → cleanup does nothing and doesn't panic.
        bare_agent().cleanup().await;
    }

    // ---- truncation handling (finish_reason = length / max_tokens) ----------
    //
    // A completion truncated at the output-token cap parses to zero tool calls; the
    // loop must NOT mistake it for a final answer (that scored a cut-off file-write
    // as a "successful" 0 in the graph-arena campaign). It nudges + continues, and
    // fails fast if the model truncates forever.

    /// The classifier: exactly `length`/`max_tokens` (case-insensitive), nothing
    /// else — an unrecognised (or hostile) reason is a normal stop, the fail-safe
    /// direction.
    #[rstest]
    #[case::positive_openai_length("length", true)]
    #[case::positive_anthropic_max_tokens("max_tokens", true)]
    #[case::corner_uppercase("LENGTH", true)]
    #[case::corner_mixed_case("Max_Tokens", true)]
    #[case::negative_stop("stop", false)]
    #[case::negative_tool_calls("tool_calls", false)]
    #[case::negative_end_turn("end_turn", false)]
    #[case::boundary_empty("", false)]
    #[case::adversarial_trailing_space("length ", false)]
    #[case::adversarial_injection("length; DROP TABLE", false)]
    #[case::adversarial_unicode_lookalike("lëngth", false)]
    fn is_truncated_finish_cases(#[case] finish: &str, #[case] want: bool) {
        assert_eq!(is_truncated_finish(finish), want);
    }

    fn agent_over(provider: Arc<dyn LlmProvider>, max_iterations: usize) -> Arc<Agent> {
        let mut s = settings(false);
        s.max_iterations = max_iterations;
        Arc::new(Agent::new(
            provider,
            ToolRegistry::new(),
            Arc::new(RecordingMemory::new()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            s,
        ))
    }

    /// `boundary_`: exactly `MAX_CONSECUTIVE_TRUNCATIONS` truncations in a row are
    /// tolerated — the model is nudged each time and its next, complete answer wins.
    #[tokio::test]
    async fn boundary_max_truncations_then_answer_recovers() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            truncated_turn("partial 1"),
            truncated_turn("partial 2"),
            truncated_turn("partial 3"),
            final_turn("done"),
        ]));
        let script = provider.clone();
        let out = agent_over(provider, 10).run("go").await.unwrap();
        assert_eq!(
            out, "done",
            "a truncated turn is a continuation, not the answer"
        );
        assert_eq!(
            script.calls(),
            (MAX_CONSECUTIVE_TRUNCATIONS + 1) as usize,
            "three nudged continuations, then the real answer"
        );
    }

    /// `corner_`: a model that truncates forever fails fast (an honest error the
    /// caller turns into a DNF) on the (MAX+1)th truncation — never silently
    /// returning a fragment, and never spinning to the far-higher iteration cap.
    #[tokio::test]
    async fn corner_persistent_truncation_fails_fast() {
        // ScriptedProvider repeats its last response, so this truncates every turn.
        let provider = Arc::new(ScriptedProvider::new(vec![truncated_turn("still cut off")]));
        let script = provider.clone();
        let err = agent_over(provider, 50).run("go").await.unwrap_err();
        assert!(
            err.to_string().contains("truncated"),
            "expected a truncation error, got: {err}"
        );
        assert_eq!(
            script.calls(),
            (MAX_CONSECUTIVE_TRUNCATIONS + 1) as usize,
            "bails on the 4th truncation, far below the 50-iteration ceiling"
        );
    }

    /// `positive_`: the streak is CONSECUTIVE — a productive (tool-call) turn resets
    /// it, so truncations either side of real work never sum to a false bail. Also
    /// covers Anthropic's `max_tokens` spelling.
    #[tokio::test]
    async fn positive_productive_turn_resets_truncation_streak() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            truncated_turn_with("a", "max_tokens"),
            truncated_turn_with("b", "max_tokens"),
            truncated_turn("c"),
            tool_turn(vec![tool_call("t0", "noop")]), // productive → streak resets
            truncated_turn("d"),
            truncated_turn("e"),
            truncated_turn("f"),
            final_turn("done"),
        ]));
        let out = agent_over(provider, 50).run("go").await.unwrap();
        assert_eq!(out, "done", "6 truncations but never 4 in a row → no bail");
    }

    // ---- tool timeout + panic isolation ------------------------------------

    /// A tool that never returns — stands in for a hung build / deadlocked call.
    struct HangTool;
    #[async_trait::async_trait]
    impl agent_core::Tool for HangTool {
        fn name(&self) -> &str {
            "hang"
        }
        fn schema(&self) -> agent_core::ToolSchema {
            agent_core::ToolSchema {
                name: "hang".into(),
                description: "never returns".into(),
                parameters: json!({"type": "object"}),
            }
        }
        async fn execute(
            &self,
            _a: serde_json::Value,
            _c: &agent_core::ToolContext,
        ) -> agent_core::Result<Observation> {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            Ok(Observation::ok("unreachable"))
        }
    }

    /// A tool that panics mid-execution — must be isolated, not crash the loop.
    struct PanicTool;
    #[async_trait::async_trait]
    impl agent_core::Tool for PanicTool {
        fn name(&self) -> &str {
            "panic"
        }
        fn schema(&self) -> agent_core::ToolSchema {
            agent_core::ToolSchema {
                name: "panic".into(),
                description: "panics".into(),
                parameters: json!({"type": "object"}),
            }
        }
        async fn execute(
            &self,
            _a: serde_json::Value,
            _c: &agent_core::ToolContext,
        ) -> agent_core::Result<Observation> {
            panic!("boom from a tool");
        }
    }

    /// A tool that takes a little while but does finish — used to prove the
    /// timeout-disabled (`timeout_secs == 0`) branch lets it complete.
    struct SlowTool;
    #[async_trait::async_trait]
    impl agent_core::Tool for SlowTool {
        fn name(&self) -> &str {
            "slow"
        }
        fn schema(&self) -> agent_core::ToolSchema {
            agent_core::ToolSchema {
                name: "slow".into(),
                description: "slow but finite".into(),
                parameters: json!({"type": "object"}),
            }
        }
        async fn execute(
            &self,
            _a: serde_json::Value,
            _c: &agent_core::ToolContext,
        ) -> agent_core::Result<Observation> {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            Ok(Observation::ok("finished"))
        }
    }

    #[tokio::test]
    async fn guard_times_out_a_hung_tool() {
        let obs = run_tool_guarded(Arc::new(HangTool), json!({}), std::env::temp_dir(), 1).await;
        assert!(obs.is_error);
        assert!(obs.content.contains("timed out"), "{}", obs.content);
    }

    #[tokio::test]
    async fn guard_disabled_timeout_lets_a_slow_tool_finish() {
        // `timeout_secs == 0` disables the loop-level timeout (the untested branch):
        // a slow-but-finite tool must complete, not be cut off.
        let obs = run_tool_guarded(Arc::new(SlowTool), json!({}), std::env::temp_dir(), 0).await;
        assert!(!obs.is_error, "{}", obs.content);
        assert!(obs.content.contains("finished"), "{}", obs.content);
    }

    #[tokio::test]
    async fn guard_isolates_a_panicking_tool() {
        let obs = run_tool_guarded(Arc::new(PanicTool), json!({}), std::env::temp_dir(), 5).await;
        assert!(obs.is_error);
        assert!(obs.content.contains("panicked"), "{}", obs.content);
    }

    #[tokio::test]
    async fn guard_passes_ok_and_err_through() {
        let cwd = std::env::temp_dir();
        let ok = run_tool_guarded(Arc::new(EchoTool), json!({"val": "hi"}), cwd.clone(), 5).await;
        assert!(!ok.is_error, "{}", ok.content);
        assert!(ok.content.contains("hi"));

        let err = run_tool_guarded(Arc::new(ErrTool), json!({}), cwd, 5).await;
        assert!(err.is_error);
        assert!(err.content.contains("tool errored"), "{}", err.content);
    }

    #[tokio::test]
    async fn loop_continues_after_a_tool_times_out() {
        // The model calls a hung tool, then answers. The loop must feed the timeout
        // back as an observation and keep going (not freeze), reaching the answer.
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(HangTool));
        let provider = ScriptedProvider::new(vec![
            tool_turn(vec![tool_call("t0", "hang")]),
            final_turn("recovered"),
        ]);
        let memory = RecordingMemory::new();
        let mut s = settings(false);
        s.tool_timeout_secs = 1; // fast timeout for the test
        let agent = Arc::new(Agent::new(
            Arc::new(provider),
            tools,
            Arc::new(memory.clone()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            s,
        ));

        let out = agent.run("go").await.unwrap();
        assert_eq!(out, "recovered", "loop should recover past the timeout");

        let tool_msgs: Vec<String> = memory
            .events()
            .into_iter()
            .filter(|e| e.kind == "tool")
            .map(|e| e.message.content_text())
            .collect();
        assert!(
            tool_msgs.iter().any(|c| c.contains("timed out")),
            "timeout not fed back: {tool_msgs:?}"
        );
    }

    /// Peak concurrent executions of three `conc` calls in one turn, given the
    /// tool's `parallel_safe` flag (with `parallel_tools = true`).
    async fn peak_concurrency(safe: bool) -> usize {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        let max = Arc::new(AtomicUsize::new(0));
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(ConcProbe {
            active: Arc::new(AtomicUsize::new(0)),
            max: max.clone(),
            safe,
        }));
        let provider = ScriptedProvider::new(vec![
            tool_turn(vec![
                tool_call("t0", "conc"),
                tool_call("t1", "conc"),
                tool_call("t2", "conc"),
            ]),
            final_turn("done"),
        ]);
        let agent = Arc::new(Agent::new(
            Arc::new(provider),
            tools,
            Arc::new(RecordingMemory::new()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            settings(true),
        ));
        assert_eq!(agent.run("go").await.unwrap(), "done");
        max.load(SeqCst)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn parallel_safe_tools_run_concurrently() {
        assert!(
            peak_concurrency(true).await >= 2,
            "parallel-safe tools should run concurrently"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn non_parallel_safe_tool_forces_sequential() {
        assert_eq!(
            peak_concurrency(false).await,
            1,
            "a non-parallel-safe tool must serialize the whole turn"
        );
    }
}

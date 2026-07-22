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
use agent_metrics::Metrics;
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
}

pub struct Agent {
    provider: Arc<dyn LlmProvider>,
    tools: ToolRegistry,
    memory: Arc<dyn MemoryStore>,
    context: Arc<dyn ContextStrategy>,
    policy: Arc<dyn Policy>,
    metrics: Metrics,
    settings: Settings,
    /// The composed search backend, if the `search` seam is wired. Held so it can
    /// be hosted over gRPC (`agent --serve-search`); the loop reaches search
    /// through the `search` *tool*, not this field.
    search: Option<Arc<dyn agent_core::SearchBackend>>,
    /// The git backend, if the `git` seam is wired. Held so it can be hosted over
    /// gRPC (`agent --serve-git`); the loop reaches it through the `git_*` tools.
    repo: Option<Arc<dyn agent_core::RepoBackend>>,
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
        Self {
            provider,
            tools,
            memory,
            context,
            policy,
            metrics,
            settings,
            search: None,
            repo: None,
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
        }
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

    /// Run a single goal to completion (one-shot): open a session and send it.
    pub async fn run(&self, goal: &str) -> anyhow::Result<String> {
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
    /// The git backend, if wired (for `agent --serve-git`).
    pub fn repo(&self) -> Option<Arc<dyn agent_core::RepoBackend>> {
        self.repo.clone()
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

    pub fn session(&self) -> Session<'_> {
        Session {
            agent: self,
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
        }
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
    async fn run_loop(
        &self,
        working: &mut WorkingSet,
        budget: &TokenBudget,
        tool_ctx: &ToolContext,
        tool_schemas: &[ToolSchema],
    ) -> anyhow::Result<String> {
        let model = self.settings.model.as_str();
        for iter in 1..=self.settings.max_iterations {
            self.metrics.on_iteration();
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
            };

            let call_start = Instant::now();
            let msg_count = working.messages.len();
            // `stream=true` uses the provider's incremental stream (with a live
            // echo); `stream=false` is the buffered path (an escape hatch for
            // servers that misbehave on SSE).
            let resp = if self.settings.stream {
                self.complete_streaming(req)
                    .instrument(tracing::info_span!("provider.stream", iter, model))
                    .await?
            } else {
                self.provider
                    .complete(req)
                    .instrument(tracing::info_span!("provider.complete", iter, model))
                    .await?
            };
            self.metrics.on_api_call(
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

            if let Some(u) = &resp.usage {
                tracing::info!(
                    iter,
                    finish = %resp.finish_reason,
                    tool_calls = assistant.tool_calls.len(),
                    prompt_tokens = u.prompt_tokens,
                    completion_tokens = u.completion_tokens,
                    "model turn"
                );
                self.metrics
                    .add_tokens(model, u.prompt_tokens as u64, u.completion_tokens as u64);
                self.metrics
                    .set_context(u.prompt_tokens as i64, msg_count as i64);
                // Prompt-cache token split (Anthropic/OpenAI report these) + USD cost
                // from the price table — the accounting half of the tokenizer seam.
                self.metrics.add_cache_tokens(
                    model,
                    u.cache_read_tokens as u64,
                    u.cache_write_tokens as u64,
                );
                #[cfg(feature = "tokenizer")]
                {
                    let prices = agent_tokenizer::PriceTable::builtin();
                    let (cost, _status) = agent_core::calculate_cost(model, u, &prices);
                    self.metrics.add_cost(
                        model,
                        cost.input,
                        cost.output,
                        cost.cache_read,
                        cost.cache_write,
                    );
                }
                self.record_usage(iter as u32, u).await;
            }

            // No tools requested → this is the final answer.
            if assistant.tool_calls.is_empty() {
                self.memory.distill().await.ok();
                return Ok(assistant.content_text());
            }

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

            let parallel = self.settings.parallel_tools
                && assistant
                    .tool_calls
                    .iter()
                    .all(|c| self.tools.get(&c.name).is_none_or(|t| t.parallel_safe()));

            let tool_timeout = self.settings.tool_timeout_secs;
            let futures = assistant
                .tool_calls
                .iter()
                .zip(&decisions)
                .map(|(call, dec)| {
                    let tools = &self.tools;
                    let cwd = tool_ctx.cwd.clone();
                    let span = tracing::info_span!("tool.execute", iter, tool = %call.name);
                    async move {
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
                let msg = match &decisions[i] {
                    Decision::Deny(reason) => {
                        self.metrics.on_tool(&call.name, "denied");
                        Message::tool(&call.id, format!("denied by policy: {reason}"))
                    }
                    Decision::Allow => {
                        let observation = observations[i]
                            .take()
                            .expect("allowed tool call has an observation");
                        self.metrics.on_tool(
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
                };
                working.messages.push(msg.clone());
                self.record("tool", msg).await;
            }

            // Keep the working set within budget before the next turn.
            let before = working.messages.len();
            let tokens_before = agent_context::bench_estimate_tokens(&working.messages);
            self.context
                .compact(working, budget)
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
        })
        .await;
    }

    async fn append_event(&self, event: MemoryEvent) {
        if let Err(e) = self.memory.append(event).await {
            tracing::warn!("episodic append failed: {e}");
        }
    }
}

/// A multi-turn conversation over an [`Agent`]. The working set (message history)
/// persists across [`Session::send`] calls, so follow-up turns continue the
/// conversation rather than starting fresh. The one-shot [`Agent::run`] is just a
/// session that sends a single message.
pub struct Session<'a> {
    agent: &'a Agent,
    working: WorkingSet,
    budget: TokenBudget,
    tool_ctx: ToolContext,
    tool_schemas: Vec<ToolSchema>,
    /// Whether the initial context (system prompt + recall) has been assembled or
    /// a saved transcript loaded.
    started: bool,
    /// Extra system context queued before the first turn (e.g. a loaded skill),
    /// injected once the initial context is assembled.
    pending_context: Vec<String>,
}

impl Session<'_> {
    /// Send a user message and run the loop to the next final answer. Each send
    /// is recorded as one metrics "run".
    pub async fn send(&mut self, input: &str) -> anyhow::Result<String> {
        self.agent.metrics.run_started();
        let start = Instant::now();
        // Root span of the run's trace tree; every seam interaction below nests
        // under it, and OTLP exports the whole tree to the collector.
        let goal: String = input.chars().take(80).collect();
        let span = tracing::info_span!("agent.turn", goal = %goal);
        let result = self.send_inner(input).instrument(span).await;
        // Checkpoint the completed turn, so `restore`/`branch`/`undo` have
        // something to work with (parity spec 19). Best-effort: a checkpoint
        // failure must not fail a turn that already succeeded.
        #[cfg(feature = "session")]
        if result.is_ok() && self.agent.auto_checkpoint {
            let label: String = input.chars().take(60).collect();
            if let Err(e) = self
                .agent
                .checkpoint(&self.agent.settings.session_id, &self.working, &label)
                .await
            {
                tracing::warn!(error = %e, "auto-checkpoint failed");
            }
        }
        let outcome = if result.is_ok() { "success" } else { "error" };
        self.agent
            .metrics
            .run_finished(outcome, start.elapsed().as_secs_f64());
        result
    }

    async fn send_inner(&mut self, input: &str) -> anyhow::Result<String> {
        // Expand the prompt's `@file`/`@dir`/`@symbol`/`@url` mentions into
        // context blocks BEFORE assembly, so the model sees the exact bytes the
        // user pointed at (parity spec 17). Resolution never errors: an
        // unresolved or denied reference degrades to a warning and the turn runs.
        #[cfg(feature = "reference")]
        let expanded: Vec<ContextBlock> = {
            let res = self.agent.resolve_references(input).await;
            for w in &res.warnings {
                tracing::info!(warning = %w, "reference not expanded");
            }
            if res.blocked {
                tracing::warn!(
                    "reference expansion exceeded its token budget; the prompt is unmodified"
                );
            }
            res.blocks
        };
        #[cfg(not(feature = "reference"))]
        let expanded: Vec<ContextBlock> = Vec::new();

        if !self.started {
            // First turn: recall relevant memory and assemble the initial context.
            let recall_span = tracing::info_span!("memory.recall", items = tracing::field::Empty);
            let recalled = async {
                let out = self
                    .agent
                    .memory
                    .recall(&RecallQuery {
                        text: input.to_string(),
                        limit: self.agent.settings.recall_limit,
                    })
                    .await;
                if let Ok(items) = &out {
                    tracing::Span::current().record("items", items.len());
                }
                out
            }
            .instrument(recall_span)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("recall failed: {e}");
                Vec::new()
            });
            if !recalled.is_empty() {
                tracing::info!(items = recalled.len(), "recalled memory");
            }
            self.working.messages = self
                .agent
                .context
                .assemble(ContextInput {
                    system_prompt: self.agent.settings.system_prompt.clone(),
                    prepend: {
                        let mut p = self.agent.settings.context_prepend.clone();
                        p.extend(expanded.iter().cloned());
                        p
                    },
                    recalled,
                    goal: input.to_string(),
                    append: self.agent.settings.context_append.clone(),
                })
                .instrument(tracing::info_span!("context.assemble"))
                .await?;
            // Inject any context queued before the first turn (e.g. skills).
            for ctx in self.pending_context.drain(..) {
                self.working.messages.push(Message::system(ctx));
            }
            self.started = true;
        } else {
            // Continuation: assembly already happened, so expanded references are
            // injected as system context ahead of the new user message.
            for b in &expanded {
                self.working
                    .messages
                    .push(Message::system(format!("## {}\n{}", b.source, b.content)));
            }
            self.working.messages.push(Message::user(input));
        }

        self.agent.record("goal", Message::user(input)).await;
        self.agent
            .run_loop(
                &mut self.working,
                &self.budget,
                &self.tool_ctx,
                &self.tool_schemas,
            )
            .await
    }

    /// The current message history (for persistence / resume).
    pub fn messages(&self) -> &[Message] {
        &self.working.messages
    }

    /// Replace the working set with a saved transcript (resume).
    pub fn load(&mut self, messages: Vec<Message>) {
        self.working.messages = messages;
        self.started = true;
    }

    /// Add a system-context block (e.g. a loaded skill body). Applied immediately
    /// if the conversation has started, otherwise queued for the first turn.
    pub fn add_context(&mut self, text: String) {
        if self.started {
            self.working.messages.push(Message::system(text));
        } else {
            self.pending_context.push(text);
        }
    }

    /// Whether any turn has run (or a transcript was loaded).
    pub fn is_started(&self) -> bool {
        self.started
    }

    /// Force a compaction pass on the working set now (e.g. a `/compact` command).
    pub async fn compact(&mut self) -> anyhow::Result<()> {
        self.agent
            .context
            .compact(&mut self.working, &self.budget)
            .await?;
        Ok(())
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Total bytes an observation carries — its text plus any media payload. Used
/// for telemetry so an image-bearing result isn't reported as near-zero.
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
    use agent_core::ToolCall;
    use agent_testkit::{
        final_turn, tool_turn, EchoTool, FnProvider, RecordingMemory, ScriptedProvider,
        StaticContext,
    };
    use rstest::rstest;
    use serde_json::json;

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
        }
    }

    async fn run_with(parallel: bool) -> Vec<String> {
        let memory = RecordingMemory::new();
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(EchoTool));
        let agent = Agent::new(
            Arc::new(seq_provider()),
            tools,
            Arc::new(memory.clone()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            settings(parallel),
        );
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
        let agent = Agent::new(
            Arc::new(provider),
            tools,
            Arc::new(memory.clone()),
            Arc::new(StaticContext),
            Arc::new(DenyNamed("echo")),
            Metrics::new(),
            settings(false),
        );
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
        let agent = Agent::new(
            Arc::new(counting),
            ToolRegistry::new(),
            Arc::new(RecordingMemory::new()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            settings(false),
        );
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
        let agent = Agent::new(
            Arc::new(provider),
            tools,
            Arc::new(memory.clone()),
            Arc::new(StaticContext),
            policy,
            Metrics::new(),
            settings(false),
        );
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
        assert!(events[0].1.contains("unknown tool `nope`"), "{:?}", events);
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
            "{:?}",
            events
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
        let agent = Agent::new(
            Arc::new(provider),
            tools,
            Arc::new(RecordingMemory::new()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            s,
        );
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
        let agent = Agent::new(
            Arc::new(provider),
            tools,
            Arc::new(memory.clone()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            s,
        );

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
        let agent = Agent::new(
            Arc::new(provider),
            tools,
            Arc::new(RecordingMemory::new()),
            Arc::new(StaticContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            settings(true),
        );
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

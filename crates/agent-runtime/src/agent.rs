//! The agent loop — the one place that ties the seams together.
//!
//! It depends only on the `agent-core` traits; every concrete component was
//! chosen by the factory in `builder.rs`. The loop shape is deliberately
//! ordinary (DESIGN.md §2): assemble → complete → dispatch tools → record →
//! compact → repeat until the model stops asking for tools.

use agent_core::{
    CompletionRequest, CompletionResponse, ContextBlock, ContextInput, ContextStrategy, Decision,
    LlmProvider, MemoryEvent, MemoryStore, Message, Observation, Policy, RecallQuery, Role,
    TokenBudget, ToolContext, ToolRegistry, ToolSchema, WorkingSet,
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
        }
    }

    /// Attach the composed search backend (so `--serve-search` can host it).
    pub fn with_search(mut self, search: Arc<dyn agent_core::SearchBackend>) -> Self {
        self.search = Some(search);
        self
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
            let req = CompletionRequest {
                messages: working.messages.clone(),
                tools: tool_schemas.to_vec(),
                max_tokens: self.settings.max_tokens,
                temperature: self.settings.temperature,
            };

            let call_start = Instant::now();
            let msg_count = working.messages.len();
            // `stream=true` uses the provider's incremental stream (with a live
            // echo); `stream=false` is the buffered path (an escape hatch for
            // servers that misbehave on SSE).
            let resp = if self.settings.stream {
                self.complete_streaming(req)
                    .instrument(tracing::info_span!("provider.stream", iter))
                    .await?
            } else {
                self.provider
                    .complete(req)
                    .instrument(tracing::info_span!("provider.complete", iter))
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
                self.record_usage(iter as u32, u).await;
            }

            // No tools requested → this is the final answer.
            if assistant.tool_calls.is_empty() {
                self.memory.distill().await.ok();
                return Ok(assistant.content);
            }

            // Dispatch the requested tool calls. Authorization runs sequentially
            // (interactive prompts must not interleave); execution runs
            // concurrently when enabled and every requested tool is parallel-safe.
            // Results are appended in original call order for a deterministic
            // transcript.
            let mut decisions = Vec::with_capacity(assistant.tool_calls.len());
            for call in &assistant.tool_calls {
                decisions.push(
                    self.policy
                        .authorize(call)
                        .instrument(
                            tracing::info_span!("policy.authorize", iter, tool = %call.name),
                        )
                        .await,
                );
            }

            let parallel = self.settings.parallel_tools
                && assistant
                    .tool_calls
                    .iter()
                    .all(|c| self.tools.get(&c.name).is_none_or(|t| t.parallel_safe()));

            let futures = assistant
                .tool_calls
                .iter()
                .zip(&decisions)
                .map(|(call, dec)| {
                    let tools = &self.tools;
                    let tool_ctx: &ToolContext = tool_ctx;
                    let span = tracing::info_span!("tool.execute", iter, tool = %call.name);
                    async move {
                        match dec {
                            Decision::Deny(_) => None,
                            Decision::Allow => Some(match tools.get(&call.name) {
                                Some(tool) => {
                                    match tool.execute(call.arguments.clone(), tool_ctx).await {
                                        Ok(obs) => obs,
                                        Err(e) => Observation::error(format!("tool errored: {e}")),
                                    }
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
                            bytes = observation.content.len(),
                            "tool result"
                        );
                        Message::tool(&call.id, observation.content)
                    }
                };
                working.messages.push(msg.clone());
                self.record("tool", msg).await;
            }

            // Keep the working set within budget before the next turn.
            self.context
                .compact(working, budget)
                .instrument(tracing::info_span!("context.compact", iter))
                .await?;
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
                content,
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
        let outcome = if result.is_ok() { "success" } else { "error" };
        self.agent
            .metrics
            .run_finished(outcome, start.elapsed().as_secs_f64());
        result
    }

    async fn send_inner(&mut self, input: &str) -> anyhow::Result<String> {
        if !self.started {
            // First turn: recall relevant memory and assemble the initial context.
            let recalled = self
                .agent
                .memory
                .recall(&RecallQuery {
                    text: input.to_string(),
                    limit: self.agent.settings.recall_limit,
                })
                .instrument(tracing::info_span!("memory.recall"))
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
                    prepend: self.agent.settings.context_prepend.clone(),
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
            // Continuation: append the new user message to the running history.
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ToolCall;
    use agent_testkit::{
        final_turn, tool_turn, EchoTool, FnProvider, RecordingMemory, ScriptedProvider,
        StaticContext,
    };
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
}

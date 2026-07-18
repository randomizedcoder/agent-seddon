//! The agent loop — the one place that ties the seams together.
//!
//! It depends only on the `agent-core` traits; every concrete component was
//! chosen by the factory in `builder.rs`. The loop shape is deliberately
//! ordinary (DESIGN.md §2): assemble → complete → dispatch tools → record →
//! compact → repeat until the model stops asking for tools.

use crate::metrics::Metrics;
use agent_core::{
    CompletionRequest, CompletionResponse, ContextBlock, ContextInput, ContextStrategy, Decision,
    LlmProvider, MemoryEvent, MemoryStore, Message, Observation, Policy, RecallQuery, Role,
    TokenBudget, ToolContext, ToolRegistry, WorkingSet,
};
use futures_util::StreamExt;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

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
        }
    }

    /// Run the loop to completion, returning the model's final text answer.
    /// Wraps `run_inner` to record run outcome/duration metrics on every path.
    pub async fn run(&self, goal: &str) -> anyhow::Result<String> {
        self.metrics.run_started();
        let start = Instant::now();
        let result = self.run_inner(goal).await;
        let outcome = if result.is_ok() { "success" } else { "error" };
        self.metrics
            .run_finished(outcome, start.elapsed().as_secs_f64());
        result
    }

    async fn run_inner(&self, goal: &str) -> anyhow::Result<String> {
        // 1. Recall relevant memory for the goal.
        let recalled = self
            .memory
            .recall(&RecallQuery {
                text: goal.to_string(),
                limit: self.settings.recall_limit,
            })
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("recall failed: {e}");
                Vec::new()
            });
        if !recalled.is_empty() {
            tracing::info!(items = recalled.len(), "recalled memory");
        }

        // 2. Assemble the initial working set.
        let messages = self
            .context
            .assemble(ContextInput {
                system_prompt: self.settings.system_prompt.clone(),
                prepend: self.settings.context_prepend.clone(),
                recalled,
                goal: goal.to_string(),
                append: self.settings.context_append.clone(),
            })
            .await?;
        let mut working = WorkingSet { messages };

        self.record("goal", Message::user(goal)).await;

        let budget = TokenBudget {
            max_context_tokens: self.settings.context_window,
            reserve_output: self.settings.reserve_output,
        };
        let tool_ctx = ToolContext {
            cwd: self.settings.cwd.clone(),
        };
        let tool_schemas = self.tools.describe_all();

        // 3. The loop.
        let model = self.settings.model.as_str();
        for iter in 1..=self.settings.max_iterations {
            self.metrics.on_iteration();
            let req = CompletionRequest {
                messages: working.messages.clone(),
                tools: tool_schemas.clone(),
                max_tokens: self.settings.max_tokens,
                temperature: self.settings.temperature,
            };

            let call_start = Instant::now();
            let msg_count = working.messages.len();
            // `stream=true` uses the provider's incremental stream (with a live
            // echo); `stream=false` is the buffered path (an escape hatch for
            // servers that misbehave on SSE).
            let resp = if self.settings.stream {
                self.complete_streaming(req).await?
            } else {
                self.provider.complete(req).await?
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
                decisions.push(self.policy.authorize(call).await);
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
                    let tool_ctx = &tool_ctx;
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
            self.context.compact(&mut working, &budget).await?;
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

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{
        CompletionResponse, MemoryItem, ModelCapabilities, Tool, ToolCall, ToolSchema,
    };
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// Emits three tool calls on the first turn, then a final answer.
    struct SeqProvider {
        calls: AtomicUsize,
    }
    #[async_trait]
    impl LlmProvider for SeqProvider {
        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities {
                supports_tools: true,
                context_window: 1000,
            }
        }
        async fn complete(
            &self,
            _req: CompletionRequest,
        ) -> agent_core::Result<CompletionResponse> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                let tool_calls = vec![
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
                ];
                Ok(CompletionResponse {
                    message: Message {
                        role: Role::Assistant,
                        content: String::new(),
                        tool_calls,
                        tool_call_id: None,
                    },
                    finish_reason: "tool_calls".into(),
                    usage: None,
                })
            } else {
                Ok(CompletionResponse {
                    message: Message::assistant("done"),
                    finish_reason: "stop".into(),
                    usage: None,
                })
            }
        }
    }

    /// Sleeps `sleep_ms` then echoes `val`, so completion order differs from call
    /// order (t0 sleeps longest yet is requested first).
    struct EchoTool;
    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "echo".into(),
                description: "test".into(),
                parameters: json!({"type": "object"}),
            }
        }
        async fn execute(
            &self,
            args: serde_json::Value,
            _ctx: &ToolContext,
        ) -> agent_core::Result<Observation> {
            let ms = args.get("sleep_ms").and_then(|v| v.as_u64()).unwrap_or(0);
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            Ok(Observation::ok(
                args.get("val")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            ))
        }
    }

    /// Records the `tool_call_id` of every appended tool event, in order.
    struct RecordMemory {
        tool_order: Arc<Mutex<Vec<String>>>,
    }
    #[async_trait]
    impl MemoryStore for RecordMemory {
        async fn recall(&self, _q: &RecallQuery) -> agent_core::Result<Vec<MemoryItem>> {
            Ok(vec![])
        }
        async fn append(&self, e: MemoryEvent) -> agent_core::Result<()> {
            if e.kind == "tool" {
                if let Some(id) = e.message.tool_call_id {
                    self.tool_order.lock().unwrap().push(id);
                }
            }
            Ok(())
        }
        async fn distill(&self) -> agent_core::Result<usize> {
            Ok(0)
        }
    }

    struct MockContext;
    #[async_trait]
    impl ContextStrategy for MockContext {
        async fn assemble(&self, input: ContextInput) -> agent_core::Result<Vec<Message>> {
            Ok(vec![
                Message::system(input.system_prompt),
                Message::user(input.goal),
            ])
        }
        async fn compact(&self, _w: &mut WorkingSet, _b: &TokenBudget) -> agent_core::Result<()> {
            Ok(())
        }
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
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(EchoTool));
        let agent = Agent::new(
            Arc::new(SeqProvider {
                calls: AtomicUsize::new(0),
            }),
            tools,
            Arc::new(RecordMemory {
                tool_order: order.clone(),
            }),
            Arc::new(MockContext),
            Arc::new(crate::policy::AutoApprove),
            Metrics::new(),
            settings(parallel),
        );
        let out = agent.run("go").await.unwrap();
        assert_eq!(out, "done");
        let v = order.lock().unwrap().clone();
        v
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
}

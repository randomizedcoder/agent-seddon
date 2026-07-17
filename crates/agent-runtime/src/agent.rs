//! The agent loop — the one place that ties the seams together.
//!
//! It depends only on the `agent-core` traits; every concrete component was
//! chosen by the factory in `builder.rs`. The loop shape is deliberately
//! ordinary (DESIGN.md §2): assemble → complete → dispatch tools → record →
//! compact → repeat until the model stops asking for tools.

use agent_core::{
    CompletionRequest, ContextInput, ContextStrategy, Decision, LlmProvider, MemoryEvent,
    MemoryStore, Message, Policy, RecallQuery, TokenBudget, ToolContext, ToolRegistry, WorkingSet,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Settings {
    pub max_iterations: usize,
    pub max_tokens: u32,
    pub temperature: f32,
    pub context_window: u32,
    pub reserve_output: u32,
    pub system_prompt: String,
    pub recall_limit: usize,
    pub cwd: PathBuf,
    /// Per-run id, stamped on every recorded event (empty when telemetry is off).
    pub session_id: String,
}

pub struct Agent {
    provider: Arc<dyn LlmProvider>,
    tools: ToolRegistry,
    memory: Arc<dyn MemoryStore>,
    context: Arc<dyn ContextStrategy>,
    policy: Arc<dyn Policy>,
    settings: Settings,
}

impl Agent {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        tools: ToolRegistry,
        memory: Arc<dyn MemoryStore>,
        context: Arc<dyn ContextStrategy>,
        policy: Arc<dyn Policy>,
        settings: Settings,
    ) -> Self {
        Self {
            provider,
            tools,
            memory,
            context,
            policy,
            settings,
        }
    }

    /// Run the loop to completion, returning the model's final text answer.
    pub async fn run(&self, goal: &str) -> anyhow::Result<String> {
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
                recalled,
                goal: goal.to_string(),
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
        for iter in 1..=self.settings.max_iterations {
            let req = CompletionRequest {
                messages: working.messages.clone(),
                tools: tool_schemas.clone(),
                max_tokens: self.settings.max_tokens,
                temperature: self.settings.temperature,
            };

            let resp = self.provider.complete(req).await?;
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
                self.record_usage(iter as u32, u).await;
            }

            // No tools requested → this is the final answer.
            if assistant.tool_calls.is_empty() {
                self.memory.distill().await.ok();
                return Ok(assistant.content);
            }

            // Dispatch each requested tool call.
            for call in &assistant.tool_calls {
                match self.policy.authorize(call).await {
                    Decision::Allow => {}
                    Decision::Deny(reason) => {
                        let msg = Message::tool(&call.id, format!("denied by policy: {reason}"));
                        working.messages.push(msg.clone());
                        self.record("tool", msg).await;
                        continue;
                    }
                }

                let observation = match self.tools.get(&call.name) {
                    Some(tool) => match tool.execute(call.arguments.clone(), &tool_ctx).await {
                        Ok(obs) => obs,
                        Err(e) => agent_core::Observation::error(format!("tool errored: {e}")),
                    },
                    None => agent_core::Observation::error(format!("unknown tool `{}`", call.name)),
                };

                tracing::info!(
                    tool = %call.name,
                    is_error = observation.is_error,
                    bytes = observation.content.len(),
                    "tool result"
                );

                let msg = Message::tool(&call.id, observation.content);
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

//! Subagent delegation — the `delegate` tool.
//!
//! `delegate` lets the model hand a self-contained sub-task to a **child agent**
//! that runs its own tool loop in an isolated context and returns only its final
//! summary (the "boomerang" pattern from DESIGN.md §4.5). This keeps large
//! subtasks from polluting the parent's context.
//!
//! A `DelegateTool` carries the shared [`SubagentContext`] plus its current
//! depth. When it runs, it builds a child agent from that context; if another
//! level of delegation is still allowed, the child's tool set includes a
//! `delegate` at `depth + 1`, so recursion is bounded by `max_depth`.

use crate::agent::{Agent, Settings};
use agent_core::{
    ContextStrategy, LlmProvider, MemoryStore, Observation, Policy, Result, Tool, ToolContext,
    ToolRegistry, ToolSchema,
};
use agent_metrics::Metrics;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::Instrument;

/// Everything needed to build a child agent, shared (via `Arc`) across every
/// `DelegateTool` in a run.
pub(crate) struct SubagentContext {
    pub provider: Arc<dyn LlmProvider>,
    pub context: Arc<dyn ContextStrategy>,
    pub policy: Arc<dyn Policy>,
    pub memory: Arc<dyn MemoryStore>,
    /// Tools available to children (the worker set — never includes `delegate`).
    pub worker_tools: ToolRegistry,
    pub metrics: Metrics,
    pub max_depth: usize,
    /// Settings template for children (system prompt gets a sub-agent note).
    pub child_settings: Settings,
}

pub(crate) struct DelegateTool {
    ctx: Arc<SubagentContext>,
    depth: usize,
}

impl DelegateTool {
    /// The parent-level delegate tool (depth 0).
    pub(crate) fn root(ctx: Arc<SubagentContext>) -> Self {
        Self { ctx, depth: 0 }
    }
}

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> &str {
        "delegate"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "delegate".into(),
            description:
                "Delegate a self-contained sub-task to a child agent with an isolated \
                          context. The child runs its own tool loop and returns only its final \
                          summary — use it to keep your own context clean for well-scoped subtasks."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "goal": { "type": "string", "description": "The sub-task for the child agent." },
                    "tools": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional subset of tool names the child may use (default: all)."
                    }
                },
                "required": ["goal"]
            }),
        }
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let Some(goal) = args.get("goal").and_then(Value::as_str) else {
            return Ok(Observation::error(
                "delegate: missing string argument `goal`",
            ));
        };

        // Child tools: the requested subset (or all worker tools) …
        let requested: Option<Vec<String>> = args.get("tools").and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        });
        let mut child_tools = match requested {
            Some(names) => {
                let mut r = ToolRegistry::new();
                for name in names {
                    if let Some(t) = self.ctx.worker_tools.get(&name) {
                        r.register(t);
                    }
                }
                r
            }
            None => self.ctx.worker_tools.clone(),
        };
        // … plus a deeper `delegate`, while depth remains.
        if self.depth + 1 < self.ctx.max_depth {
            child_tools.register(Arc::new(DelegateTool {
                ctx: self.ctx.clone(),
                depth: self.depth + 1,
            }));
        }

        let mut settings = self.ctx.child_settings.clone();
        settings.system_prompt = format!(
            "{}\n\nYou are a sub-agent handling a single delegated sub-task. Complete it, then \
             reply with a concise summary of the outcome and stop.",
            settings.system_prompt
        );

        let child = Agent::new(
            self.ctx.provider.clone(),
            child_tools,
            self.ctx.memory.clone(),
            self.ctx.context.clone(),
            self.ctx.policy.clone(),
            self.ctx.metrics.clone(),
            settings,
        );

        tracing::info!(depth = self.depth + 1, goal, "delegating sub-task");
        let span = tracing::info_span!("agent.delegate", depth = self.depth + 1);
        match child.run(goal).instrument(span).await {
            Ok(answer) => Ok(Observation::ok(answer)),
            Err(e) => Ok(Observation::error(format!("subagent failed: {e}"))),
        }
    }

    /// Delegation serializes child loops; keep it off the parallel path.
    fn parallel_safe(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ToolCall;
    use agent_testkit::{final_turn, tool_turn, RecordingMemory, ScriptedProvider, StaticContext};
    use serde_json::json;

    /// call 0 → parent asks to delegate; call 1 → child's final answer;
    /// call 2 → parent's final answer. Shared across parent + child (same Arc), so
    /// the scripted sequence spans both loops.
    fn delegating_provider() -> ScriptedProvider {
        ScriptedProvider::new(vec![
            tool_turn(vec![ToolCall {
                id: "d0".into(),
                name: "delegate".into(),
                arguments: json!({ "goal": "sub-task" }),
            }]),
            final_turn("child-done"),
            final_turn("parent-done"),
        ])
    }

    fn settings() -> Settings {
        Settings {
            max_iterations: 5,
            max_tokens: 100,
            temperature: 0.0,
            context_window: 100_000,
            reserve_output: 1000,
            system_prompt: "sys".into(),
            stream: false,
            parallel_tools: true,
            tool_timeout_secs: 30,
            recall_limit: 0,
            cwd: std::env::temp_dir(),
            model: "m".into(),
            session_id: String::new(),
            context_prepend: vec![],
            context_append: vec![],
        }
    }

    #[tokio::test]
    async fn delegate_runs_a_child_loop_and_returns_to_parent() {
        let provider: Arc<dyn LlmProvider> = Arc::new(delegating_provider());
        let context: Arc<dyn ContextStrategy> = Arc::new(StaticContext);
        let policy: Arc<dyn Policy> = Arc::new(crate::policy::AutoApprove);
        let memory: Arc<dyn MemoryStore> = Arc::new(RecordingMemory::new());

        let ctx = Arc::new(SubagentContext {
            provider: provider.clone(),
            context: context.clone(),
            policy: policy.clone(),
            memory: memory.clone(),
            worker_tools: ToolRegistry::new(),
            metrics: Metrics::new(),
            max_depth: 2,
            child_settings: settings(),
        });
        let mut parent_tools = ToolRegistry::new();
        parent_tools.register(Arc::new(DelegateTool::root(ctx)));

        let parent = Agent::new(
            provider.clone(),
            parent_tools,
            memory,
            context,
            policy,
            Metrics::new(),
            settings(),
        );

        // parent delegates -> child returns "child-done" -> parent returns
        // "parent-done" (only reachable if the child loop actually ran).
        let out = parent.run("do it").await.unwrap();
        assert_eq!(out, "parent-done");
    }
}

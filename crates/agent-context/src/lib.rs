//! Context assembly / compaction behind the `ContextStrategy` seam.
//!
//! `SlidingWindow` builds the initial `[system, user]` message list (folding
//! recalled memory into the system prompt) and, when the working set grows past
//! the token budget, drops the oldest turns. Trimming is non-destructive w.r.t.
//! episodic memory — it only shortens the live window. A summarizing compactor
//! is a future strategy behind the same trait (see DESIGN.md §4.4).

use agent_core::{ContextInput, ContextStrategy, Message, Result, Role, TokenBudget, WorkingSet};
use async_trait::async_trait;

pub struct SlidingWindow;

/// Very rough token estimate: ~4 chars per token, plus a small per-message tax.
fn estimate_tokens(messages: &[Message]) -> u32 {
    let mut chars = 0usize;
    for m in messages {
        chars += m.content.len();
        for tc in &m.tool_calls {
            chars += tc.name.len() + tc.arguments.to_string().len();
        }
        chars += 8; // role/formatting overhead
    }
    (chars / 4) as u32
}

#[async_trait]
impl ContextStrategy for SlidingWindow {
    async fn assemble(&self, input: ContextInput) -> Result<Vec<Message>> {
        let mut system = input.system_prompt;
        if !input.recalled.is_empty() {
            system.push_str("\n\n## Recalled memory\n");
            system.push_str("The following may be relevant to this task:\n");
            for item in &input.recalled {
                system.push_str(&format!("\n### {}\n{}\n", item.source, item.content));
            }
        }
        Ok(vec![Message::system(system), Message::user(input.goal)])
    }

    async fn compact(&self, working: &mut WorkingSet, budget: &TokenBudget) -> Result<()> {
        let target = budget
            .max_context_tokens
            .saturating_sub(budget.reserve_output);

        // Drop the oldest non-system message until we fit (or can't trim more).
        while estimate_tokens(&working.messages) > target {
            let victim = working.messages.iter().position(|m| m.role != Role::System);
            match victim {
                Some(idx) if working.messages.len() > 2 => {
                    working.messages.remove(idx);
                }
                _ => break,
            }
        }

        // Never let a `tool` message be the first non-system message: the API
        // rejects a tool result with no preceding assistant tool_call.
        while let Some(idx) = working.messages.iter().position(|m| m.role != Role::System) {
            if working.messages[idx].role == Role::Tool {
                working.messages.remove(idx);
            } else {
                break;
            }
        }

        tracing::debug!(
            estimated_tokens = estimate_tokens(&working.messages),
            target,
            "compacted working set"
        );
        Ok(())
    }
}

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

        // User "prepend" context (context.d/prepend/*.md) — always injected,
        // ahead of recalled memory.
        for block in &input.prepend {
            system.push_str(&format!("\n\n## {}\n{}", block.source, block.content));
        }

        if !input.recalled.is_empty() {
            system.push_str("\n\n## Recalled memory\n");
            system.push_str("The following may be relevant to this task:\n");
            for item in &input.recalled {
                system.push_str(&format!("\n### {}\n{}\n", item.source, item.content));
            }
        }

        let mut messages = vec![Message::system(system), Message::user(input.goal)];

        // User "append" context (context.d/append/*.md) — a trailing system
        // message after the goal.
        if !input.append.is_empty() {
            let mut tail = String::new();
            for block in &input.append {
                tail.push_str(&format!("## {}\n{}\n\n", block.source, block.content));
            }
            messages.push(Message::system(tail.trim_end().to_string()));
        }

        Ok(messages)
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{ContextBlock, Role};

    #[tokio::test]
    async fn assemble_places_prepend_and_append() {
        let input = ContextInput {
            system_prompt: "BASE".into(),
            prepend: vec![ContextBlock {
                source: "0001_pre.md".into(),
                content: "PRE-CONTENT".into(),
            }],
            recalled: vec![],
            goal: "do the thing".into(),
            append: vec![ContextBlock {
                source: "0010_post.md".into(),
                content: "POST-CONTENT".into(),
            }],
        };
        let msgs = SlidingWindow.assemble(input).await.unwrap();

        // [ system(base+prepend), user(goal), system(append) ]
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, Role::System);
        assert!(msgs[0].content.contains("BASE"));
        assert!(msgs[0].content.contains("PRE-CONTENT"));
        assert_eq!(msgs[1].role, Role::User);
        assert_eq!(msgs[1].content, "do the thing");
        assert_eq!(msgs[2].role, Role::System);
        assert!(msgs[2].content.contains("POST-CONTENT"));
    }

    #[tokio::test]
    async fn assemble_without_append_has_two_messages() {
        let input = ContextInput {
            system_prompt: "BASE".into(),
            prepend: vec![],
            recalled: vec![],
            goal: "g".into(),
            append: vec![],
        };
        let msgs = SlidingWindow.assemble(input).await.unwrap();
        assert_eq!(msgs.len(), 2);
    }
}

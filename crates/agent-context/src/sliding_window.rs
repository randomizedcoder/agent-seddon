//! `context-sliding-window` — assemble the message list and, when the working
//! set grows past the token budget, drop the oldest turns.
//!
//! Trimming is non-destructive w.r.t. episodic memory — it only shortens the live
//! window. For a lossy-free alternative that summarizes instead of dropping, see
//! `context-summarizing`.

use crate::{assemble_messages, estimate_tokens};
use agent_core::{ContextInput, ContextStrategy, Message, Result, Role, TokenBudget, WorkingSet};
use async_trait::async_trait;

pub struct SlidingWindow;

#[async_trait]
impl ContextStrategy for SlidingWindow {
    async fn assemble(&self, input: ContextInput) -> Result<Vec<Message>> {
        Ok(assemble_messages(input))
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

    // --- compact: drop-oldest under budget ---------------------------------

    fn long(role: Role, n: usize) -> Message {
        let c = "x ".repeat(n);
        match role {
            Role::System => Message::system(c),
            Role::User => Message::user(c),
            Role::Assistant => Message::assistant(c),
            Role::Tool => Message::tool("id", c),
        }
    }

    #[tokio::test]
    async fn compact_drops_oldest_until_under_budget() {
        let mut w = WorkingSet {
            messages: vec![
                long(Role::System, 10),
                long(Role::User, 400),
                long(Role::Assistant, 400),
                long(Role::User, 400),
                long(Role::Assistant, 20),
            ],
        };
        let budget = TokenBudget {
            max_context_tokens: 400,
            reserve_output: 100,
        }; // target 300
        SlidingWindow.compact(&mut w, &budget).await.unwrap();
        assert!(estimate_tokens(&w.messages) <= 300, "still over target");
        assert_eq!(w.messages[0].role, Role::System, "system head kept");
        assert!(w.messages.len() < 5, "some turns were dropped");
    }

    #[tokio::test]
    async fn compact_no_op_under_budget() {
        let mut w = WorkingSet {
            messages: vec![Message::system("s"), Message::user("hi")],
        };
        let budget = TokenBudget {
            max_context_tokens: 100_000,
            reserve_output: 1000,
        };
        SlidingWindow.compact(&mut w, &budget).await.unwrap();
        assert_eq!(w.messages.len(), 2);
    }

    #[tokio::test]
    async fn compact_keeps_at_least_two_messages() {
        // Even an impossibly small budget can't trim below two messages.
        let mut w = WorkingSet {
            messages: vec![
                long(Role::System, 100),
                long(Role::User, 100),
                long(Role::Assistant, 100),
            ],
        };
        let budget = TokenBudget {
            max_context_tokens: 1,
            reserve_output: 0,
        };
        SlidingWindow.compact(&mut w, &budget).await.unwrap();
        assert!(w.messages.len() >= 2);
    }

    #[tokio::test]
    async fn compact_removes_leading_orphan_tool() {
        // A tool result must never be the first non-system message (the API rejects
        // a tool_result with no preceding assistant tool_call).
        let mut w = WorkingSet {
            messages: vec![
                Message::system("s"),
                Message::tool("id", "orphan"),
                Message::assistant("a"),
            ],
        };
        let budget = TokenBudget {
            max_context_tokens: 100_000,
            reserve_output: 0,
        };
        SlidingWindow.compact(&mut w, &budget).await.unwrap();
        assert_eq!(w.messages[0].role, Role::System);
        assert_eq!(w.messages[1].role, Role::Assistant, "orphan tool removed");
    }
}

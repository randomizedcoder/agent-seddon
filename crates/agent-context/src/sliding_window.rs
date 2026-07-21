//! `context-sliding-window` — assemble the message list and, when the working
//! set grows past the token budget, drop the oldest turns.
//!
//! Trimming is non-destructive w.r.t. episodic memory — it only shortens the live
//! window. For a lossy-free alternative that summarizes instead of dropping, see
//! `context-summarizing`.

use crate::{assemble_messages, estimate_tokens};
use agent_core::{
    ContextInput, ContextStrategy, Message, Result, Role, TokenBudget, Tokenizer, WorkingSet,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Drop-oldest compaction. Holds an optional [`Tokenizer`]: when present, the
/// budget boundary is computed from the target model's **real** token count
/// (parity spec 23); when absent it falls back to the crate-private `~chars/4`
/// [`estimate_tokens`] heuristic — the only count available before this seam.
#[derive(Default)]
pub struct SlidingWindow {
    tokenizer: Option<Arc<dyn Tokenizer>>,
    /// The model whose tokenization the budget is measured in (unused by the
    /// heuristic fallback).
    model: String,
}

impl SlidingWindow {
    /// Accurate-count variant: budget the working set with `tokenizer` for `model`.
    /// A `None` tokenizer degrades to the heuristic.
    pub fn new(tokenizer: Option<Arc<dyn Tokenizer>>, model: impl Into<String>) -> Self {
        Self {
            tokenizer,
            model: model.into(),
        }
    }

    /// Heuristic-only variant (no tokenizer) — the pre-spec-23 behaviour, handy in
    /// tests that don't exercise the seam.
    pub fn heuristic() -> Self {
        Self::default()
    }

    /// Tokens in `messages` under the configured tokenizer, or the heuristic when
    /// none is set (or the tokenizer errors — budgeting must never hard-fail).
    async fn budget_tokens(&self, messages: &[Message]) -> u32 {
        match &self.tokenizer {
            Some(t) => t
                .count_messages(messages, &self.model)
                .await
                .unwrap_or_else(|_| estimate_tokens(messages)),
            None => estimate_tokens(messages),
        }
    }
}

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
        // The gate uses the real tokenizer count when configured, so the boundary
        // matches the model's actual window rather than a byte estimate.
        while self.budget_tokens(&working.messages).await > target {
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

        let final_tokens = self.budget_tokens(&working.messages).await;
        tracing::debug!(tokens = final_tokens, target, "compacted working set");
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
        let msgs = SlidingWindow::heuristic().assemble(input).await.unwrap();

        // [ system(base+prepend), user(goal), system(append) ]
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, Role::System);
        assert!(msgs[0].content_text().contains("BASE"));
        assert!(msgs[0].content_text().contains("PRE-CONTENT"));
        assert_eq!(msgs[1].role, Role::User);
        assert_eq!(msgs[1].content_text(), "do the thing");
        assert_eq!(msgs[2].role, Role::System);
        assert!(msgs[2].content_text().contains("POST-CONTENT"));
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
        let msgs = SlidingWindow::heuristic().assemble(input).await.unwrap();
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
        SlidingWindow::heuristic()
            .compact(&mut w, &budget)
            .await
            .unwrap();
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
        SlidingWindow::heuristic()
            .compact(&mut w, &budget)
            .await
            .unwrap();
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
        SlidingWindow::heuristic()
            .compact(&mut w, &budget)
            .await
            .unwrap();
        assert!(w.messages.len() >= 2);
    }

    // --- real token count drives compaction (the spec-23 differentiator) -----
    // `FixedVocabTokenizer` counts 1 token per whitespace word; `long(role,n)`
    // builds "x "×n = n words. So the tokenizer sees ~n tokens where the byte
    // heuristic sees ~n/2 — the two disagree about the budget boundary, and the
    // injected tokenizer must win.
    use agent_testkit::FixedVocabTokenizer;
    use std::sync::Arc;

    #[tokio::test]
    async fn compact_uses_real_count_where_heuristic_would_not() {
        // Two 100-word user turns: tokenizer count ≈ 206, heuristic ≈ 106.
        let msgs = || WorkingSet {
            messages: vec![
                Message::system("s"),
                long(Role::User, 100),
                long(Role::User, 100),
            ],
        };
        // Target 150 sits *between* the heuristic (106) and the real count (206).
        let budget = TokenBudget {
            max_context_tokens: 200,
            reserve_output: 50,
        };

        // Heuristic thinks we're under budget → no-op.
        let mut w_heur = msgs();
        SlidingWindow::heuristic()
            .compact(&mut w_heur, &budget)
            .await
            .unwrap();
        assert_eq!(
            w_heur.messages.len(),
            3,
            "heuristic wrongly leaves it alone"
        );

        // The real tokenizer knows we're over → it drops a turn.
        let mut w_tok = msgs();
        SlidingWindow::new(Some(Arc::new(FixedVocabTokenizer)), "fixed")
            .compact(&mut w_tok, &budget)
            .await
            .unwrap();
        assert!(
            w_tok.messages.len() < 3,
            "real count should trigger a drop the heuristic skips"
        );
        let tok = FixedVocabTokenizer;
        assert!(
            tok.count_messages(&w_tok.messages, "fixed").await.unwrap() <= 150,
            "real count under target after compaction"
        );
    }

    #[tokio::test]
    async fn compact_noop_where_heuristic_would_over_count() {
        // The reverse crossover: one long *space-free* blob — the tokenizer sees
        // ~1 token, the byte heuristic sees ~100. Under a target of 50 the
        // heuristic would compact; the real count must leave it untouched.
        let blob = Message::user("x".repeat(400));
        let msgs = || WorkingSet {
            messages: vec![Message::system("s"), blob.clone(), Message::assistant("ok")],
        };
        let budget = TokenBudget {
            max_context_tokens: 50,
            reserve_output: 0,
        };

        let mut w_heur = msgs();
        SlidingWindow::heuristic()
            .compact(&mut w_heur, &budget)
            .await
            .unwrap();
        assert!(w_heur.messages.len() < 3, "heuristic over-counts and trims");

        let mut w_tok = msgs();
        SlidingWindow::new(Some(Arc::new(FixedVocabTokenizer)), "fixed")
            .compact(&mut w_tok, &budget)
            .await
            .unwrap();
        assert_eq!(
            w_tok.messages.len(),
            3,
            "real count is under budget → no compaction"
        );
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
        SlidingWindow::heuristic()
            .compact(&mut w, &budget)
            .await
            .unwrap();
        assert_eq!(w.messages[0].role, Role::System);
        assert_eq!(w.messages[1].role, Role::Assistant, "orphan tool removed");
    }
}

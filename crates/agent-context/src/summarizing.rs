//! `context-summarizing` — compact by summarizing, not dropping.
//!
//! When the working set grows past budget, keep the leading system message(s) and
//! a recent tail (~`keep_recent_tokens`), and replace the middle with a single
//! LLM-generated summary system message. If summarization fails, fall back to
//! dropping the oldest turns so the loop still makes progress.
//!
//! Unlike `SlidingWindow`, this needs a model, so the strategy holds its own
//! `LlmProvider` (the registry passes the agent's provider to the factory).

use crate::{assemble_messages, estimate_tokens};
use agent_core::{
    CompletionRequest, ContextInput, ContextStrategy, LlmProvider, Message, Result, Role,
    TokenBudget, WorkingSet,
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct SummarizingWindow {
    summarizer: Arc<dyn LlmProvider>,
    keep_recent_tokens: u32,
    summary_max_tokens: u32,
}

impl SummarizingWindow {
    pub fn new(summarizer: Arc<dyn LlmProvider>, keep_recent_tokens: u32) -> Self {
        Self {
            summarizer,
            keep_recent_tokens,
            summary_max_tokens: 1024,
        }
    }
}

#[async_trait]
impl ContextStrategy for SummarizingWindow {
    async fn assemble(&self, input: ContextInput) -> Result<Vec<Message>> {
        Ok(assemble_messages(input))
    }

    async fn compact(&self, working: &mut WorkingSet, budget: &TokenBudget) -> Result<()> {
        let target = budget
            .max_context_tokens
            .saturating_sub(budget.reserve_output);
        if estimate_tokens(&working.messages) <= target {
            return Ok(());
        }

        let msgs = &working.messages;
        let head = leading_system_count(msgs);

        // Walk back from the end, keeping a recent tail of ~keep_recent_tokens.
        let mut cut = msgs.len();
        let mut acc = 0u32;
        while cut > head {
            acc += estimate_tokens(std::slice::from_ref(&msgs[cut - 1]));
            if acc >= self.keep_recent_tokens {
                break;
            }
            cut -= 1;
        }
        // Don't let the tail begin with an orphan tool result (no preceding
        // assistant tool_call in the kept set): fold such messages into the
        // summary instead.
        while cut < msgs.len() && msgs[cut].role == Role::Tool {
            cut += 1;
        }

        // Nothing meaningful to summarize → fall back to truncation.
        if cut <= head {
            drop_oldest(working, target);
            return Ok(());
        }

        let to_summarize = &msgs[head..cut];
        match self.summarize(to_summarize).await {
            Ok(summary) => {
                let mut rebuilt: Vec<Message> = msgs[..head].to_vec();
                rebuilt.push(Message::system(format!(
                    "## Summary of earlier conversation\n{summary}"
                )));
                rebuilt.extend_from_slice(&msgs[cut..]);
                working.messages = rebuilt;
                tracing::debug!(
                    kept = working.messages.len(),
                    "summarized older turns into a single message"
                );
            }
            Err(e) => {
                tracing::warn!("summarization failed ({e}); falling back to truncation");
                drop_oldest(working, target);
            }
        }
        Ok(())
    }
}

impl SummarizingWindow {
    async fn summarize(&self, messages: &[Message]) -> Result<String> {
        let rendered = render(messages);
        let req = CompletionRequest {
            messages: vec![
                Message::system(
                    "You compress conversation history. Summarize the excerpt below concisely, \
                     preserving key facts, decisions, file paths, and tool outcomes. Output only \
                     the summary.",
                ),
                Message::user(rendered),
            ],
            tools: vec![],
            max_tokens: self.summary_max_tokens,
            temperature: 0.0,
        };
        let resp = self.summarizer.complete(req).await?;
        Ok(resp.message.content)
    }
}

/// Count leading system messages (the head to preserve verbatim).
fn leading_system_count(messages: &[Message]) -> usize {
    messages
        .iter()
        .take_while(|m| m.role == Role::System)
        .count()
}

/// Render messages as plain text for the summarizer prompt.
fn render(messages: &[Message]) -> String {
    let mut out = String::new();
    for m in messages {
        let who = match m.role {
            Role::System => "SYSTEM",
            Role::User => "USER",
            Role::Assistant => "ASSISTANT",
            Role::Tool => "TOOL",
        };
        out.push_str(who);
        out.push_str(": ");
        out.push_str(&m.content);
        for tc in &m.tool_calls {
            out.push_str(&format!("\n  [tool_call {} {}]", tc.name, tc.arguments));
        }
        out.push('\n');
    }
    out
}

/// Truncation fallback: drop the oldest non-system message until under target.
fn drop_oldest(working: &mut WorkingSet, target: u32) {
    while estimate_tokens(&working.messages) > target {
        let victim = working.messages.iter().position(|m| m.role != Role::System);
        match victim {
            Some(idx) if working.messages.len() > 2 => {
                working.messages.remove(idx);
            }
            _ => break,
        }
    }
    while let Some(idx) = working.messages.iter().position(|m| m.role != Role::System) {
        if working.messages[idx].role == Role::Tool {
            working.messages.remove(idx);
        } else {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{CompletionResponse, ModelCapabilities, Usage};

    /// Returns a fixed summary regardless of input.
    struct FixedSummarizer;
    #[async_trait]
    impl LlmProvider for FixedSummarizer {
        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities {
                supports_tools: false,
                context_window: 1000,
            }
        }
        async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
            Ok(CompletionResponse {
                message: Message::assistant("SUMMARY"),
                finish_reason: "stop".into(),
                usage: Some(Usage::default()),
            })
        }
    }

    fn long(role: Role, n: usize) -> Message {
        let content = "x ".repeat(n);
        match role {
            Role::System => Message::system(content),
            Role::User => Message::user(content),
            Role::Assistant => Message::assistant(content),
            Role::Tool => Message::tool("id", content),
        }
    }

    #[tokio::test]
    async fn summarizes_middle_keeps_head_and_tail() {
        let strat = SummarizingWindow::new(Arc::new(FixedSummarizer), 200);
        // system head + several large turns; small budget forces compaction.
        let mut working = WorkingSet {
            messages: vec![
                long(Role::System, 20),
                long(Role::User, 400),
                long(Role::Assistant, 400),
                long(Role::User, 400),
                long(Role::Assistant, 50), // recent tail
            ],
        };
        let budget = TokenBudget {
            max_context_tokens: 500,
            reserve_output: 100,
        };
        strat.compact(&mut working, &budget).await.unwrap();

        // Head system preserved; a summary system message inserted right after.
        assert_eq!(working.messages[0].role, Role::System);
        assert!(working.messages[1].content.contains("SUMMARY"));
        // The recent tail survived.
        let last = working.messages.last().unwrap();
        assert_eq!(last.role, Role::Assistant);
        // Fewer messages than we started with.
        assert!(working.messages.len() < 5);
    }

    #[tokio::test]
    async fn no_op_when_under_budget() {
        let strat = SummarizingWindow::new(Arc::new(FixedSummarizer), 200);
        let mut working = WorkingSet {
            messages: vec![Message::system("s"), Message::user("hi")],
        };
        let budget = TokenBudget {
            max_context_tokens: 100_000,
            reserve_output: 1000,
        };
        strat.compact(&mut working, &budget).await.unwrap();
        assert_eq!(working.messages.len(), 2); // untouched
    }
}

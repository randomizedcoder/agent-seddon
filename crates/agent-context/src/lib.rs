//! Context assembly / compaction implementations behind the `ContextStrategy`
//! seam. Each strategy is gated by a cargo feature; the registry in
//! `agent-runtime` selects one by config string. See `docs/extending.md`.
//!
//! The strategies share the same `assemble` (only compaction differs), so that
//! logic — plus the rough token estimate — lives here.

#[cfg(feature = "context-sliding-window")]
mod sliding_window;
#[cfg(feature = "context-sliding-window")]
pub use sliding_window::SlidingWindow;

#[cfg(feature = "context-summarizing")]
mod summarizing;
#[cfg(feature = "context-summarizing")]
pub use summarizing::SummarizingWindow;

#[cfg(any(feature = "context-sliding-window", feature = "context-summarizing"))]
use agent_core::{ContextInput, Message};

/// Build the initial `[system, user, (system-append)]` message list: fold the
/// prepend context + recalled memory into the system prompt, add the goal, and
/// append any trailing context as a final system message.
#[cfg(any(feature = "context-sliding-window", feature = "context-summarizing"))]
pub(crate) fn assemble_messages(input: ContextInput) -> Vec<Message> {
    let mut system = input.system_prompt;

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

    if !input.append.is_empty() {
        let mut tail = String::new();
        for block in &input.append {
            tail.push_str(&format!("## {}\n{}\n\n", block.source, block.content));
        }
        messages.push(Message::system(tail.trim_end().to_string()));
    }
    messages
}

/// Very rough token estimate: ~4 chars per token, plus a small per-message tax.
#[cfg(any(feature = "context-sliding-window", feature = "context-summarizing"))]
pub(crate) fn estimate_tokens(messages: &[Message]) -> u32 {
    let mut tokens = 0u32;
    for m in messages {
        let mut chars = 0usize;
        for b in &m.content {
            match b {
                agent_core::ContentBlock::Text { text } => chars += text.len(),
                // Media is not text: charge it via the shared estimator so all
                // three estimators agree on what an image costs. Counting only
                // text here would let an image-bearing turn look nearly free and
                // overflow the model's window.
                media => tokens = tokens.saturating_add(agent_core::media_block_tokens(media)),
            }
        }
        for tc in &m.tool_calls {
            chars += tc.name.len() + tc.arguments.to_string().len();
        }
        chars += 8; // role/formatting overhead
        tokens = tokens.saturating_add((chars / 4) as u32);
    }
    tokens
}

/// Benchmark hook: `estimate_tokens` is called repeatedly inside the compaction
/// loop, so guard its cost. Exposed for `benches/context.rs` (the fn is crate-private).
#[doc(hidden)]
pub fn bench_estimate_tokens(messages: &[Message]) -> u32 {
    estimate_tokens(messages)
}

#[cfg(all(
    test,
    any(feature = "context-sliding-window", feature = "context-summarizing")
))]
mod tests {
    use super::*;
    use agent_core::{ContextBlock, MemoryItem, Role, ToolCall};
    use rstest::rstest;
    use serde_json::json;

    // --- estimate_tokens: (Σ content + tool-calls + 8/msg) / 4 -------------
    #[rstest]
    #[case::boundary_empty(vec![], 0)]
    #[case::single_empty_is_overhead_only(vec![Message::system("")], 2)]
    #[case::single_text(vec![Message::user("hello")], 3)]
    #[case::two_messages(vec![Message::system(""), Message::user("hi")], 4)]
    #[case::corner_unicode_uses_byte_len(vec![Message::user("café")], 3)]
    fn estimate_tokens_cases(#[case] messages: Vec<Message>, #[case] expected: u32) {
        assert_eq!(estimate_tokens(&messages), expected);
    }

    #[test]
    fn estimate_tokens_counts_tool_calls() {
        let mut m = Message::assistant("");
        m.tool_calls.push(ToolCall {
            id: "1".into(),
            name: "ls".into(),
            arguments: json!({}),
        });
        // 0 content + ("ls"=2 + "{}"=2) + 8 overhead = 12 → 12/4 = 3
        assert_eq!(estimate_tokens(&[m]), 3);
    }

    // --- assemble_messages: system/user/(append) composition ---------------
    fn block(source: &str, content: &str) -> ContextBlock {
        ContextBlock {
            source: source.into(),
            content: content.into(),
        }
    }
    fn mem(source: &str, content: &str) -> MemoryItem {
        MemoryItem {
            source: source.into(),
            content: content.into(),
        }
    }
    fn input(
        prepend: Vec<ContextBlock>,
        recalled: Vec<MemoryItem>,
        append: Vec<ContextBlock>,
    ) -> ContextInput {
        ContextInput {
            system_prompt: "SYS".into(),
            prepend,
            recalled,
            goal: "GOAL".into(),
            append,
        }
    }

    /// `(n_prepend, n_recalled, n_append) → message count`. Append adds a third
    /// (trailing system) message; prepend/recalled fold into the first system one.
    #[rstest]
    #[case::minimal(0, 0, 0, 2)]
    #[case::with_prepend(1, 0, 0, 2)]
    #[case::with_recalled(0, 1, 0, 2)]
    #[case::with_append(0, 0, 1, 3)]
    #[case::all(2, 1, 1, 3)]
    fn assemble_message_count_cases(
        #[case] np: usize,
        #[case] nr: usize,
        #[case] na: usize,
        #[case] expected: usize,
    ) {
        let prepend = (0..np).map(|i| block(&format!("p{i}"), "c")).collect();
        let recalled = (0..nr).map(|i| mem(&format!("m{i}"), "c")).collect();
        let append = (0..na).map(|i| block(&format!("a{i}"), "c")).collect();
        let msgs = assemble_messages(input(prepend, recalled, append));
        assert_eq!(msgs.len(), expected);
        assert_eq!(msgs[0].role, Role::System);
        assert_eq!(msgs[1].role, Role::User);
        assert_eq!(msgs[1].content_text(), "GOAL");
    }

    #[test]
    fn assemble_folds_prepend_and_recalled_into_system() {
        let msgs = assemble_messages(input(
            vec![block("srcP", "BODYP")],
            vec![mem("srcM", "BODYM")],
            vec![],
        ));
        let sys = msgs[0].content_text();
        assert!(sys.starts_with("SYS"), "system prompt kept as head: {sys}");
        assert!(sys.contains("## srcP\nBODYP"));
        assert!(sys.contains("## Recalled memory"));
        assert!(sys.contains("### srcM\nBODYM"));
    }

    #[test]
    fn assemble_append_is_trailing_system_message() {
        let msgs = assemble_messages(input(vec![], vec![], vec![block("note", "N")]));
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[2].role, Role::System);
        assert!(msgs[2].content_text().contains("## note\nN"));
    }
}

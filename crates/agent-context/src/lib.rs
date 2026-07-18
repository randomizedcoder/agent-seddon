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

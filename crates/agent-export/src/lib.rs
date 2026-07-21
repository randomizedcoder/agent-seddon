//! Session export: render a saved transcript to markdown, JSON, or a
//! self-contained HTML page (parity spec 20).
//!
//! Two properties define this crate:
//!
//! * **Deterministic.** Rendering is a pure function of the transcript — no wall
//!   clock, no random ids, no map-iteration order in the output. The same
//!   session exports to the same bytes, which is what makes the result diffable,
//!   cacheable, golden-testable, and instruction-count benchable.
//! * **Safe to share.** Secrets are redacted before any renderer sees the text,
//!   and every interpolated value is HTML-escaped — a transcript is exactly the
//!   artifact people paste into bug reports.

pub mod redact;
pub mod render;

pub use redact::{apply as apply_redactions, fallback_findings, redact_with};
pub use render::{escape_html, render, Format};

use agent_core::Message;

/// Redact a transcript in place, using `scanner` when wired and the built-in
/// fallback otherwise, then render it.
///
/// Redaction operates on each message's **text**; media blocks carry bytes, not
/// prose, and are described rather than inlined by the renderers.
pub async fn export(
    format: Format,
    session_id: &str,
    messages: &[Message],
    scanner: Option<&dyn agent_core::Scanner>,
    redact: bool,
) -> String {
    if !redact {
        return render::render(format, session_id, messages);
    }
    let mut safe: Vec<Message> = Vec::with_capacity(messages.len());
    for m in messages {
        let mut m = m.clone();
        for block in &mut m.content {
            if let agent_core::ContentBlock::Text { text } = block {
                *text = match scanner {
                    Some(s) => redact::redact_with(s, text).await,
                    None => redact::apply(text, redact::fallback_findings(text)),
                };
            }
        }
        // Tool arguments routinely carry file bodies and command lines, which is
        // where a credential most often ends up.
        for tc in &mut m.tool_calls {
            let raw = tc.arguments.to_string();
            let cleaned = match scanner {
                Some(s) => redact::redact_with(s, &raw).await,
                None => redact::apply(&raw, redact::fallback_findings(&raw)),
            };
            if cleaned != raw {
                // Redaction breaks JSON validity, so carry it as a string rather
                // than emitting a malformed object.
                tc.arguments = serde_json::Value::String(cleaned);
            }
        }
        safe.push(m);
    }
    render::render(format, session_id, &safe)
}

/// Bench hook: render all three formats (the deterministic CPU path).
#[doc(hidden)]
pub fn bench_render(messages: &[Message]) -> usize {
    render::bench_render(messages)
}

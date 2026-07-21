//! Prompt-cache breakpoint placement behind the `CacheStrategy` seam
//! (parity spec 24).
//!
//! Two strategies:
//!
//! * [`StablePrefix`] (default) — anchor the provably-stable prefix: end of
//!   system, last tool def, and the last stable history message.
//! * [`TailWindow`] — opencode's shape: anchor the head and a recent-tail
//!   message, trading some prefix coverage for faster re-anchoring.
//!
//! Both obey the same invariants, which are where prompt caching actually goes
//! wrong in the field:
//!
//! 1. **Never anchor the volatile tail.** The newest message changes next turn,
//!    so anchoring it pays the write premium for a prefix that will never be read.
//! 2. **Never anchor history in a just-compacted window.** Compaction rewrites
//!    the middle, invalidating every downstream anchor; that turn is a full write.
//! 3. **Respect the provider's cap** (Anthropic: 4), dropping the
//!    lowest-priority anchors rather than sending an over-limit request.
//! 4. **No-op on a non-caching provider** — the request must be byte-identical.

use agent_core::{CacheCapabilities, CacheMarks, CacheStrategy, PromptShape};

mod key;
pub use key::stable_cache_key;

/// Anchor the provably-stable prefix: system, tool defs, and the newest history
/// message that is *not* the volatile tail.
///
/// This maximises cached prefix length, which is what dominates cost: the system
/// prompt and tool definitions are stable for the whole session, and history is
/// stable up to the tail until compaction rewrites it.
pub struct StablePrefix;

impl CacheStrategy for StablePrefix {
    fn name(&self) -> &str {
        "stable-prefix"
    }

    fn place(&self, prompt: &PromptShape<'_>, caps: &CacheCapabilities) -> CacheMarks {
        let mut marks = CacheMarks::default();
        if caps.is_noop() {
            return marks; // request must be unchanged
        }
        if caps.automatic_prefix {
            // No explicit anchors; a stable key is what makes the provider's own
            // prefix cache hit across turns.
            marks.cache_key = Some(stable_cache_key(prompt));
            return marks;
        }

        marks.system = prompt.has_system;
        marks.tools = prompt.tools > 0 && caps.supports_on_tools;

        // History is only anchorable when the window was NOT just compacted —
        // otherwise everything downstream of the rewrite is invalid anyway.
        if !prompt.compacted {
            if let Some(idx) = stable_history_index(prompt) {
                marks.messages.push(idx);
            }
        }
        enforce_cap(&mut marks, caps);
        marks
    }
}

/// opencode's shape: anchor the head (system/tools) plus a message a little way
/// back from the tail, so the anchor moves forward as the conversation grows.
pub struct TailWindow {
    /// How far back from the tail to anchor (1 ⇒ the message before the tail).
    back: usize,
}

impl Default for TailWindow {
    fn default() -> Self {
        Self { back: 2 }
    }
}

impl TailWindow {
    pub fn new(back: usize) -> Self {
        Self { back: back.max(1) }
    }
}

impl CacheStrategy for TailWindow {
    fn name(&self) -> &str {
        "tail-window"
    }

    fn place(&self, prompt: &PromptShape<'_>, caps: &CacheCapabilities) -> CacheMarks {
        let mut marks = CacheMarks::default();
        if caps.is_noop() {
            return marks;
        }
        if caps.automatic_prefix {
            marks.cache_key = Some(stable_cache_key(prompt));
            return marks;
        }

        marks.system = prompt.has_system;
        marks.tools = prompt.tools > 0 && caps.supports_on_tools;

        if !prompt.compacted {
            // `back` from the tail, and never the tail itself.
            if let Some(tail) = prompt.tail_index() {
                if let Some(idx) = tail.checked_sub(self.back) {
                    marks.messages.push(idx);
                }
            }
        }
        enforce_cap(&mut marks, caps);
        marks
    }
}

/// The newest message that is safe to anchor: everything except the volatile
/// tail. `None` when the window is only the tail (or empty) — there is no stable
/// history yet.
fn stable_history_index(prompt: &PromptShape<'_>) -> Option<usize> {
    prompt.tail_index().and_then(|t| t.checked_sub(1))
}

/// Drop the lowest-priority anchors until the provider's cap is met.
///
/// Priority is longest-prefix-first: the system prompt covers the most bytes,
/// then tools, then history. Sending an over-limit request is an error at the
/// provider, so this trims rather than hoping.
fn enforce_cap(marks: &mut CacheMarks, caps: &CacheCapabilities) {
    if caps.max_breakpoints == 0 {
        return;
    }
    while marks.count() > caps.max_breakpoints {
        if marks.messages.pop().is_some() {
            continue;
        }
        if marks.tools {
            marks.tools = false;
            continue;
        }
        if marks.system {
            marks.system = false;
            continue;
        }
        break; // nothing left to drop
    }
}

/// Bench hook: placement over a synthetic prompt (the CPU hot path).
#[doc(hidden)]
pub fn bench_place(messages: &[agent_core::Message], tools: usize) -> usize {
    let shape = PromptShape::new(true, tools, messages);
    StablePrefix
        .place(&shape, &CacheCapabilities::anthropic())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::Message;
    use rstest::rstest;

    fn msgs(n: usize) -> Vec<Message> {
        (0..n).map(|i| Message::user(format!("m{i}"))).collect()
    }

    /// The default strategy anchors the stable prefix: system, tools, and the
    /// newest non-tail history message.
    #[test]
    fn positive_breakpoints_after_system_and_tools() {
        let m = msgs(4);
        let marks = StablePrefix.place(
            &PromptShape::new(true, 8, &m),
            &CacheCapabilities::anthropic(),
        );
        assert!(marks.system);
        assert!(marks.tools);
        assert_eq!(marks.messages, vec![2], "the message before the tail");
    }

    /// The volatile tail is NEVER anchored: it changes next turn, so anchoring
    /// pays the write premium for a prefix that will never be read. This is the
    /// single most costly placement mistake.
    #[rstest]
    #[case::negative_stable_prefix_skips_tail(0)]
    #[case::negative_tail_window_skips_tail(1)]
    fn negative_no_anchor_on_volatile_tail(#[case] which: usize) {
        let m = msgs(4);
        let shape = PromptShape::new(true, 4, &m);
        let caps = CacheCapabilities::anthropic();
        let marks = if which == 0 {
            StablePrefix.place(&shape, &caps)
        } else {
            TailWindow::default().place(&shape, &caps)
        };
        let tail = shape.tail_index().unwrap();
        assert!(
            !marks.messages.contains(&tail),
            "tail {tail} must never be anchored, got {:?}",
            marks.messages
        );
    }

    /// A just-compacted window has no stable history prefix: the rewrite
    /// invalidates every anchor downstream of it, so only system + tools are
    /// anchored that turn. This is the marquee compaction boundary.
    #[rstest]
    #[case::boundary_compacted_drops_history(true, 0)]
    #[case::positive_uncompacted_keeps_history(false, 1)]
    fn compaction_invalidates_history_anchors(
        #[case] compacted: bool,
        #[case] want_history: usize,
    ) {
        let m = msgs(6);
        let marks = StablePrefix.place(
            &PromptShape::new(true, 4, &m).compacted(compacted),
            &CacheCapabilities::anthropic(),
        );
        assert_eq!(marks.messages.len(), want_history);
        assert!(marks.system, "system stays anchorable either way");
    }

    /// Never exceed the provider's hard cap; drop lowest-priority first.
    #[rstest]
    #[case::boundary_at_most_four(4)]
    #[case::boundary_cap_of_one(1)]
    #[case::boundary_cap_of_zero_is_unlimited(0)]
    fn boundary_breakpoint_cap_is_enforced(#[case] cap: usize) {
        let m = msgs(10);
        let caps = CacheCapabilities {
            max_breakpoints: cap,
            ..CacheCapabilities::anthropic()
        };
        let marks = StablePrefix.place(&PromptShape::new(true, 12, &m), &caps);
        if cap > 0 {
            assert!(marks.count() <= cap, "placed {} > cap {cap}", marks.count());
        }
    }

    /// A provider with no prompt cache must get a byte-identical request.
    #[test]
    fn negative_noop_for_non_caching_provider() {
        let m = msgs(4);
        let marks = StablePrefix.place(&PromptShape::new(true, 8, &m), &CacheCapabilities::none());
        assert!(marks.is_empty(), "must place nothing, got {marks:?}");
    }

    /// OpenAI-family: no anchors, but a stable key so the automatic prefix cache
    /// hits across turns.
    #[test]
    fn corner_openai_sets_stable_key_and_no_marks() {
        let m = msgs(4);
        let marks =
            StablePrefix.place(&PromptShape::new(true, 8, &m), &CacheCapabilities::openai());
        assert_eq!(marks.count(), 0, "no explicit anchors for this family");
        let key = marks.cache_key.expect("a cache key");
        assert!(key.len() <= 64, "key must be clamped, got {}", key.len());
    }

    /// Tool anchoring is gated on the capability.
    #[rstest]
    #[case::positive_tools_supported(true, true)]
    #[case::boundary_tools_gating_off(false, false)]
    fn tool_anchor_follows_capability(#[case] supported: bool, #[case] want: bool) {
        let m = msgs(4);
        let caps = CacheCapabilities {
            supports_on_tools: supported,
            ..CacheCapabilities::anthropic()
        };
        let marks = StablePrefix.place(&PromptShape::new(true, 6, &m), &caps);
        assert_eq!(marks.tools, want);
    }

    /// Degenerate windows must not panic or anchor anything nonsensical.
    #[rstest]
    #[case::boundary_empty_window(0)]
    #[case::boundary_single_message_is_only_the_tail(1)]
    #[case::corner_two_messages(2)]
    fn boundary_small_windows(#[case] n: usize) {
        let m = msgs(n);
        let shape = PromptShape::new(true, 2, &m);
        for s in [
            &StablePrefix as &dyn CacheStrategy,
            &TailWindow::default() as &dyn CacheStrategy,
        ] {
            let marks = s.place(&shape, &CacheCapabilities::anthropic());
            for &i in &marks.messages {
                assert!(i < n, "{} anchored out-of-range index {i}", s.name());
                assert!(Some(i) != shape.tail_index(), "anchored the tail");
            }
        }
    }

    /// `TailWindow` anchors further back than `StablePrefix`, which is the whole
    /// point of the alternative.
    #[test]
    fn positive_tail_window_anchors_further_back() {
        let m = msgs(8);
        let shape = PromptShape::new(true, 4, &m);
        let caps = CacheCapabilities::anthropic();
        let sp = StablePrefix.place(&shape, &caps);
        let tw = TailWindow::new(3).place(&shape, &caps);
        assert!(tw.messages[0] < sp.messages[0]);
    }
}

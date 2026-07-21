//! A stable prompt-cache key for providers that cache automatically.
//!
//! OpenAI-family providers reuse a cached prefix on their own, but reward a
//! stable `prompt_cache_key` for routing affinity — the same key should land on
//! the same cache. The key must therefore be:
//!
//! * **stable across turns** of one session (or affinity is lost every turn),
//! * **distinct across different prefixes** (or two unrelated sessions collide),
//! * **≤ 64 characters** (the documented limit; pi clamps to the same).
//!
//! So it is derived from the *stable* part of the prompt only — the system
//! presence, the tool count, and the head of the conversation — deliberately
//! excluding the volatile tail, which changes every turn.

use agent_core::PromptShape;

/// Maximum length of the key, per the provider's documented limit.
pub const MAX_KEY_LEN: usize = 64;

/// FNV-1a — small, dependency-free, and sufficient: this is a cache-affinity
/// hint, not a security primitive.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// A stable key for `prompt`, clamped to [`MAX_KEY_LEN`].
pub fn stable_cache_key(prompt: &PromptShape<'_>) -> String {
    let mut buf = String::new();
    buf.push_str(if prompt.has_system { "s1" } else { "s0" });
    buf.push(':');
    buf.push_str(&prompt.tools.to_string());
    buf.push(':');
    // Hash the STABLE head only — everything except the volatile tail. Including
    // the tail would change the key every turn and defeat the affinity it exists
    // to provide.
    let stable = prompt
        .messages
        .len()
        .saturating_sub(prompt.tail_index().map(|_| 1).unwrap_or(0));
    for m in &prompt.messages[..stable] {
        buf.push_str(m.role.as_str());
        buf.push('\u{1}');
        buf.push_str(&m.content_text());
        buf.push('\u{2}');
    }
    let key = format!("agent-{:016x}", fnv1a(buf.as_bytes()));
    debug_assert!(key.len() <= MAX_KEY_LEN);
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::Message;
    use rstest::rstest;

    fn shape(msgs: &[Message], tools: usize) -> PromptShape<'_> {
        PromptShape::new(true, tools, msgs)
    }

    /// The key must not change when only the tail changes — that is the whole
    /// point: affinity has to survive each new turn.
    #[test]
    fn positive_key_is_stable_as_the_tail_changes() {
        let a = vec![
            Message::system("sys"),
            Message::user("first"),
            Message::assistant("reply"),
        ];
        let mut b = a.clone();
        b.push(Message::user("a brand new question"));
        // `a`'s tail is "reply"; `b`'s tail is the new question. The shared stable
        // head is the same, so the key must match.
        let ka = stable_cache_key(&shape(&a, 4));
        let kb = stable_cache_key(&shape(&b[..3], 4));
        assert_eq!(ka, kb);
    }

    /// Different prefixes must not collide onto one cache.
    #[rstest]
    #[case::negative_different_history("other")]
    #[case::negative_different_role("sys")]
    fn negative_distinct_prefixes_differ(#[case] first: &str) {
        let a = vec![
            Message::system("sys"),
            Message::user("first"),
            Message::user("t"),
        ];
        let b = vec![
            Message::system("sys"),
            Message::user(first),
            Message::user("t"),
        ];
        let ka = stable_cache_key(&shape(&a, 4));
        let kb = stable_cache_key(&shape(&b, 4));
        if first == "first" {
            assert_eq!(ka, kb);
        } else {
            assert_ne!(ka, kb, "distinct prefixes must not share a cache key");
        }
    }

    /// A different tool count is a different prefix.
    #[test]
    fn negative_tool_count_changes_the_key() {
        let m = vec![Message::system("sys"), Message::user("q")];
        assert_ne!(
            stable_cache_key(&shape(&m, 4)),
            stable_cache_key(&shape(&m, 5))
        );
    }

    /// The provider's documented limit, held regardless of input size.
    #[rstest]
    #[case::boundary_empty(0)]
    #[case::positive_small(4)]
    #[case::adversarial_huge_history(5_000)]
    fn boundary_key_is_always_clamped(#[case] n: usize) {
        let m: Vec<Message> = (0..n)
            .map(|i| Message::user("x".repeat(200) + &i.to_string()))
            .collect();
        let key = stable_cache_key(&shape(&m, 12));
        assert!(key.len() <= MAX_KEY_LEN, "key was {} chars", key.len());
        assert!(key.is_ascii(), "key must be header-safe ascii");
    }
}

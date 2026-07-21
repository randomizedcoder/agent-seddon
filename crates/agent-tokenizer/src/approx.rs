//! `tokenizer-approx` — a deterministic, Unicode-aware segmenter.
//!
//! It classifies each character into one of three classes and counts token
//! "pieces" the way a real BPE tokenizer roughly does, without a vocab:
//!
//! - **word** run (letters/digits/`_`): `ceil(len / CHARS_PER_WORD_TOKEN)` tokens,
//!   so a long identifier splits into several subword pieces (real BPE behaviour)
//!   and a short word is one token.
//! - **whitespace** run: 0 tokens — a leading space is folded into the next token
//!   by real tokenizers, so it carries no independent cost here.
//! - any other character (punctuation/symbol): 1 token each — dense punctuation
//!   (`fn f(x:i32)->i32{…}`) costs far more than `chars/4` would suggest, which is
//!   exactly where the byte heuristic is worst.
//!
//! It counts by **`char`**, not bytes, so `"héllo"` is not inflated the way the
//! byte-based heuristic inflates multi-byte UTF-8. The result is deterministic and
//! allocation-free, which makes it a clean iai bench + dhat leak target.

use agent_core::{Result, Tokenizer};
use async_trait::async_trait;

/// Average characters per subword token inside a word run. 4 tracks common BPE
/// vocabularies (GPT/Claude-family) closely enough for budgeting.
const CHARS_PER_WORD_TOKEN: usize = 4;

/// The default, dependency-free [`Tokenizer`]. `model` is accepted (and surfaced
/// as a span label upstream) but does not change the count today — the seam lets a
/// per-model BPE backend replace this without touching callers.
#[derive(Debug, Clone, Copy, Default)]
pub struct ApproxTokenizer;

impl ApproxTokenizer {
    pub fn new() -> Self {
        Self
    }

    /// The synchronous core — the CPU hot path benched in `benches/tokenize.rs`.
    /// Model-agnostic today; see the module docs for the segmentation rule.
    pub fn count_text(&self, text: &str) -> u32 {
        let mut tokens: u32 = 0;
        // Length (in chars) of the word run currently being accumulated.
        let mut word_len: usize = 0;

        for ch in text.chars() {
            if ch.is_alphanumeric() || ch == '_' {
                word_len += 1;
            } else {
                tokens = tokens.saturating_add(word_tokens(word_len));
                word_len = 0;
                // Whitespace folds into the next token (0); every other char is 1.
                if !ch.is_whitespace() {
                    tokens = tokens.saturating_add(1);
                }
            }
        }
        tokens.saturating_add(word_tokens(word_len))
    }
}

/// Tokens contributed by a word run of `len` chars: `ceil(len / 4)`, and 0 for an
/// empty run (so flushing between segments is a no-op).
fn word_tokens(len: usize) -> u32 {
    if len == 0 {
        0
    } else {
        len.div_ceil(CHARS_PER_WORD_TOKEN) as u32
    }
}

#[async_trait]
impl Tokenizer for ApproxTokenizer {
    fn backend(&self) -> &str {
        "approx"
    }

    async fn count(&self, text: &str, _model: &str) -> Result<u32> {
        Ok(self.count_text(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{Message, Role, ToolCall, MESSAGE_TOKEN_OVERHEAD};
    use rstest::rstest;
    use serde_json::json;

    // --- count_text: word/punct/whitespace segmentation --------------------
    // `positive_` a normal count, `corner_` odd-but-valid (unicode/dense code),
    // `boundary_` an edge (empty). Numbers follow the documented rule:
    // word run = ceil(chars/4), punctuation = 1 each, whitespace = 0.
    #[rstest]
    #[case::positive_four_short_words("one two four five", 4)] // each ≤4 chars → 1 token
    #[case::positive_long_identifier("supercalifragilistic", 5)] // 20 chars → ceil(20/4)
    #[case::corner_unicode_by_char_not_bytes("héllo wörld", 4)] // 5+5 chars → 2+2; bytes would inflate
    #[case::corner_dense_code("fn f(x:i32)->i32{x+1}", 15)] // 7 word tokens + 8 punctuation
    #[case::boundary_empty("", 0)]
    #[case::boundary_only_whitespace("   \n\t ", 0)]
    #[case::corner_trailing_word("abcd", 1)]
    fn count_text_cases(#[case] text: &str, #[case] expected: u32) {
        assert_eq!(ApproxTokenizer.count_text(text), expected, "text={text:?}");
    }

    // --- count (async) delegates to count_text -----------------------------
    #[tokio::test]
    async fn count_matches_count_text() {
        let t = ApproxTokenizer::new();
        let s = "let mut x = compute(y);";
        assert_eq!(t.count(s, "any-model").await.unwrap(), t.count_text(s));
    }

    // --- count_messages: folds per-message + per-tool-call overhead ---------
    #[tokio::test]
    async fn count_messages_folds_overhead() {
        let t = ApproxTokenizer::new();
        let msgs = vec![Message::system("sys"), Message::user("hi there")];
        // "sys"=1, +overhead; "hi there"= word(2)=1 + word(5)? "there"=5→2 → 1+2=3?
        // "hi"=ceil(2/4)=1, "there"=ceil(5/4)=2 → 3, +overhead.
        let expected = 1 + MESSAGE_TOKEN_OVERHEAD + 3 + MESSAGE_TOKEN_OVERHEAD;
        assert_eq!(t.count_messages(&msgs, "m").await.unwrap(), expected);
    }

    #[tokio::test]
    async fn count_messages_counts_tool_calls() {
        let t = ApproxTokenizer::new();
        let mut m = Message::assistant("");
        m.tool_calls.push(ToolCall {
            id: "1".into(),
            name: "ls".into(),
            arguments: json!({}),
        });
        // content ""=0, name "ls"=1, args "{}"= two punct =2, +overhead.
        let expected = 1 + 2 + MESSAGE_TOKEN_OVERHEAD;
        assert_eq!(t.count_messages(&[m], "m").await.unwrap(), expected);
    }

    #[tokio::test]
    async fn count_messages_empty_history_is_zero() {
        let t = ApproxTokenizer::new();
        let msgs: Vec<Message> = vec![];
        assert_eq!(t.count_messages(&msgs, "m").await.unwrap(), 0);
        let _ = Role::System;
    }
}

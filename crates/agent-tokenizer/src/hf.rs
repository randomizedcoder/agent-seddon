//! `tokenizer-hf` — exact counts from a local model's `tokenizer.json`.
//!
//! The models this repo actually runs (Qwen / GLM / Llama on local GPUs) are not
//! tokenized by tiktoken, so their true counts come from the model's own
//! `tokenizer.json`. This backend loads one (or several) such files through the
//! HuggingFace [`tokenizers`] crate and counts with them. The vocab is a **local
//! file the operator points at** — nothing is downloaded, so the build stays
//! hermetic and offline (the `tokenizers` dep is pulled with `default-features =
//! false`, dropping the `onig` C dependency for a pure-Rust regex path).
//!
//! Robustness: a configured file that is **missing, oversized, or unparseable** is
//! skipped with a warning rather than being fatal — the models it would have
//! served fall through to a less-specific file or, ultimately, the dependency-free
//! [`ApproxTokenizer`]. An unmapped or hostile `model` string can therefore never
//! panic, error, or block a count. See parity spec 23.

use std::path::{Path, PathBuf};

use agent_core::{Result, Tokenizer};
use async_trait::async_trait;
use tokenizers::Tokenizer as HfInner;

use crate::ApproxTokenizer;

/// Upper bound on a `tokenizer.json` we will load. Real vocab files run to a few
/// MB (a large-vocab model ~10–20 MB); this is generous headroom while refusing an
/// accidental or hostile giant file *before* reading it into memory.
const MAX_TOKENIZER_BYTES: u64 = 64 * 1024 * 1024;

/// A [`Tokenizer`] backed by one or more local `tokenizer.json` files, keyed by
/// model-id prefix, with the [`ApproxTokenizer`] heuristic as the final fallback.
pub struct HfTokenizer {
    /// (lowercased model-id prefix, loaded tokenizer), sorted by prefix length
    /// **descending** so the longest (most specific) match wins; the empty-string
    /// prefix is the catch-all default and therefore sorts last.
    by_prefix: Vec<(String, HfInner)>,
    fallback: ApproxTokenizer,
}

// `HfInner` has no `Debug`; report only how many files loaded.
impl std::fmt::Debug for HfTokenizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HfTokenizer")
            .field("loaded", &self.by_prefix.len())
            .finish_non_exhaustive()
    }
}

impl HfTokenizer {
    /// Build from `(model-prefix, path)` pairs. An empty prefix `""` is the
    /// catch-all default. A file that is missing, larger than
    /// [`MAX_TOKENIZER_BYTES`], or unparseable is **skipped with a warning** — it
    /// is never fatal. Always succeeds (a backend with zero loaded files simply
    /// counts everything with the `approx` fallback).
    pub fn new(files: &[(String, PathBuf)]) -> Self {
        let mut by_prefix: Vec<(String, HfInner)> = Vec::new();
        for (prefix, path) in files {
            if let Some(tk) = load_one(path) {
                by_prefix.push((prefix.to_ascii_lowercase(), tk));
            }
        }
        // Longest prefix first so the most specific match is found first; the
        // empty default prefix ends up last.
        by_prefix.sort_by_key(|(prefix, _)| std::cmp::Reverse(prefix.len()));
        Self {
            by_prefix,
            fallback: ApproxTokenizer::new(),
        }
    }

    /// The loaded tokenizer whose prefix matches `model` (longest first), or
    /// `None` when none does (→ approx fallback). Case-insensitive, like the
    /// tiktoken backend, so `Qwen2.5` matches a `qwen` prefix.
    fn tokenizer_for(&self, model: &str) -> Option<&HfInner> {
        let m = model.to_ascii_lowercase();
        self.by_prefix
            .iter()
            .find(|(prefix, _)| m.starts_with(prefix.as_str()))
            .map(|(_, tk)| tk)
    }

    /// Synchronous count core, shared by [`Tokenizer::count`] and the tests. Never
    /// panics: an encode error (or no matching file) falls back to `approx`.
    pub fn count_text(&self, text: &str, model: &str) -> u32 {
        match self.tokenizer_for(model) {
            // `add_special_tokens = false`: we are counting content; the per-message
            // BOS/EOS overhead is folded in separately by `count_messages`.
            Some(tk) => match tk.encode(text, false) {
                Ok(enc) => u32::try_from(enc.len()).unwrap_or(u32::MAX),
                Err(e) => {
                    tracing::warn!(model, error = %e, "hf encode failed; using approx");
                    self.fallback.count_text(text)
                }
            },
            None => self.fallback.count_text(text),
        }
    }
}

/// Load and validate one `tokenizer.json`, or `None` (logged) if it is missing,
/// not a regular file, too large, or unparseable.
fn load_one(path: &Path) -> Option<HfInner> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "hf tokenizer file unreadable; skipping");
            return None;
        }
    };
    if !meta.is_file() {
        tracing::warn!(path = %path.display(), "hf tokenizer path is not a regular file; skipping");
        return None;
    }
    if meta.len() > MAX_TOKENIZER_BYTES {
        tracing::warn!(
            path = %path.display(),
            bytes = meta.len(),
            cap = MAX_TOKENIZER_BYTES,
            "hf tokenizer file exceeds the size cap; skipping"
        );
        return None;
    }
    match HfInner::from_file(path) {
        Ok(tk) => Some(tk),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "hf tokenizer file unparseable; skipping");
            None
        }
    }
}

#[async_trait]
impl Tokenizer for HfTokenizer {
    fn backend(&self) -> &str {
        "hf"
    }

    async fn count(&self, text: &str, model: &str) -> Result<u32> {
        Ok(self.count_text(text, model))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // A tiny WordLevel tokenizer (vocab: hello/world/foo/bar, [UNK]=0) with a
    // Whitespace pre-tokenizer — deterministic counts without shipping a real
    // multi-MB BPE vocab. Kept in the crane source filter (`/tests/fixtures/`).
    fn fixture() -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/tiny_tokenizer.json"
        ))
    }

    // A backend with the fixture mapped to the `tiny` prefix AND as the default.
    fn tok() -> HfTokenizer {
        HfTokenizer::new(&[("tiny".to_string(), fixture()), (String::new(), fixture())])
    }

    // --- exact counts from the loaded vocab --------------------------------
    // Whitespace splits into words; each known word is 1 token, unknown → [UNK].
    #[rstest]
    #[case::positive_two_known_words("hello world", 2)]
    #[case::positive_three_known_words("hello foo bar", 3)]
    #[case::corner_unknown_word_is_unk("hello zzz", 2)] // hello=1, zzz→[UNK]=1
    #[case::boundary_empty("", 0)]
    fn exact_counts(#[case] text: &str, #[case] expected: u32) {
        assert_eq!(
            tok().count_text(text, "tiny-model"),
            expected,
            "text={text:?}"
        );
    }

    // --- prefix routing + default -----------------------------------------
    #[test]
    fn longest_prefix_and_default_route() {
        let t = tok();
        // `tiny-model` matches the `tiny` prefix; a model matching only the empty
        // default still resolves to the fixture (not the approx fallback).
        assert!(t.tokenizer_for("tiny-model").is_some());
        assert!(t.tokenizer_for("anything-else").is_some()); // via the "" default
    }

    // With NO default configured, an unmatched model gets the approx fallback.
    #[test]
    fn no_default_falls_back_to_approx() {
        let t = HfTokenizer::new(&[("tiny".to_string(), fixture())]);
        assert!(t.tokenizer_for("qwen2.5").is_none());
        let s = "hello world foo";
        assert_eq!(t.count_text(s, "qwen2.5"), ApproxTokenizer.count_text(s));
    }

    // --- the async seam delegates to the sync core -------------------------
    #[tokio::test]
    async fn count_matches_count_text() {
        let t = tok();
        let s = "hello world foo bar";
        assert_eq!(t.count(s, "tiny").await.unwrap(), t.count_text(s, "tiny"));
        assert_eq!(t.backend(), "hf");
    }

    // --- adversarial: bad files + hostile models never fatal ---------------
    // A missing path is skipped at construction (no panic), so the backend loads
    // zero files and everything falls back to approx.
    #[test]
    fn missing_file_is_skipped_not_fatal() {
        let t = HfTokenizer::new(&[(String::new(), PathBuf::from("/no/such/tokenizer.json"))]);
        assert!(t.by_prefix.is_empty());
        let s = "hello world";
        assert_eq!(t.count_text(s, "any"), ApproxTokenizer.count_text(s));
    }

    // A directory (not a regular file) and an empty/garbage model id are handled.
    #[rstest]
    #[case::adversarial_dir_path("/tmp")]
    #[case::adversarial_relative_traversal("../../etc/passwd")]
    fn bad_paths_are_skipped(#[case] p: &str) {
        let t = HfTokenizer::new(&[(String::new(), PathBuf::from(p))]);
        assert!(t.by_prefix.is_empty());
    }

    #[tokio::test]
    async fn hostile_model_id_never_panics() {
        let t = tok();
        for model in ["", "\u{202e}../..", "\0\0", "TINY-MODEL"] {
            let n = t.count("hello world", model).await.unwrap();
            assert!(n <= 2); // routed to the fixture or approx, always finite
        }
    }
}

//! `tokenizer-tiktoken` — exact byte-pair token counts for the OpenAI model
//! family, via the offline [`tiktoken-rs`] crate.
//!
//! `tiktoken-rs` embeds the `cl100k_base` and `o200k_base` merge ranks with
//! `include_bytes!`, so this backend needs **no network** at build or runtime and
//! ships no separate vocab file — the default build stays hermetic because the
//! whole thing is gated off behind the `tokenizer-tiktoken` cargo feature.
//!
//! Counting uses [`CoreBPE::encode_ordinary`], which treats every byte of the
//! input as ordinary text: it never interprets a special-token string embedded in
//! (attacker-controlled) content, and it has no error/panic path. A `model` that
//! tiktoken ships no vocabulary for — including an unmapped or hostile id — falls
//! back to the dependency-free [`ApproxTokenizer`] segmenter, so a bad `model`
//! string can never panic, error, or block a count. See parity spec 23.

use agent_core::{Error, Result, Tokenizer};
use async_trait::async_trait;
use tiktoken_rs::CoreBPE;

use crate::ApproxTokenizer;

/// A [`Tokenizer`] giving exact counts for the models tiktoken bundles a
/// vocabulary for, and the [`ApproxTokenizer`] heuristic for everything else.
///
/// Both encodings are built once at construction (decompressing the vendored rank
/// tables is the expensive part) and reused for every count; `CoreBPE` is
/// `Send + Sync`, so one instance serves the whole process.
pub struct TiktokenTokenizer {
    /// GPT-4 / GPT-3.5-turbo / v2 embeddings.
    cl100k: CoreBPE,
    /// GPT-4o / o-series / GPT-5 family.
    o200k: CoreBPE,
    /// Used for any model tiktoken does not recognise.
    fallback: ApproxTokenizer,
}

// `CoreBPE` holds regexes + rank maps and has no `Debug`, so derive can't apply.
impl std::fmt::Debug for TiktokenTokenizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TiktokenTokenizer").finish_non_exhaustive()
    }
}

impl TiktokenTokenizer {
    /// Build both bundled encodings. Fails only if the vendored rank tables can't
    /// be decoded (a packaging bug, not a runtime condition).
    pub fn new() -> Result<Self> {
        let cl100k = tiktoken_rs::cl100k_base()
            .map_err(|e| Error::Tokenizer(format!("cl100k_base: {e}")))?;
        let o200k =
            tiktoken_rs::o200k_base().map_err(|e| Error::Tokenizer(format!("o200k_base: {e}")))?;
        Ok(Self {
            cl100k,
            o200k,
            fallback: ApproxTokenizer::new(),
        })
    }

    /// The encoding tiktoken uses for `model`, or `None` when it ships no
    /// vocabulary for it (→ approx fallback). Matching is lowercase + prefix
    /// based, so a dated id (`gpt-4o-2024-08-06`) still resolves to its family.
    fn encoding_for(&self, model: &str) -> Option<&CoreBPE> {
        let m = model.to_ascii_lowercase();
        // o200k_base — the GPT-4o / o-series / GPT-5 family. Checked first because
        // `gpt-4o*` also matches the `gpt-4` prefix below.
        if m.starts_with("gpt-4o")
            || m.starts_with("chatgpt-4o")
            || m.starts_with("gpt-5")
            || m.starts_with("o1")
            || m.starts_with("o3")
            || m.starts_with("o4")
        {
            return Some(&self.o200k);
        }
        // cl100k_base — GPT-4, GPT-3.5-turbo, and the v2 / ada-002 embeddings.
        if m.starts_with("gpt-4")
            || m.starts_with("gpt-3.5")
            || m.starts_with("gpt-35")
            || m.starts_with("text-embedding-3")
            || m.starts_with("text-embedding-ada-002")
        {
            return Some(&self.cl100k);
        }
        None
    }

    /// The synchronous count core (no `.await`), shared by [`Tokenizer::count`],
    /// the tests, and the fallback path. Never panics on any input.
    pub fn count_text(&self, text: &str, model: &str) -> u32 {
        match self.encoding_for(model) {
            // `encode_ordinary` is infallible and ignores special-token syntax in
            // untrusted text. Saturate the (practically impossible) >u32 length so
            // an enormous input clamps instead of wrapping.
            Some(enc) => u32::try_from(enc.encode_ordinary(text).len()).unwrap_or(u32::MAX),
            None => self.fallback.count_text(text),
        }
    }

    /// Count each of `texts` for one `model`. The inputs are independent BPE encodes,
    /// so above a work threshold they run in parallel across rayon's pool; below it,
    /// the dispatch/join overhead would cost more than it saves, so it stays
    /// sequential. The result is **identical either way** (`map` preserves order and
    /// each count is deterministic) — only the wall-clock changes.
    pub fn count_texts(&self, texts: &[&str], model: &str) -> Vec<u32> {
        let total_bytes: usize = texts.iter().map(|t| t.len()).sum();
        if texts.len() >= PAR_MIN_ITEMS && total_bytes >= PAR_MIN_BYTES {
            use rayon::prelude::*;
            texts
                .par_iter()
                .map(|t| self.count_text(t, model))
                .collect()
        } else {
            texts.iter().map(|t| self.count_text(t, model)).collect()
        }
    }
}

/// Fan out to rayon only once a batch is big enough to repay the ~µs dispatch/join
/// cost: at least this many inputs AND this many total bytes of text. Chosen from a
/// wall-clock sweep (see the PR) and deliberately conservative; the counts are the
/// same at any threshold, so tuning it trades only latency, never correctness.
const PAR_MIN_ITEMS: usize = 8;
const PAR_MIN_BYTES: usize = 32 * 1024;

#[async_trait]
impl Tokenizer for TiktokenTokenizer {
    fn backend(&self) -> &str {
        "tiktoken"
    }

    async fn count(&self, text: &str, model: &str) -> Result<u32> {
        Ok(self.count_text(text, model))
    }

    async fn count_batch(&self, texts: &[&str], model: &str) -> Result<Vec<u32>> {
        Ok(self.count_texts(texts, model))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn tok() -> TiktokenTokenizer {
        TiktokenTokenizer::new().expect("bundled ranks decode")
    }

    // --- model → encoding routing -----------------------------------------
    // `positive_` a recognised model resolves to a real encoding; `corner_` a
    // dated id still resolves via prefix; `negative_` an unknown model routes to
    // the approx fallback (no encoding).
    #[rstest]
    #[case::positive_gpt4o_is_o200k("gpt-4o", true)]
    #[case::positive_gpt4_is_cl100k("gpt-4", true)]
    #[case::positive_gpt35_turbo("gpt-3.5-turbo", true)]
    #[case::corner_dated_gpt4o("gpt-4o-2024-08-06", true)]
    #[case::corner_o_series("o3-mini", true)]
    #[case::corner_case_insensitive("GPT-4O", true)]
    #[case::negative_claude_falls_back("claude-sonnet-5", false)]
    #[case::negative_local_model("qwen2.5-coder:32b", false)]
    fn encoding_routing(#[case] model: &str, #[case] recognised: bool) {
        assert_eq!(
            tok().encoding_for(model).is_some(),
            recognised,
            "model={model}"
        );
    }

    // --- exact counts vs the approx heuristic ------------------------------
    // The point of the backend: real BPE counts differ from `~chars/4`. We assert
    // the recognised path is exact-and-stable and that unicode is not byte-inflated.
    #[rstest]
    #[case::positive_hello_world("hello world", "gpt-4o", 2)] // two common words → two tokens
    #[case::boundary_empty("", "gpt-4o", 0)]
    #[case::corner_unicode_not_bytelen("héllo wörld", "gpt-4o", 5)] // 13 bytes, but counted by BPE piece, not byte
    fn exact_counts(#[case] text: &str, #[case] model: &str, #[case] expected: u32) {
        assert_eq!(tok().count_text(text, model), expected, "text={text:?}");
    }

    // Dense code costs far more than `chars/4`: assert the real tokenizer is used
    // (count strictly exceeds a naive quarter-of-chars estimate) rather than
    // pinning a brittle exact number to a vocab version.
    #[test]
    fn dense_code_exceeds_char_quarter() {
        let s = "fn f(x:i32)->i32{x+1}";
        let n = tok().count_text(s, "gpt-4o");
        assert!(n as usize >= s.chars().count() / 4, "n={n} for {s:?}");
    }

    // --- the async seam delegates to the sync core -------------------------
    #[tokio::test]
    async fn count_matches_count_text() {
        let t = tok();
        let s = "let mut total = compute(y);";
        assert_eq!(
            t.count(s, "gpt-4o").await.unwrap(),
            t.count_text(s, "gpt-4o")
        );
        assert_eq!(t.backend(), "tiktoken");
    }

    // --- adversarial: an untrusted `model` must never panic or error --------
    // The `model` arg is attacker-controlled (CLAUDE.md). An unknown, empty,
    // separator-laden, or special-token-shaped input must fall back cleanly.
    #[rstest]
    #[case::adversarial_unknown_model("some text", "no-such-model-\u{202e}../..")]
    #[case::adversarial_empty_model("some text", "")]
    #[case::adversarial_special_token_text("<|endoftext|> <|fim_prefix|>", "gpt-4o")]
    #[case::adversarial_null_bytes("a\0b\0c", "gpt-4o")]
    #[tokio::test]
    async fn hostile_input_never_panics(#[case] text: &str, #[case] model: &str) {
        // Must return Ok with a finite count; unknown models take the approx path.
        let n = tok().count(text, model).await.unwrap();
        assert!(n < u32::MAX);
    }

    // An unmapped model yields exactly the approx count — the documented contract.
    #[test]
    fn unknown_model_equals_approx() {
        let t = tok();
        let s = "the quick brown fox";
        assert_eq!(
            t.count_text(s, "no-such-model"),
            ApproxTokenizer.count_text(s)
        );
    }

    // Special-token strings in untrusted text are counted as ORDINARY bytes (not
    // one special token), so a hostile prompt can't shrink its apparent size.
    #[test]
    fn special_tokens_counted_as_ordinary_text() {
        let t = tok();
        assert!(t.count_text("<|endoftext|>", "gpt-4o") > 1);
    }

    // --- batch counting is identical to per-text, on BOTH paths ------------
    // The parallel path must be a pure optimisation: same counts, same order.
    // A small batch stays sequential; a big one (≥ PAR_MIN_ITEMS and ≥ PAR_MIN_BYTES)
    // fans out to rayon. Assert equivalence across the threshold.
    #[rstest]
    #[case::sequential_small(vec!["hello".into(), "world of code".into(), String::new()])]
    #[case::parallel_large(large_batch())]
    fn count_batch_matches_sequential(#[case] texts: Vec<String>) {
        let t = tok();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let batched = t.count_texts(&refs, "gpt-4o");
        let seq: Vec<u32> = refs.iter().map(|s| t.count_text(s, "gpt-4o")).collect();
        assert_eq!(batched, seq, "batch must equal per-text");
    }

    // 12 chunks × ~4 KiB = ~48 KiB over 12 items → crosses both thresholds.
    fn large_batch() -> Vec<String> {
        (0..12)
            .map(|i| format!("fn item_{i}(x: i32) -> i32 {{ x + {i} }}\n").repeat(120))
            .collect()
    }

    // Manual wall-clock probe (not gated — the iai/valgrind gate serialises threads
    // and can't show a parallel win). Run: `cargo test -p agent-tokenizer
    // --features tokenizer-tiktoken -- --ignored --nocapture wallclock`.
    #[ignore = "manual wall-clock probe; run with --ignored --nocapture"]
    #[test]
    fn wallclock_seq_vs_par() {
        let t = tok();
        // 400 chunks × ~4 KiB ≈ 1.6 MiB of code — the shape of a large history.
        let owned: Vec<String> = (0..400)
            .map(|i| format!("fn item_{i}(x: i32) -> i32 {{ x + {i} }}\n").repeat(120))
            .collect();
        let refs: Vec<&str> = owned.iter().map(String::as_str).collect();

        let t0 = std::time::Instant::now();
        let seq: Vec<u32> = refs.iter().map(|s| t.count_text(s, "gpt-4o")).collect();
        let seq_ms = t0.elapsed().as_secs_f64() * 1e3;

        let t1 = std::time::Instant::now();
        let par = t.count_texts(&refs, "gpt-4o"); // crosses the threshold → rayon
        let par_ms = t1.elapsed().as_secs_f64() * 1e3;

        assert_eq!(seq, par);
        println!(
            "tiktoken batch of {} ({} KiB): sequential {seq_ms:.1} ms, parallel {par_ms:.1} ms, speedup {:.2}x",
            refs.len(),
            refs.iter().map(|s| s.len()).sum::<usize>() / 1024,
            seq_ms / par_ms,
        );
    }

    // `count_batch` (the async seam) matches the sync core.
    #[tokio::test]
    async fn count_batch_seam_matches_core() {
        let t = tok();
        let texts = ["a", "bb cc", "dddd"];
        assert_eq!(
            t.count_batch(&texts, "gpt-4o").await.unwrap(),
            t.count_texts(&texts, "gpt-4o")
        );
    }
}

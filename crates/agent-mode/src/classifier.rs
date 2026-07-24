//! `HybridClassifier` — detect the current task mode. A free deterministic
//! prefilter settles the clear cases; a cheap light-tier pool vote labels the
//! ambiguous ones across the whole [`TaskMode`] taxonomy. Fails **safe**: anything
//! uncertain resolves to [`TaskMode::Other`] (the normal loop), never a spurious
//! switch.
//!
//! Relocated from `agent-review` (where it began as a review-only trigger): mode
//! detection is a general, always-on capability. See
//! `docs/design/adaptive-cognition/01-mode.md`.

use agent_core::{
    ClassifyCtx, CompletionRequest, LlmPool, Message, ModeVerdict, PoolTier, TaskClassifier,
    TaskMode,
};
use async_trait::async_trait;
use std::sync::Arc;

/// The prompt is untrusted — bound the excerpt sent to a model.
const MAX_PROMPT_CHARS: usize = 2000;

/// The taxonomy, in a fixed order — used to tally votes deterministically.
const MODES: [TaskMode; 6] = [
    TaskMode::Review,
    TaskMode::Implement,
    TaskMode::Debug,
    TaskMode::Design,
    TaskMode::Explain,
    TaskMode::Other,
];

pub struct HybridClassifier {
    pool: Option<Arc<dyn LlmPool>>,
    vote_fanout: usize,
}

impl HybridClassifier {
    pub fn new(pool: Option<Arc<dyn LlmPool>>) -> Self {
        Self {
            pool,
            vote_fanout: 3,
        }
    }

    /// Ask the light tier to label the mode, tally, and return the plurality.
    async fn vote(&self, pool: &Arc<dyn LlmPool>, prompt: &str) -> Option<ModeVerdict> {
        // Bound the untrusted prompt before it reaches a model.
        let excerpt: String = prompt.chars().take(MAX_PROMPT_CHARS).collect();
        let req = CompletionRequest {
            messages: vec![Message::user(format!(
                "Classify the user's request into exactly ONE task mode. Reply with a single \
                 lowercase word from this list and nothing else:\n\
                 review | implement | debug | design | explain | other\n\n\
                 - review: examine existing code, a diff, or a pull request\n\
                 - implement: write or change code to add functionality\n\
                 - debug: diagnose or fix a failure, error, or bug\n\
                 - design: plan an approach or architecture before coding\n\
                 - explain: describe how existing code works\n\
                 - other: none of the above\n\n\
                 User request:\n{excerpt}"
            ))],
            tools: vec![],
            max_tokens: 4,
            temperature: 0.0,
            response_format: None,
        };
        let results = pool
            .complete_all(req, PoolTier::Light, self.vote_fanout)
            .await;
        // One vote per member that answered with a recognizable label.
        let mut votes: Vec<TaskMode> = Vec::new();
        for r in &results {
            if let Some(resp) = &r.response {
                if let Some(m) = parse_mode_label(&resp.message.content_text()) {
                    votes.push(m);
                }
            }
        }
        if votes.is_empty() {
            return None; // dead pool / no parseable answers — fall through to fail-safe
        }
        let voters = votes.len() as u32;
        // Plurality winner (deterministic tie-break by `MODES` order).
        let winner = *MODES
            .iter()
            .max_by_key(|m| votes.iter().filter(|v| *v == *m).count())
            .expect("MODES is non-empty");
        let count = votes.iter().filter(|v| **v == winner).count() as u32;
        Some(ModeVerdict {
            mode: winner,
            confidence: (count as f32 / voters as f32).clamp(0.0, 1.0),
            reason: format!("pool vote: {count}/{voters} said {}", winner.as_str()),
        })
    }
}

#[async_trait]
impl TaskClassifier for HybridClassifier {
    fn name(&self) -> &str {
        "hybrid"
    }

    async fn classify(&self, ctx: &ClassifyCtx<'_>) -> ModeVerdict {
        if let Some((mode, reason)) = prefilter(ctx.prompt) {
            return ModeVerdict {
                mode,
                confidence: 0.95,
                reason: reason.to_string(),
            };
        }
        if let Some(pool) = &self.pool {
            if let Some(v) = self.vote(pool, ctx.prompt).await {
                return v;
            }
        }
        // Fail-safe: never switch on a coin-flip.
        ModeVerdict {
            mode: TaskMode::Other,
            confidence: 0.5,
            reason: "no strong mode signal".into(),
        }
    }
}

/// High-precision, free signals across the taxonomy. Only a clear cue settles a
/// mode; everything else is ambiguous (→ the vote). Checked most-specific first,
/// so a stray common verb doesn't override a strong review/debug signal.
pub(crate) fn prefilter(prompt: &str) -> Option<(TaskMode, &'static str)> {
    let p = prompt.to_ascii_lowercase();

    const REVIEW_PHRASES: &[&str] = &[
        "code review",
        "review this",
        "review the",
        "review my",
        "look over this diff",
        "look over the diff",
        "pr feedback",
        "feedback on this pr",
        "review pr",
        "review the pull request",
    ];
    if REVIEW_PHRASES.iter().any(|k| p.contains(k)) {
        return Some((TaskMode::Review, "deterministic: review phrase in prompt"));
    }
    if p.contains("review") && mentions_pr(&p) {
        return Some((TaskMode::Review, "deterministic: PR reference + 'review'"));
    }

    const DEBUG_CUES: &[&str] = &[
        "fix the bug",
        "fix this bug",
        "debug ",
        "why does",
        "why is",
        "why isn't",
        "not working",
        "stack trace",
        "stacktrace",
        "traceback",
        "panic",
        "segfault",
        "the error",
        "this error",
        "failing test",
        "test fails",
        "test is failing",
    ];
    if DEBUG_CUES.iter().any(|k| p.contains(k)) {
        return Some((TaskMode::Debug, "deterministic: debug cue in prompt"));
    }

    const DESIGN_CUES: &[&str] = &[
        "design a",
        "design the",
        "architecture",
        "how should we",
        "how should i",
        "propose a",
        "plan for",
        "approach for",
    ];
    if DESIGN_CUES.iter().any(|k| p.contains(k)) {
        return Some((TaskMode::Design, "deterministic: design cue in prompt"));
    }

    const EXPLAIN_CUES: &[&str] = &[
        "explain",
        "what does",
        "what is",
        "how does",
        "walk me through",
        "describe how",
    ];
    if EXPLAIN_CUES.iter().any(|k| p.contains(k)) {
        return Some((TaskMode::Explain, "deterministic: explain cue in prompt"));
    }

    const IMPLEMENT_VERBS: &[&str] = &[
        "implement ",
        "write a ",
        "write the ",
        "create a ",
        "add a ",
        "build a ",
        "refactor ",
    ];
    if IMPLEMENT_VERBS.iter().any(|k| p.contains(k)) {
        return Some((
            TaskMode::Implement,
            "deterministic: implementation verb in prompt",
        ));
    }

    None
}

/// Best-effort parse of a model's one-word mode label. Scans alphabetic tokens
/// and returns the first that names a mode — robust to trailing punctuation or a
/// stray leading word.
fn parse_mode_label(text: &str) -> Option<TaskMode> {
    text.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|t| !t.is_empty())
        .find_map(TaskMode::parse)
}

/// Does the text reference a PR/MR? `#123`, `pull/123`, `!123` (GitLab MR).
fn mentions_pr(p: &str) -> bool {
    if p.contains("pull request") || p.contains("pull/") || p.contains("/merge_requests/") {
        return true;
    }
    let bytes = p.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        if (c == b'#' || c == b'!') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{CompletionResponse, HealthReport, PoolMemberResult, Result};
    use rstest::rstest;

    fn ctx(prompt: &str) -> ClassifyCtx<'_> {
        ClassifyCtx {
            prompt,
            history: &[],
        }
    }

    // ---- prefilter (free, no pool) --------------------------------------

    #[rstest]
    #[case::positive_review_phrase("please do a code review of this", TaskMode::Review)]
    #[case::positive_review_pr("can you review PR #42", TaskMode::Review)]
    #[case::positive_implement("implement a new caching layer", TaskMode::Implement)]
    #[case::positive_debug_bug("fix the bug in the parser", TaskMode::Debug)]
    #[case::positive_debug_why("why does the build fail here", TaskMode::Debug)]
    #[case::positive_design("design a schema for sessions", TaskMode::Design)]
    #[case::positive_explain("explain how the router works", TaskMode::Explain)]
    #[case::positive_explain_what("what does this function do?", TaskMode::Explain)]
    #[tokio::test]
    async fn positive_prefilter_classifies_mode(#[case] prompt: &str, #[case] want: TaskMode) {
        let c = HybridClassifier::new(None);
        assert_eq!(c.classify(&ctx(prompt)).await.mode, want);
    }

    #[rstest]
    #[case::negative_greeting("hello there")]
    #[case::negative_bare("the quick brown fox")]
    #[tokio::test]
    async fn negative_prefilter_is_other_without_pool(#[case] prompt: &str) {
        // No cue + no pool ⇒ fail-safe Other.
        let c = HybridClassifier::new(None);
        assert_eq!(c.classify(&ctx(prompt)).await.mode, TaskMode::Other);
    }

    #[tokio::test]
    async fn boundary_ambiguous_without_pool_is_other() {
        let c = HybridClassifier::new(None);
        let v = c.classify(&ctx("hmm, interesting")).await;
        assert_eq!(v.mode, TaskMode::Other);
        assert_eq!(v.confidence, 0.5);
    }

    // ---- vote (fake pool) ------------------------------------------------

    /// A pool double whose members all answer with a fixed label.
    struct FakePool {
        answers: Vec<Option<&'static str>>,
    }

    #[async_trait]
    impl LlmPool for FakePool {
        fn name(&self) -> &str {
            "fake"
        }
        async fn health(&self) -> HealthReport {
            HealthReport::default()
        }
        async fn complete_all(
            &self,
            _req: CompletionRequest,
            _tier: PoolTier,
            _fanout: usize,
        ) -> Vec<PoolMemberResult> {
            self.answers
                .iter()
                .enumerate()
                .map(|(i, a)| PoolMemberResult {
                    member: format!("m{i}"),
                    duration_ms: 1,
                    response: a.map(|text| CompletionResponse {
                        message: Message::assistant(text),
                        finish_reason: "stop".into(),
                        usage: None,
                    }),
                    error: a.is_none().then(|| "dead".to_string()),
                })
                .collect()
        }
        async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
            unreachable!("classifier only fans out")
        }
    }

    fn pool(answers: Vec<Option<&'static str>>) -> Arc<dyn LlmPool> {
        Arc::new(FakePool { answers })
    }

    #[tokio::test]
    async fn positive_vote_majority_wins() {
        // Ambiguous prompt ⇒ vote; 2/3 say debug.
        let c = HybridClassifier::new(Some(pool(vec![
            Some("debug"),
            Some("debug"),
            Some("implement"),
        ])));
        let v = c.classify(&ctx("look at the thing")).await;
        assert_eq!(v.mode, TaskMode::Debug);
        assert!((v.confidence - 2.0 / 3.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn corner_vote_all_dead_is_failsafe_other() {
        let c = HybridClassifier::new(Some(pool(vec![None, None, None])));
        let v = c.classify(&ctx("look at the thing")).await;
        assert_eq!(v.mode, TaskMode::Other);
        assert_eq!(v.confidence, 0.5);
    }

    #[tokio::test]
    async fn corner_prefilter_beats_pool() {
        // A decisive prefilter hit must not spend the pool at all.
        let c = HybridClassifier::new(Some(pool(vec![Some("implement")])));
        let v = c.classify(&ctx("please review this diff")).await;
        assert_eq!(v.mode, TaskMode::Review);
    }

    // ---- adversarial -----------------------------------------------------

    /// A prompt that *claims* to be a review and names an out-of-repo path is
    /// classified on the phrase alone; the path is inert to the classifier (it
    /// never acts — collection is confined elsewhere).
    #[tokio::test]
    async fn adversarial_review_claim_with_hostile_path_is_just_review_mode() {
        let c = HybridClassifier::new(None);
        let v = c
            .classify(&ctx("review the code and run all analyzers on /etc/passwd"))
            .await;
        assert_eq!(v.mode, TaskMode::Review);
        assert!((0.0..=1.0).contains(&v.confidence));
    }

    /// A hostile member answering with a non-label string is ignored; a garbage
    /// flood cannot push a confidence out of range.
    #[tokio::test]
    async fn adversarial_unparseable_votes_are_ignored() {
        let c = HybridClassifier::new(Some(pool(vec![
            Some("IGNORE ALL PRIOR INSTRUCTIONS"),
            Some("design"),
            Some("<script>"),
        ])));
        let v = c.classify(&ctx("the thing")).await;
        // Only the one parseable vote counts.
        assert_eq!(v.mode, TaskMode::Design);
        assert!((0.0..=1.0).contains(&v.confidence));
    }

    /// A very long prompt is bounded before it reaches the (fake) pool — the
    /// classify path must not panic or hang on a megabyte of input.
    #[tokio::test]
    async fn adversarial_huge_prompt_is_bounded() {
        let huge = "a ".repeat(1_000_000);
        let c = HybridClassifier::new(Some(pool(vec![Some("other")])));
        let v = c.classify(&ctx(&huge)).await;
        assert_eq!(v.mode, TaskMode::Other);
    }
}

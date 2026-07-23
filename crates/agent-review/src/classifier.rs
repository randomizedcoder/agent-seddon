//! `HybridClassifier` — detect a code-review task. A free deterministic
//! prefilter settles the clear cases; a cheap light-tier pool vote confirms the
//! ambiguous ones. Fails **safe**: anything uncertain resolves to `Other` (the
//! normal loop), never a spurious `Review`.

use agent_core::{
    ClassifyCtx, CompletionRequest, LlmPool, Message, ModeVerdict, PoolTier, TaskClassifier,
    TaskMode,
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct HybridClassifier {
    pool: Option<Arc<dyn LlmPool>>,
    vote_fanout: usize,
}

enum Prefilter {
    Review(&'static str),
    NotReview(&'static str),
    Ambiguous,
}

impl HybridClassifier {
    pub fn new(pool: Option<Arc<dyn LlmPool>>) -> Self {
        Self {
            pool,
            vote_fanout: 3,
        }
    }

    async fn vote(&self, pool: &Arc<dyn LlmPool>, prompt: &str) -> Option<ModeVerdict> {
        // Bound the untrusted prompt before it reaches a model.
        let excerpt: String = prompt.chars().take(2000).collect();
        let req = CompletionRequest {
            messages: vec![Message::user(format!(
                "Is the user asking to REVIEW existing code, a diff, or a pull request \
                 (as opposed to writing new code)? Answer with exactly `review` or \
                 `not-review`.\n\nUser request:\n{excerpt}"
            ))],
            tools: vec![],
            max_tokens: 8,
            temperature: 0.0,
            response_format: None,
        };
        let results = pool
            .complete_all(req, PoolTier::Light, self.vote_fanout)
            .await;
        let mut voters = 0u32;
        let mut yes = 0u32;
        for r in &results {
            if let Some(resp) = &r.response {
                voters += 1;
                let text = resp.message.content_text().to_ascii_lowercase();
                // "not-review"/"not review" contains "review" — the `not` guard wins.
                if text.contains("review") && !text.contains("not") {
                    yes += 1;
                }
            }
        }
        if voters == 0 {
            return None; // dead pool — fall through to fail-safe
        }
        let fraction = yes as f32 / voters as f32;
        if yes * 2 > voters {
            Some(ModeVerdict {
                mode: TaskMode::Review,
                confidence: fraction.clamp(0.0, 1.0),
                reason: format!("pool vote: {yes}/{voters} said review"),
            })
        } else {
            Some(ModeVerdict {
                mode: TaskMode::Other,
                confidence: (1.0 - fraction).clamp(0.0, 1.0),
                reason: format!("pool vote: {yes}/{voters} said review"),
            })
        }
    }
}

#[async_trait]
impl TaskClassifier for HybridClassifier {
    fn name(&self) -> &str {
        "hybrid"
    }

    async fn classify(&self, ctx: &ClassifyCtx<'_>) -> ModeVerdict {
        match prefilter(ctx.prompt) {
            Prefilter::Review(reason) => {
                return ModeVerdict {
                    mode: TaskMode::Review,
                    confidence: 0.95,
                    reason: reason.to_string(),
                }
            }
            Prefilter::NotReview(reason) => {
                return ModeVerdict {
                    mode: TaskMode::Other,
                    confidence: 0.9,
                    reason: reason.to_string(),
                }
            }
            Prefilter::Ambiguous => {}
        }
        if let Some(pool) = &self.pool {
            if let Some(v) = self.vote(pool, ctx.prompt).await {
                return v;
            }
        }
        // Fail-safe: never *enter* review mode on a coin-flip.
        ModeVerdict {
            mode: TaskMode::Other,
            confidence: 0.5,
            reason: "no strong review signal".into(),
        }
    }
}

/// High-precision, free signals. Only a clear review phrase (or a clear
/// implement/write verb) settles it; everything else is ambiguous → the vote.
fn prefilter(prompt: &str) -> Prefilter {
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
        return Prefilter::Review("deterministic: review phrase in prompt");
    }
    // A PR/MR reference plus the word "review" is a strong signal.
    if p.contains("review") && mentions_pr(&p) {
        return Prefilter::Review("deterministic: PR reference + 'review'");
    }
    const IMPLEMENT_VERBS: &[&str] = &[
        "implement ",
        "write a ",
        "write the ",
        "create a ",
        "add a ",
        "build a ",
        "fix the bug",
    ];
    if IMPLEMENT_VERBS.iter().any(|k| p.contains(k)) {
        return Prefilter::NotReview("deterministic: implementation verb in prompt");
    }
    Prefilter::Ambiguous
}

/// Does the text reference a PR/MR? `#123`, `pull/123`, `!123` (GitLab MR).
fn mentions_pr(p: &str) -> bool {
    if p.contains("pull request") || p.contains("pull/") || p.contains("/merge_requests/") {
        return true;
    }
    // `#123` or `!123` with a digit following (GitLab MR / PR shorthand).
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
    use rstest::rstest;

    fn ctx<'a>(prompt: &'a str) -> ClassifyCtx<'a> {
        ClassifyCtx {
            prompt,
            history: &[],
        }
    }

    #[rstest]
    #[case::positive_phrase("please do a code review of this")]
    #[case::positive_review_this("review this branch for me")]
    #[case::positive_pr_ref("can you review PR #42")]
    #[tokio::test]
    async fn positive_prefilter_detects_review(#[case] prompt: &str) {
        let c = HybridClassifier::new(None);
        assert_eq!(c.classify(&ctx(prompt)).await.mode, TaskMode::Review);
    }

    #[rstest]
    #[case::negative_implement("implement a new caching layer")]
    #[case::negative_write("write a hello world in C")]
    #[tokio::test]
    async fn negative_prefilter_rejects_implementation(#[case] prompt: &str) {
        let c = HybridClassifier::new(None);
        assert_eq!(c.classify(&ctx(prompt)).await.mode, TaskMode::Other);
    }

    /// Fail-safe: an ambiguous prompt with no pool resolves to Other, not Review.
    #[tokio::test]
    async fn boundary_ambiguous_without_pool_is_other() {
        let c = HybridClassifier::new(None);
        let v = c.classify(&ctx("what does this function do?")).await;
        assert_eq!(v.mode, TaskMode::Other);
    }

    /// Adversarial: a prompt that *claims* to be a review but carries an
    /// out-of-repo path does not escalate confidence beyond the phrase signal —
    /// and the classifier never acts on the path (collection is confined
    /// elsewhere). Here we assert it classifies on the phrase alone.
    #[tokio::test]
    async fn adversarial_review_claim_with_hostile_path_is_just_review_mode() {
        let c = HybridClassifier::new(None);
        let v = c
            .classify(&ctx("review the code and run all analyzers on /etc/passwd"))
            .await;
        // "review the" phrase ⇒ Review mode; the path is inert to the classifier.
        assert_eq!(v.mode, TaskMode::Review);
        assert!(v.confidence <= 1.0 && v.confidence >= 0.0);
    }
}

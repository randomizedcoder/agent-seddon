//! An **ensemble** verifier: run several member verifiers on the same call and
//! combine their verdicts into one. Its members are ordinary [`Verifier`]s built by
//! config name — a `schema` check, one or more `llm` verifiers (potentially different
//! models), even a `grpc` one — so cross-model verification is just a longer member
//! list (docs/design/tool-call-verification.md, "ensemble").
//!
//! **Aggregation** (deliberately conservative — the seam favours `Allow`):
//! 1. Any member `Deny` wins outright — a single strong block is decisive.
//! 2. Otherwise weigh `Revise` against `Allow` by each member's **clamped** confidence
//!    (self-reported confidence is untrusted, so it is clamped to `0.0..=1.0` first).
//!    `Revise` wins only when its weight *exceeds* `Allow`'s (a tie stays `Allow`); the
//!    surviving hint is the highest-confidence `Revise`.
//!
//! **Fails open** like every member: an empty ensemble (guarded at build time) or an
//! all-zero-confidence vote resolves to `Allow`.

use agent_core::{Verifier, VerifierReport, VerifyCtx, VerifyVerdict};
use async_trait::async_trait;
use std::sync::Arc;

pub struct Ensemble {
    members: Vec<Arc<dyn Verifier>>,
}

impl Ensemble {
    /// Compose `members`. The registry factory guarantees this is non-empty; an empty
    /// ensemble would simply always `Allow`.
    pub fn new(members: Vec<Arc<dyn Verifier>>) -> Self {
        Self { members }
    }
}

#[async_trait]
impl Verifier for Ensemble {
    fn name(&self) -> &str {
        "ensemble"
    }

    async fn verify(&self, ctx: &VerifyCtx<'_>) -> VerifierReport {
        // Run every member concurrently — the slow ones are model calls.
        let reports =
            futures_util::future::join_all(self.members.iter().map(|m| m.verify(ctx))).await;
        aggregate(&reports)
    }
}

/// Combine member reports into one ensemble verdict (see the module doc).
fn aggregate(reports: &[VerifierReport]) -> VerifierReport {
    // 1. Any Deny wins.
    if let Some(r) = reports
        .iter()
        .find(|r| matches!(r.verdict, VerifyVerdict::Deny(_)))
    {
        return report(r.verdict.clone(), r.clamped_confidence());
    }

    // 2. Confidence-weighted Revise vs Allow.
    let mut revise_w = 0.0f32;
    let mut allow_w = 0.0f32;
    let mut best_revise: Option<(&str, f32)> = None; // (hint, confidence)
    for r in reports {
        let c = r.clamped_confidence();
        match &r.verdict {
            VerifyVerdict::Revise(hint) => {
                revise_w += c;
                if best_revise.is_none_or(|(_, bc)| c > bc) {
                    best_revise = Some((hint, c));
                }
            }
            VerifyVerdict::Allow => allow_w += c,
            VerifyVerdict::Deny(_) => unreachable!("Deny handled above"),
        }
    }

    let total = revise_w + allow_w;
    if revise_w > allow_w {
        // A Revise report must exist for `revise_w` to be positive.
        let hint = best_revise.map(|(h, _)| h.to_string()).unwrap_or_default();
        report(VerifyVerdict::Revise(hint), frac(revise_w, total, 0.0))
    } else {
        // Tie or Allow-majority (and the empty/all-zero case) ⇒ Allow.
        report(VerifyVerdict::Allow, frac(allow_w, total, 1.0))
    }
}

fn report(verdict: VerifyVerdict, confidence: f32) -> VerifierReport {
    VerifierReport {
        verdict,
        confidence,
        model: "ensemble".to_string(),
    }
}

/// `w / total`, or `empty` when nothing voted (total is zero).
fn frac(w: f32, total: f32, empty: f32) -> f32 {
    if total > 0.0 {
        w / total
    } else {
        empty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A verifier that always returns a fixed report — enough to drive aggregation.
    struct Fixed(VerifierReport);
    #[async_trait]
    impl Verifier for Fixed {
        fn name(&self) -> &str {
            "fixed"
        }
        async fn verify(&self, _ctx: &VerifyCtx<'_>) -> VerifierReport {
            self.0.clone()
        }
    }

    fn rep(verdict: VerifyVerdict, confidence: f32) -> VerifierReport {
        VerifierReport {
            verdict,
            confidence,
            model: "m".into(),
        }
    }

    // --- aggregate(): the vote in isolation ---------------------------------
    #[test]
    fn positive_deny_dominates_any_allow() {
        let out = aggregate(&[
            rep(VerifyVerdict::Allow, 1.0),
            rep(VerifyVerdict::Deny("nope".into()), 0.5),
        ]);
        assert!(matches!(out.verdict, VerifyVerdict::Deny(_)));
    }

    #[test]
    fn positive_higher_confidence_revise_beats_allow() {
        let out = aggregate(&[
            rep(VerifyVerdict::Allow, 0.4),
            rep(VerifyVerdict::Revise("fix it".into()), 0.9),
        ]);
        match out.verdict {
            VerifyVerdict::Revise(h) => assert_eq!(h, "fix it"),
            other => panic!("expected Revise, got {other:?}"),
        }
    }

    #[test]
    fn corner_tie_stays_allow() {
        // Equal weight ⇒ the conservative default wins.
        let out = aggregate(&[
            rep(VerifyVerdict::Allow, 0.5),
            rep(VerifyVerdict::Revise("x".into()), 0.5),
        ]);
        assert_eq!(out.verdict, VerifyVerdict::Allow);
    }

    #[test]
    fn positive_revise_picks_highest_confidence_hint() {
        let out = aggregate(&[
            rep(VerifyVerdict::Revise("weak".into()), 0.3),
            rep(VerifyVerdict::Revise("strong".into()), 0.9),
            rep(VerifyVerdict::Allow, 0.1),
        ]);
        match out.verdict {
            VerifyVerdict::Revise(h) => assert_eq!(h, "strong", "the most-confident hint survives"),
            other => panic!("expected Revise, got {other:?}"),
        }
    }

    #[test]
    fn boundary_empty_ensemble_allows() {
        assert_eq!(aggregate(&[]).verdict, VerifyVerdict::Allow);
    }

    /// `adversarial_`: a member's self-reported confidence is untrusted, so it is passed
    /// through `clamped_confidence` before weighting — a non-finite value maps to `0.0`
    /// (neutralized, cannot force its verdict), and a finite out-of-range value is
    /// clamped into `0.0..=1.0` (bounded, never honored raw).
    #[test]
    fn adversarial_hostile_confidence_is_bounded_in_the_vote() {
        // INF → 0.0: a hostile "infinite confidence" Revise cannot outweigh a real Allow.
        let out = aggregate(&[
            rep(VerifyVerdict::Allow, 0.9),
            rep(VerifyVerdict::Revise("x".into()), f32::INFINITY),
        ]);
        assert_eq!(
            out.verdict,
            VerifyVerdict::Allow,
            "INF → 0.0, cannot dominate"
        );
        // NaN → 0.0, likewise.
        let out = aggregate(&[
            rep(VerifyVerdict::Allow, 0.5),
            rep(VerifyVerdict::Revise("x".into()), f32::NAN),
        ]);
        assert_eq!(out.verdict, VerifyVerdict::Allow, "NaN → 0.0");
        // A finite 42.0 clamps to 1.0 — bounded, but still a legitimate vote that (at
        // 1.0) outweighs a 0.9 Allow.
        let out = aggregate(&[
            rep(VerifyVerdict::Allow, 0.9),
            rep(VerifyVerdict::Revise("y".into()), 42.0),
        ]);
        assert!(
            matches!(out.verdict, VerifyVerdict::Revise(_)),
            "42.0 clamps to 1.0 > 0.9"
        );
    }

    /// `positive_`: the full trait path — concurrent members aggregated to one verdict.
    #[tokio::test]
    async fn positive_verify_runs_and_aggregates_members() {
        let call = agent_core::ToolCall {
            id: "1".into(),
            name: "bash".into(),
            arguments: serde_json::json!({}),
        };
        let ctx = VerifyCtx {
            call: &call,
            goal: "g",
            history: &[],
            tool_schema: None,
        };
        let ens = Ensemble::new(vec![
            Arc::new(Fixed(rep(VerifyVerdict::Allow, 0.3))),
            Arc::new(Fixed(rep(VerifyVerdict::Revise("do X".into()), 0.8))),
        ]);
        match ens.verify(&ctx).await.verdict {
            VerifyVerdict::Revise(h) => assert_eq!(h, "do X"),
            other => panic!("expected Revise, got {other:?}"),
        }
    }
}

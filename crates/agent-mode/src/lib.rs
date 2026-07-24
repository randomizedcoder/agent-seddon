//! `agent-mode` — general task-mode detection (the [`agent_core::TaskClassifier`]
//! seam).
//!
//! Detects which [`agent_core::TaskMode`] the current work is — every turn — so the
//! loop can switch to a more appropriate mode. It began life inside `agent-review`
//! as a review-only trigger; mode detection is a *general* capability, so it lives
//! here now and `agent-review` is one consumer of the result. The model is only
//! ever asked to *vote* on the mode, never to supply a fact.
//!
//! See `docs/design/adaptive-cognition/01-mode.md`.

mod classifier;
pub use classifier::HybridClassifier;

/// The deterministic prefilter over a prompt — the free, every-turn hot path.
/// Exposed for the instruction-count bench (`benches/classify.rs`); returns the
/// decisively-detected mode, or `None` when the prompt is ambiguous.
pub fn bench_prefilter(prompt: &str) -> Option<agent_core::TaskMode> {
    classifier::prefilter(prompt).map(|(m, _)| m)
}

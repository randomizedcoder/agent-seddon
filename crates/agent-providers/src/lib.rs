//! LLM provider implementations behind the `LlmProvider` seam.
//!
//! Each provider is gated by a cargo feature so a build links only the ones it
//! needs:
//!   * `provider-openai-compat` — OpenAI-compatible chat-completions (GLM,
//!     OpenAI, local vLLM/Ollama, …).
//!   * `provider-anthropic` — Anthropic-native Messages API.
//!
//! Adding a provider is a new module here plus a registry line in
//! `agent-runtime` — the loop never changes. See `docs/extending.md`.

#[cfg(feature = "provider-openai-compat")]
mod openai_compat;
#[cfg(feature = "provider-openai-compat")]
pub use openai_compat::{OpenAiCompatConfig, OpenAiCompatProvider};

#[cfg(feature = "provider-anthropic")]
mod anthropic;
#[cfg(feature = "provider-anthropic")]
pub use anthropic::{AnthropicConfig, AnthropicProvider};

// ---- Streaming decode caps (fail-closed against a hostile/broken endpoint) ----------
//
// A provider response is attacker-controlled (CLAUDE.md: "every provider/server value").
// The SSE decode loops in both providers accumulate bytes incrementally, so — like every
// other buffered surface in the tree — they must be *capped* before the accumulation, or a
// server that streams a never-ending "line", an endless run of tool-argument fragments, or
// an unbounded set of tool-call indices can grow memory without limit and OOM the process
// (a 600s client timeout doesn't help — a fast server delivers gigabytes well within it).
#[cfg(any(feature = "provider-openai-compat", feature = "provider-anthropic"))]
pub(crate) mod stream_caps {
    /// Max bytes an incomplete (un-newline-terminated) SSE frame may buffer. A real frame
    /// is a single small JSON event; 8 MiB is orders of magnitude above any legitimate one,
    /// so exceeding it without a delimiter is a hostile/broken stream → error out.
    pub const MAX_STREAM_BUF_BYTES: usize = 8 * 1024 * 1024;
    /// Max total accumulated tool-call argument JSON for a single streamed call (the value
    /// is then `serde_json`-parsed, so this also bounds the parse input).
    pub const MAX_STREAM_TOOL_ARG_BYTES: usize = 4 * 1024 * 1024;
    /// Max distinct tool-call slots one stream may open (server-supplied index / block id).
    pub const MAX_STREAM_TOOL_CALLS: usize = 512;
}
#[cfg(any(feature = "provider-openai-compat", feature = "provider-anthropic"))]
pub use stream_caps::{MAX_STREAM_BUF_BYTES, MAX_STREAM_TOOL_ARG_BYTES, MAX_STREAM_TOOL_CALLS};

/// Provider routing + failover (parity spec 25). A `Router` IS-A `LlmProvider`,
/// so nothing downstream knows it composes others.
#[cfg(feature = "provider-router")]
pub mod router;
#[cfg(feature = "provider-router")]
pub use router::{Candidate, RouteEvent, RoutePolicy, Router};

/// Declarative task-aware routing — the pure decision engine (model-router
/// increment 02). Depends only on `agent_core::PoolTier`, so it is always compiled.
pub mod route;

/// `TaskRouter` — a provider that routes each request to a declaratively-preferred,
/// capable upstream (via [`route::Policy`]) and fails over on a retryable error.
/// Reuses the router's circuit breaker, so it is gated on `provider-router`.
#[cfg(feature = "provider-router")]
pub mod task_router;
#[cfg(feature = "provider-router")]
pub use task_router::{RouterUpstream, TaskRouter};

// The registry-backed task router (model-router 04): fleet + policy from a live
// `ProviderRegistry` snapshot, rebuilt only when the config fingerprint changes.
#[cfg(feature = "provider-router")]
pub mod registry_router;
#[cfg(feature = "provider-router")]
pub use registry_router::{RegistryRouter, UpstreamSynth};

/// A health-checked, tiered pool of cheap providers with an active liveness probe
/// and parallel fan-out (`docs/design/code-review/llm-pool.md`). Reuses the
/// router's circuit breaker, so `provider-pool` implies `provider-router`.
#[cfg(feature = "provider-pool")]
pub mod pool;
#[cfg(feature = "provider-pool")]
pub use pool::{PoolEvent, PoolObserver, PoolPolicy, PoolProvider, PoolSpec, Saturation};

/// `ConsensusProvider` — the response-level generator × critic gate with a bounded
/// convergence loop and an alternatives ledger (cognition-graph increment 01,
/// docs/design/cognition-graph/01-consensus-gate.md).
#[cfg(feature = "provider-consensus")]
pub mod consensus;
#[cfg(feature = "provider-consensus")]
pub use consensus::{
    AlternativeOption, ConsensusProvider, EvidenceSource, Exhaustion, GateCfg, GateObserver,
    GateOutcome, GateOutcomeKind, GateScope,
};

/// Parallel branches: split → concurrent lens branches → policy join → merge
/// (cognition-graph increment 05,
/// docs/design/cognition-graph/05-parallel-branches.md).
#[cfg(feature = "provider-branching")]
pub mod branching;
#[cfg(feature = "provider-branching")]
pub use branching::{
    BranchCfg, BranchFate, BranchObserver, BranchReport, BranchSpec, BranchingProvider, JoinPolicy,
    MergeStrategy, OnTimeout,
};

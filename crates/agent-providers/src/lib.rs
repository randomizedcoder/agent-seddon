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

/// Provider routing + failover (parity spec 25). A `Router` IS-A `LlmProvider`,
/// so nothing downstream knows it composes others.
#[cfg(feature = "provider-router")]
pub mod router;
#[cfg(feature = "provider-router")]
pub use router::{Candidate, RouteEvent, RoutePolicy, Router};

/// A health-checked, tiered pool of cheap providers with an active liveness probe
/// and parallel fan-out (`docs/design/code-review/llm-pool.md`). Reuses the
/// router's circuit breaker, so `provider-pool` implies `provider-router`.
#[cfg(feature = "provider-pool")]
pub mod pool;
#[cfg(feature = "provider-pool")]
pub use pool::{PoolEvent, PoolObserver, PoolProvider, PoolSpec};

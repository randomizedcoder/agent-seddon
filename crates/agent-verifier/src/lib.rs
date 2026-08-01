//! Verifier seam implementations (the `Verifier` trait in `agent-core`).
//!
//! A verifier is a correctness gate on a requested tool call, checked before it
//! runs — see [`agent_core::Verifier`] and
//! `docs/design/tool-call-verification.md`. This crate holds the concrete
//! backends; the loop wiring and the shadow/enforce modes live in `agent-runtime`.
//!
//! Increment 1 ships one backend: [`SchemaVerifier`], a deterministic,
//! model-free check of a call's arguments against the tool's JSON Schema.

#[cfg(feature = "verifier-schema")]
mod schema;
#[cfg(feature = "verifier-schema")]
pub use schema::SchemaVerifier;

#[cfg(feature = "verifier-llm")]
mod llm;
#[cfg(feature = "verifier-llm")]
pub use llm::LlmVerifier;

#[cfg(feature = "verifier-ensemble")]
mod ensemble;
#[cfg(feature = "verifier-ensemble")]
pub use ensemble::Ensemble;

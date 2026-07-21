//! `agent-embed` — concrete [`Embedder`] backends behind the seam in `agent-core`
//! (parity spec 15).
//!
//! [`LocalEmbedder`] is a dependency-free, **deterministic** embedder: the
//! feature-hashing trick over word tokens **and** character trigrams, L2
//! normalised into a fixed-dimensional vector. It ships no model and needs no
//! network, so the default build stays hermetic under Nix. Being lexical-ish
//! (token + morphological overlap) it is a real, swap-in default that makes vector
//! search work; true semantic models (`text-embedding-3-small`, a local BERT)
//! drop in behind the same seam as the `embed-openai` / `embed-grpc` follow-ups.
//! See `docs/components/embedder.md`.

#[cfg(feature = "embed-local")]
mod local;
#[cfg(feature = "embed-local")]
pub use local::LocalEmbedder;

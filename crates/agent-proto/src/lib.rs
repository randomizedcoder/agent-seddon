//! `agent-proto` — the protobuf/gRPC wire contracts for the agent-seddon seams.
//!
//! This crate is the language-agnostic mirror of the `agent-core` "message
//! currency": every shared type ([`agent_core::Message`], [`agent_core::ToolCall`],
//! …) has a generated protobuf twin in [`pb`], and every seam trait
//! ([`agent_core::LlmProvider`], [`agent_core::MemoryStore`], …) has a gRPC service.
//! It exists so components can run as separate processes/containers with an
//! explicit, versioned contract between them, and so OpenTelemetry spans can follow
//! a request across those boundaries (see [`trace`]).
//!
//! `agent-core` stays the canonical in-process currency and never depends on this
//! crate — [`convert`] provides the `From`/`TryFrom` bridges in one direction only
//! (proto depends on core), preserving the acyclic seam graph in
//! `docs/architecture.md`.
//!
//! ## Scope
//!
//! This increment ships the schemas, the generated client+server stubs, the
//! conversions, and the trace-propagation helpers. The per-seam gRPC *transport
//! impls* (a `Grpc<Seam>` client that implements the `agent-core` trait, and a
//! server that wraps a local impl) and the `agent --serve-<seam>` binaries are
//! designed in `docs/grpc.md` and land in a follow-up.

/// The generated protobuf types and gRPC client/server stubs (package `agent.v1`).
pub mod pb {
    tonic::include_proto!("agent.v1");
}

pub mod convert;
pub mod trace;

pub use convert::{status_from_error, ConvertError};

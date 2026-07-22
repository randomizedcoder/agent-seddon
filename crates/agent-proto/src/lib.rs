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

/// The serialized `FileDescriptorSet` for every `agent.v1` proto, emitted by
/// `build.rs`. Feed it to `tonic_reflection` so a `--serve-<seam>` process can be
/// introspected by `grpcurl` (list/describe/call with JSON) without the `.proto`
/// files on hand. See `docs/components/grpc-introspection.md`.
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("agent_descriptor");

pub mod convert;
pub mod trace;

pub use convert::{status_from_error, ConvertError};

#[cfg(test)]
mod descriptor_tests {
    use super::FILE_DESCRIPTOR_SET;
    use prost::Message;
    use prost_types::FileDescriptorSet;

    // The reflection descriptor must decode and carry every seam service, so
    // `grpcurl` can introspect any `--serve-<seam>` process.
    #[test]
    fn descriptor_set_lists_every_seam_service() {
        let set =
            FileDescriptorSet::decode(FILE_DESCRIPTOR_SET).expect("emitted descriptor set decodes");
        let services: Vec<&str> = set
            .file
            .iter()
            .flat_map(|f| f.service.iter())
            .filter_map(|s| s.name.as_deref())
            .collect();
        for expected in [
            "SearchService",
            "ToolService",
            "Provider",
            "Memory",
            "ContextService",
            "Policy",
            "RepoService",
            "SessionService",
            "ScannerService",
            "ReferenceService",
            "SchedulerService",
            "TokenizerService",
            "EmbedService",
            "WebService",
            "WebSearchService",
            "SandboxService",
            "PtyService",
            "ForgeService",
            "TaskService",
        ] {
            assert!(
                services.contains(&expected),
                "reflection descriptor missing `{expected}`; has {services:?}"
            );
        }
    }
}

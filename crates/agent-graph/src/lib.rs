//! The cognition-graph **document layer** (design:
//! `docs/design/cognition-graph/04-graph-config.md`): the declarative node
//! graph that re-expresses the consensus gate, background distillation, and
//! instant compaction as user-configurable wiring.
//!
//! This crate owns everything about the document *as data* — the node-type
//! schema registry ([`schema`]), load validation with typed per-node issues
//! ([`validate`]), the human-readable textproto encoding ([`textproto`]), and
//! the file-backed [`agent_core::GraphStore`] ([`FileGraphs`]). The executor
//! that *runs* a validated document lives in `agent-runtime` (the three anchor
//! slots in the run loop); the gRPC `GraphService` lives in `agent-grpc`.
//!
//! Security posture: a document is **untrusted input** — it may arrive over
//! gRPC from a portal, from a model-suggested edit, or from a hand-edited
//! file. Everything fails closed: size caps before parse, closed edge-kind and
//! issue-code discriminators, unknown node types/versions rejected, params
//! validated against a closed key set with resource *names* (`safe_segment`)
//! only — never paths, endpoints, or secrets.

mod file;
pub mod schema;
pub mod testdata;
pub mod textproto;
pub mod validate;

pub use file::FileGraphs;
pub use schema::NodeTypeRegistry;
pub use validate::validate;

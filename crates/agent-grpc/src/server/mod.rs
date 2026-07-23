//! gRPC **servers** — one adapter per seam that wraps a locally-built
//! `Arc<dyn Trait>` and serves the generated tonic service.
//!
//! Each handler converts proto → core on the way in and core → proto out (via
//! `agent-proto`), maps `agent_core::Error` to a `tonic::Status`, and makes its
//! span a child of the caller's W3C trace context (extracted from request
//! metadata) so a trace spans the hop.
//!
//! The `*_router` helpers build a ready-to-serve `Router`, keeping tonic out of
//! the CLI; feed one to [`crate::transport::Bound::serve`].
//!
//! One module per seam. The shared pieces below — the span builder, the
//! missing-field status, and reflection — are written once and are what keep a
//! new seam's server adapter to roughly forty lines.

use tonic::Status;

/// Re-exported so callers (the CLI) can name a built router without taking a
/// direct `tonic` dependency — the point of the `*_router` helpers.
pub use tonic::transport::server::Router;
use tracing_opentelemetry::OpenTelemetrySpanExt;

mod context;
mod embed;
mod exec;
mod forge;
mod health;
mod llm_pool;
mod lsp;
mod memory;
mod policy;
mod provider;
mod reference;
mod repo;
mod review;
mod scanner;
mod scheduler;
mod search;
mod session;
mod tokenizer;
mod tools;
mod web;

pub use context::*;
pub use embed::*;
pub use exec::*;
pub use forge::*;
pub use health::*;
pub use llm_pool::*;
pub use lsp::*;
pub use memory::*;
pub use policy::*;
pub use provider::*;
pub use reference::*;
pub use repo::*;
pub use review::*;
pub use scanner::*;
pub use scheduler::*;
pub use search::*;
pub use session::*;
pub use tokenizer::*;
pub use tools::*;
pub use web::*;

/// Build a per-call span parented on the caller's extracted trace context.
pub(crate) fn span(rpc: &'static str, meta: &tonic::metadata::MetadataMap) -> tracing::Span {
    let s = tracing::info_span!("grpc.server", rpc);
    s.set_parent(agent_proto::trace::extract_context(meta));
    s
}

pub(crate) fn missing(field: &'static str) -> Status {
    Status::invalid_argument(format!("missing required field `{field}`"))
}

/// Add gRPC server reflection to a seam's `Router`, so a `--serve-<seam>` process
/// can be introspected (`grpcurl … list` / `describe`) and called with JSON without
/// the `.proto` files on hand. Registers both the `v1` and `v1alpha` reflection
/// services for maximum client compatibility (older `grpcurl` speaks only v1alpha).
///
/// **`grpc.health.v1` is registered alongside the agent's own descriptor set.**
/// The health service is served regardless, but a reflection-based client
/// (`grpcurl`, most debugging UIs) resolves a method by looking it up in
/// reflection first — so without its descriptor here, `grpcurl …
/// grpc.health.v1.Health/Check` fails with "server does not expose service" even
/// though the service is running and answering generated clients perfectly well.
pub fn with_reflection(
    router: Router,
) -> Result<Router, Box<dyn std::error::Error + Send + Sync + 'static>> {
    let v1 = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(agent_proto::FILE_DESCRIPTOR_SET)
        .register_encoded_file_descriptor_set(tonic_health::pb::FILE_DESCRIPTOR_SET)
        .build_v1()?;
    let v1alpha = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(agent_proto::FILE_DESCRIPTOR_SET)
        .register_encoded_file_descriptor_set(tonic_health::pb::FILE_DESCRIPTOR_SET)
        .build_v1alpha()?;
    Ok(router.add_service(v1).add_service(v1alpha))
}

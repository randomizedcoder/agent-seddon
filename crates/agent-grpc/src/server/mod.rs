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

/// The router type produced by [`base_router`] and threaded through the serve path:
/// a `Router` carrying the uniform [`AdmissionLayer`] (overload shedding). The
/// standalone `*_router` helpers keep the bare `Router` (tests don't need
/// admission); [`crate::transport::Endpoint::serve`] is generic so it accepts both.
pub type ServeRouter =
    Router<tower::layer::util::Stack<admission::AdmissionLayer, tower::layer::util::Identity>>;
use tracing_opentelemetry::OpenTelemetrySpanExt;

mod admission;
mod agent_session;
mod ast;
mod context;
mod digest;
mod dimension;
mod embed;
mod exec;
mod forge;
mod graph;
mod health;
mod llm_pool;
mod lsp;
mod memory;
mod metrics_proxy;
mod mode;
mod policy;
mod prompt;
mod provider;
mod provider_registry;
mod reference;
mod repo;
mod review;
mod scanner;
mod scheduler;
mod search;
mod session;
mod session_registry;
mod tokenizer;
mod tools;
mod web;

pub use admission::*;
pub use agent_session::*;
pub use ast::*;
pub use context::*;
pub use digest::*;
pub use dimension::*;
pub use embed::*;
pub use exec::*;
pub use forge::*;
pub use graph::*;
pub use health::*;
pub use llm_pool::*;
pub use lsp::*;
pub use memory::*;
pub use metrics_proxy::*;
pub use mode::*;
pub use policy::*;
pub use prompt::*;
pub use provider::*;
pub use provider_registry::*;
pub use reference::*;
pub use repo::*;
pub use review::*;
pub use scanner::*;
pub use scheduler::*;
pub use search::*;
pub use session::*;
pub use session_registry::*;
pub use tokenizer::*;
pub use tools::*;
pub use web::*;

/// Build a per-call span parented on the caller's extracted trace context, and
/// attribute it to the calling tenant when a well-formed `(user, session)` identity
/// is present in the metadata (docs/design/multi-session/01-identity.md).
///
/// The identity is attacker-controllable, so each segment is validated with
/// `agent_core::safe_segment` before being recorded; a malformed or absent segment is
/// simply not attributed here. Per-seam **fail-closed enforcement** for stateful RPCs
/// (reject when identity is absent) lands with the stateful-seam keying, once the
/// client side reliably sends identity — see docs/design/multi-session/04-tenancy.md.
pub(crate) fn span(rpc: &'static str, meta: &tonic::metadata::MetadataMap) -> tracing::Span {
    let s = tracing::info_span!(
        "grpc.server",
        rpc,
        session_id = tracing::field::Empty,
        user_id = tracing::field::Empty,
    );
    s.set_parent(agent_proto::trace::extract_context(meta));
    let (user, session) = agent_proto::identity::extract_identity(meta);
    if let Some(u) = user.as_deref().filter(|u| agent_core::safe_segment(u)) {
        s.record("user_id", u);
    }
    if let Some(sess) = session.as_deref().filter(|s| agent_core::safe_segment(s)) {
        s.record("session_id", sess);
    }
    s
}

pub(crate) fn missing(field: &'static str) -> Status {
    Status::invalid_argument(format!("missing required field `{field}`"))
}

/// The caller's `(user, session)` identity from request metadata, **only when both
/// segments are present and `safe_segment`-valid**. The values are attacker-controlled
/// (there is no auth layer — see docs/design/multi-session/07-security.md), so this
/// fails closed to `None` on anything malformed rather than sanitizing.
///
/// `None` means "run under the default `local` tenant" — a client that sends no
/// identity keeps today's single-tenant behaviour (back-compat). Per-seam *rejection*
/// of absent identity is a later increment; here absence is simply the default tenant.
pub(crate) fn identity_key(meta: &tonic::metadata::MetadataMap) -> Option<agent_core::SessionKey> {
    let (user, session) = agent_proto::identity::extract_identity(meta);
    let user = user.filter(|u| agent_core::safe_segment(u))?;
    let session = session.filter(|s| agent_core::safe_segment(s))?;
    agent_core::SessionKey::parse(&user, &session).ok()
}

/// Run `fut` with the caller's ambient identity scoped, so a stateful backend that
/// keys by `agent_core::current_identity()` — e.g. the per-user memory/dimension stores
/// (docs/design/multi-session/04-tenancy.md) — routes to the caller's tenant. With no
/// well-formed identity (`key = None`) the future runs unscoped, i.e. under the default
/// `local` tenant. Compute `key` from `request.metadata()` *before* `into_inner()`.
pub(crate) async fn run_scoped<F: std::future::Future>(
    key: Option<agent_core::SessionKey>,
    fut: F,
) -> F::Output {
    match key {
        Some(k) => agent_core::scope(k, fut).await,
        None => fut.await,
    }
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
    router: ServeRouter,
) -> Result<ServeRouter, Box<dyn std::error::Error + Send + Sync + 'static>> {
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

#[cfg(test)]
mod tests {
    use super::identity_key;
    use agent_proto::identity::{inject_identity, SESSION_ID_KEY, USER_ID_KEY};
    use tonic::metadata::{MetadataMap, MetadataValue};

    fn meta_with(user: &str, session: &str) -> MetadataMap {
        let mut m = MetadataMap::new();
        inject_identity(user, session, &mut m);
        m
    }

    #[test]
    fn positive_well_formed_identity_becomes_a_key() {
        let key = identity_key(&meta_with("alice", "sess-1")).expect("a key");
        assert_eq!(key.user.as_str(), "alice");
        assert_eq!(key.session.as_str(), "sess-1");
    }

    #[test]
    fn boundary_absent_identity_is_none() {
        assert!(identity_key(&MetadataMap::new()).is_none());
    }

    #[test]
    fn boundary_only_one_segment_present_is_none() {
        // A user with no session (or vice-versa) is not a complete key.
        let mut m = MetadataMap::new();
        m.insert(USER_ID_KEY, MetadataValue::from_static("alice"));
        assert!(identity_key(&m).is_none());
    }

    // A malformed segment is attacker-controlled and must fail closed to `None`
    // (→ the default tenant), never sanitized into a usable key.
    #[rstest::rstest]
    #[case::user_traversal("../../etc", "sess-1")]
    #[case::user_separator("a/b", "sess-1")]
    #[case::user_leading_dash("-rf", "sess-1")]
    #[case::session_traversal("alice", "..")]
    #[case::session_separator("alice", "a/b")]
    fn adversarial_malformed_segment_is_none(#[case] user: &str, #[case] session: &str) {
        // The values are valid ASCII (so they ride the wire) but fail `safe_segment`.
        let mut m = MetadataMap::new();
        m.insert(USER_ID_KEY, MetadataValue::try_from(user).unwrap());
        m.insert(SESSION_ID_KEY, MetadataValue::try_from(session).unwrap());
        assert!(
            identity_key(&m).is_none(),
            "malformed ({user:?},{session:?}) must not become a key"
        );
    }
}

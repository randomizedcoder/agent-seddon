//! `MetricsLayer` — one uniform per-RPC server metric for **every** seam
//! (config-plane observability, Phase 4). A `tower::Layer` stacked on the shared base
//! router *inside* [`super::auth::AuthLayer`], so it observes the **verified** identity
//! (the auth layer has already rewritten `x-agent-user-id` to the token's tenant) and a
//! single site counts all 40+ RPCs — no per-handler change.
//!
//! It records nothing itself: like [`super::admission::ShedObserver`] /
//! [`super::auth::AuthObserver`] it calls an injected [`RpcObserver`] the serve path
//! bridges to `agent_grpc_server_rpc_total{rpc,outcome,tenant}` +
//! `agent_grpc_server_rpc_seconds{rpc}`, so `agent-grpc` gains **no** `agent-metrics`
//! dependency. When no observer is attached the layer is a zero-overhead pass-through
//! (the many test/example callers of the base router are unaffected).
//!
//! **Outcome** is read from the response's `grpc-status` header: an error response is
//! trailers-only (the status rides the headers, visible without touching the body), so a
//! non-zero code maps to its canonical name; a successful unary/streaming call carries
//! `grpc-status` in the *trailers* (not the headers), which we do not buffer — it defaults
//! to `ok`. This is deliberate: it keeps the layer allocation-free (no body wrapping) and
//! the `outcome` label bounded to the canonical gRPC code set.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use tonic::body::BoxBody;
use tonic::codegen::http;
use tower::{Layer, Service};

/// Called once per served RPC with `(rpc_path, outcome, tenant, seconds)`, so the serve
/// path can bridge it to the RPC metric without this crate depending on `agent-metrics` —
/// the RPC twin of [`super::admission::ShedObserver`] / [`super::auth::AuthObserver`].
pub type RpcObserver = Arc<dyn Fn(&str, &str, &str, f64) + Send + Sync>;

/// Applies [`Metrics`] to a service. Cheap to clone (an `Option<Arc<…>>`).
#[derive(Clone)]
pub struct MetricsLayer {
    /// `None` ⇒ pass-through (no observer wired, e.g. tests / standalone routers).
    on_rpc: Option<RpcObserver>,
}

impl MetricsLayer {
    /// A pass-through layer that records nothing — the default for callers that do not
    /// wire an observer.
    pub fn disabled() -> Self {
        Self { on_rpc: None }
    }

    /// Attach the observer invoked once per served RPC. The serve path uses it to
    /// increment the RPC counter + observe its latency.
    pub fn with_observer(mut self, on_rpc: Option<RpcObserver>) -> Self {
        self.on_rpc = on_rpc;
        self
    }
}

impl<S> Layer<S> for MetricsLayer {
    type Service = Metrics<S>;
    fn layer(&self, inner: S) -> Metrics<S> {
        Metrics {
            inner,
            on_rpc: self.on_rpc.clone(),
        }
    }
}

/// The middleware service: times the inner call and reports `(rpc, outcome, tenant, secs)`
/// to the observer. A pass-through when no observer is attached.
#[derive(Clone)]
pub struct Metrics<S> {
    inner: S,
    on_rpc: Option<RpcObserver>,
}

impl<S> Service<http::Request<BoxBody>> for Metrics<S>
where
    S: Service<http::Request<BoxBody>, Response = http::Response<BoxBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = http::Response<BoxBody>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<BoxBody>) -> Self::Future {
        // tower contract: call the instance that was `poll_ready`d, leaving a fresh clone
        // behind for the next readiness poll.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        let Some(on_rpc) = self.on_rpc.clone() else {
            return Box::pin(async move { inner.call(req).await }); // pass-through
        };

        // Capture rpc + tenant *before* the inner call consumes the request. The tenant
        // is the VERIFIED value (this layer sits inside `AuthLayer`, which has already
        // rewritten the header); the recorder re-validates it with `safe_segment`. Empty
        // (mode=none / no identity) is a valid, bounded label value.
        let rpc = req.uri().path().to_string();
        let tenant = req
            .headers()
            .get(agent_proto::identity::USER_ID_KEY)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        Box::pin(async move {
            let started = Instant::now();
            let result = inner.call(req).await;
            let secs = started.elapsed().as_secs_f64();
            // A transport error (no HTTP response at all) is not a gRPC outcome; report
            // it as `error` so the sample is still counted.
            let outcome = match &result {
                Ok(resp) => outcome_from_headers(resp.headers()),
                Err(_) => "error",
            };
            on_rpc(&rpc, outcome, &tenant, secs);
            result
        })
    }
}

/// The RPC `outcome` from a response's `grpc-status` header — a **bounded** canonical
/// code name. Absent (status in the trailers, i.e. a successful call) or `0` ⇒ `ok`; any
/// other value ⇒ its canonical name (an unpar16able/unknown code ⇒ `error`), so the label
/// set is capped at the finite gRPC code space and never carries an attacker string.
fn outcome_from_headers(headers: &http::HeaderMap) -> &'static str {
    match headers.get("grpc-status").and_then(|v| v.to_str().ok()) {
        None => "ok",
        Some(s) => match s.trim().parse::<i32>() {
            Ok(0) => "ok",
            Ok(code) => code_name(code),
            Err(_) => "error",
        },
    }
}

/// Canonical lower-snake name for a gRPC status code (the finite code space, so the label
/// is bounded). Anything outside the canonical range maps to `error`.
fn code_name(code: i32) -> &'static str {
    match tonic::Code::from(code) {
        tonic::Code::Ok => "ok",
        tonic::Code::Cancelled => "cancelled",
        tonic::Code::Unknown => "unknown",
        tonic::Code::InvalidArgument => "invalid_argument",
        tonic::Code::DeadlineExceeded => "deadline_exceeded",
        tonic::Code::NotFound => "not_found",
        tonic::Code::AlreadyExists => "already_exists",
        tonic::Code::PermissionDenied => "permission_denied",
        tonic::Code::ResourceExhausted => "resource_exhausted",
        tonic::Code::FailedPrecondition => "failed_precondition",
        tonic::Code::Aborted => "aborted",
        tonic::Code::OutOfRange => "out_of_range",
        tonic::Code::Unimplemented => "unimplemented",
        tonic::Code::Internal => "internal",
        tonic::Code::Unavailable => "unavailable",
        tonic::Code::DataLoss => "data_loss",
        tonic::Code::Unauthenticated => "unauthenticated",
    }
}

#[cfg(test)]
mod tests;

//! gRPC **clients** — each implements an `agent_core` seam trait by calling a
//! remote server, so the loop can't tell a remote seam from a local one.
//!
//! Channels are built **lazily** (see [`crate::transport::Endpoint::connect_lazy`])
//! so the runtime's synchronous seam factories can construct a client without
//! awaiting. Every outbound request carries the current W3C trace context in its
//! metadata (via [`outbound`]) so the server can continue the trace.
//!
//! One module per seam. The shared pieces below — trace propagation, the retry
//! policy, and the gRPC status classifier — are written once; a new seam client
//! is roughly forty lines on top of them.
//!
//! **Failure semantics are per-seam and deliberate.** `Policy` fails *safe*
//! (deny), `Tool` fails *soft* (an error observation the model can read), and
//! `Search`/`Repo` fail *hard* (`Err`). Copying the wrong one silently changes
//! behaviour, so each client states its choice at the call site.

use tracing_opentelemetry::OpenTelemetrySpanExt;

mod context;
mod embed;
mod exec;
mod forge;
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

/// Wrap a message in a request carrying the caller's trace context.
///
/// We inject the *active `tracing` span's* OTel context, not
/// `opentelemetry::Context::current()` — the tracing-opentelemetry bridge keeps a
/// span's OTel context in the span's extensions, not the OTel thread-local, so the
/// latter is empty here and the server would see no parent. With the loop's seam
/// calls wrapped in spans, this makes the server's handler span a child of the
/// caller's span → one trace across the process boundary.
pub(crate) fn outbound<T>(msg: T) -> tonic::Request<T> {
    let mut req = tonic::Request::new(msg);
    let cx = tracing::Span::current().context();
    agent_proto::trace::inject_context(&cx, req.metadata_mut());
    req
}

/// Default retry policy for the gRPC seam clients: the canonical backoff (+ full
/// jitter) with 3 attempts. Threading a `[grpc] max_retries` value here is a
/// trivial follow-up; the wiring below is the substance.
pub(crate) fn grpc_retry_policy() -> agent_retry::RetryPolicy {
    agent_retry::RetryPolicy::new(3)
}

/// Retry decision for a gRPC `Status`, in the shape `agent_retry::retry` wants:
/// retry the transient "overloaded" codes (`UNAVAILABLE` / `RESOURCE_EXHAUSTED`),
/// honouring the server's `grpc-retry-pushback-ms` hint (including its `-1`
/// "do not retry" sentinel); fail fast on every other status.
pub(crate) fn grpc_retry_decision(status: &tonic::Status) -> Option<Option<std::time::Duration>> {
    use agent_retry::grpc::Pushback;
    let retryable = agent_retry::grpc::retryable_code(status.code() as i32);
    match status
        .metadata()
        .get("grpc-retry-pushback-ms")
        .and_then(|v| v.to_str().ok())
        .and_then(agent_retry::grpc::parse_pushback)
    {
        // The server explicitly forbids a retry — always honoured.
        Some(Pushback::DoNotRetry) => None,
        // A positive pushback only modulates the *delay* for a code we would already
        // retry. It must NOT let a hostile server force retries of a permanent error
        // (INVALID_ARGUMENT / PERMISSION_DENIED / …) by attaching a pushback header —
        // that is an amplification/abuse vector.
        Some(Pushback::RetryAfter(d)) if retryable => Some(Some(d)),
        Some(Pushback::RetryAfter(_)) => None,
        // No (usable) hint: fall back to code classification.
        None if retryable => Some(None),
        None => None,
    }
}

/// Run a **unary** gRPC call `op` through the canonical retry driver with the gRPC
/// classifier. `op` is re-invoked per attempt, so clone the request inside it.
/// (Streaming RPCs are intentionally not retried — a partial stream can't replay.)
pub(crate) async fn call_retry<T, Fut>(
    policy: &agent_retry::RetryPolicy,
    op: impl FnMut() -> Fut,
) -> std::result::Result<T, tonic::Status>
where
    Fut: std::future::Future<Output = std::result::Result<T, tonic::Status>>,
{
    agent_retry::retry(policy, grpc_retry_decision, op).await
}

#[cfg(test)]
mod retry_decision_tests {
    use super::grpc_retry_decision;
    use agent_retry::grpc::code;
    use rstest::rstest;
    use std::time::Duration;

    /// Build a `Status` with `code` and an optional `grpc-retry-pushback-ms` value.
    fn status(code: i32, pushback: Option<&str>) -> tonic::Status {
        let mut s = tonic::Status::new(tonic::Code::from_i32(code), "test");
        if let Some(v) = pushback {
            s.metadata_mut()
                .insert("grpc-retry-pushback-ms", v.parse().unwrap());
        }
        s
    }

    // The result shape: `None` = don't retry, `Some(None)` = retry with computed
    // backoff, `Some(Some(d))` = retry after the server's delay.
    #[rstest]
    #[case::positive_retryable_no_hint(code::UNAVAILABLE, None, Some(None))]
    #[case::positive_resource_exhausted(code::RESOURCE_EXHAUSTED, None, Some(None))]
    #[case::positive_pushback_delay(
        code::UNAVAILABLE,
        Some("100"),
        Some(Some(Duration::from_millis(100)))
    )]
    #[case::negative_nonretryable_no_hint(code::INVALID_ARGUMENT, None, None)]
    #[case::negative_permission_denied(code::PERMISSION_DENIED, None, None)]
    // Adversarial: `-1` must veto a retry even on a retryable code.
    #[case::adversarial_do_not_retry_overrides(code::UNAVAILABLE, Some("-1"), None)]
    // Adversarial: a positive pushback must NOT force a retry of a permanent error.
    #[case::adversarial_pushback_on_nonretryable(code::INVALID_ARGUMENT, Some("2500"), None)]
    // Corner: an unparseable/garbage hint falls back to code classification.
    #[case::corner_garbage_hint_falls_through(code::UNAVAILABLE, Some("later"), Some(None))]
    #[case::corner_other_negative_not_sentinel(code::UNAVAILABLE, Some("-2"), Some(None))]
    fn decision_cases(
        #[case] code: i32,
        #[case] pushback: Option<&str>,
        #[case] expected: Option<Option<Duration>>,
    ) {
        assert_eq!(grpc_retry_decision(&status(code, pushback)), expected);
    }
}

//! Uniform overload admission control (docs/design/loadtest).
//!
//! A `tower::Layer` applied once on the shared `base_router()` so **every** seam and
//! the `--serve-all` gateway shed load the same way. It bounds in-flight requests
//! with a semaphore; when the cap is reached a new request is **shed immediately**
//! with `RESOURCE_EXHAUSTED` (gRPC code 8) plus a `grpc-retry-pushback-ms` hint —
//! the standard "please slow down" signal the client's `agent-retry` already honors
//! and clamps. `max_in_flight = 0` disables it (the layer is a pass-through).

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tonic::body::BoxBody;
use tonic::codegen::http;
use tower::{Layer, Service};

/// Trailing/response-metadata key the client reads for a server backoff hint
/// (`agent-retry` `parse_pushback`). Mirrors the gRPC pushback convention.
const PUSHBACK_KEY: &str = "grpc-retry-pushback-ms";

/// Applies [`Admission`] to a service. Cheap to clone (shared semaphore + counter).
#[derive(Clone)]
pub struct AdmissionLayer {
    sem: Option<Arc<Semaphore>>,
    pushback_ms: i64,
    shed: Arc<AtomicU64>,
}

impl AdmissionLayer {
    /// Cap concurrent in-flight requests at `max_in_flight` (`0` ⇒ unbounded /
    /// pass-through); a shed response advertises `pushback_ms` as the retry hint.
    pub fn new(max_in_flight: usize, pushback_ms: i64) -> Self {
        Self {
            sem: (max_in_flight > 0).then(|| Arc::new(Semaphore::new(max_in_flight))),
            pushback_ms,
            shed: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The shared shed counter, so a caller (the load harness) can read how many
    /// requests were rejected under overload.
    pub fn shed_counter(&self) -> Arc<AtomicU64> {
        self.shed.clone()
    }
}

impl<S> Layer<S> for AdmissionLayer {
    type Service = Admission<S>;
    fn layer(&self, inner: S) -> Admission<S> {
        Admission {
            inner,
            sem: self.sem.clone(),
            pushback_ms: self.pushback_ms,
            shed: self.shed.clone(),
        }
    }
}

/// The middleware service. Holds a permit for the whole call (RAII), so the in-flight
/// count is bounded by real handler execution, not just request arrival.
#[derive(Clone)]
pub struct Admission<S> {
    inner: S,
    sem: Option<Arc<Semaphore>>,
    pushback_ms: i64,
    shed: Arc<AtomicU64>,
}

impl<S> Service<http::Request<BoxBody>> for Admission<S>
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
        let permit: Option<OwnedSemaphorePermit> = match &self.sem {
            None => None, // unbounded: pass through
            Some(sem) => match Arc::clone(sem).try_acquire_owned() {
                Ok(p) => Some(p),
                Err(_) => {
                    // At capacity → shed now, without touching the inner service.
                    self.shed.fetch_add(1, Ordering::Relaxed);
                    let resp = overloaded_response(self.pushback_ms);
                    return Box::pin(async move { Ok(resp) });
                }
            },
        };
        // tower contract: call the instance that was `poll_ready`d, leaving a fresh
        // clone behind for the next readiness poll.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        Box::pin(async move {
            let _permit = permit; // released when the call future completes
            inner.call(req).await
        })
    }
}

/// A `RESOURCE_EXHAUSTED` [`tonic::Status`] carrying the pushback hint in its
/// metadata, so the client backs off (and clamps) instead of failing fast. Shared
/// by the admission layer and by any handler that sheds internally (e.g. the pool).
pub fn overloaded_status(message: &str, pushback_ms: i64) -> tonic::Status {
    let mut status = tonic::Status::resource_exhausted(message.to_string());
    if let Ok(v) = tonic::metadata::MetadataValue::try_from(pushback_ms.to_string()) {
        status.metadata_mut().insert(PUSHBACK_KEY, v);
    }
    status
}

/// The shed as an HTTP response the tower layer returns directly.
fn overloaded_response(pushback_ms: i64) -> http::Response<BoxBody> {
    overloaded_status("server overloaded", pushback_ms).into_http()
}

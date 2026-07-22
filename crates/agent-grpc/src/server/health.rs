//! The standard `grpc.health.v1.Health` service, and the composable base router.
//!
//! A seam running as its own process is a network service like any other, and
//! the thing an orchestrator wants to ask it is "are you serving?". Answering
//! that on the **standard** service means a k8s `grpc` probe, `grpcurl
//! grpc.health.v1.Health/Check`, and every off-the-shelf gRPC load balancer all
//! work with no agent-specific knowledge.
//!
//! ## What SERVING claims here, precisely
//!
//! It claims the process is **up and that seam's adapter is wired** — the
//! transport is bound and the backing `Arc<dyn Trait>` was built and added to
//! the router. It does **not** claim the backing implementation is healthy: no
//! seam trait has a liveness probe, so a `--serve-search` whose index is corrupt
//! still reports SERVING.
//!
//! That is a deliberately narrow claim rather than a reassuring one. Health that
//! silently means less than a reader assumes is worse than no health endpoint,
//! because it gets wired into failover decisions. Widening it needs a readiness
//! method on the seam traits; [`HealthHandle`] is where a future readiness signal
//! would be flipped.
//!
//! ## Why status is set *after* the router is built
//!
//! [`base_router`] starts with nothing serving. Each seam is marked via
//! [`HealthHandle::set_serving`] only once its service has actually been added,
//! so a seam skipped because its feature is off does not advertise itself as
//! healthy. Marking a requested-but-absent seam SERVING would route traffic to a
//! service that returns `UNIMPLEMENTED`.

use tonic::transport::server::Router;
use tonic::transport::Server;

struct Inner {
    reporter: tonic_health::server::HealthReporter,
    /// Services currently marked SERVING.
    serving: Vec<String>,
}

/// A handle for updating serving status after the server is built.
pub struct HealthHandle {
    inner: tokio::sync::Mutex<Inner>,
}

impl HealthHandle {
    /// Mark one service as serving. Call once its service is on the router.
    pub async fn set_serving(&self, service: &str) {
        let mut g = self.inner.lock().await;
        g.reporter
            .set_service_status(service, tonic_health::ServingStatus::Serving)
            .await;
        if !g.serving.iter().any(|s| s == service) {
            g.serving.push(service.to_string());
        }
    }

    /// Mark one service as not serving — drains it from any load balancer
    /// honouring the standard health protocol.
    pub async fn set_not_serving(&self, service: &str) {
        let mut g = self.inner.lock().await;
        g.reporter
            .set_service_status(service, tonic_health::ServingStatus::NotServing)
            .await;
        g.serving.retain(|s| s != service);
    }

    /// Mark every hosted service, and the server as a whole, as not serving.
    /// The shape a graceful drain would take before shutdown.
    pub async fn set_all_not_serving(&self) {
        let mut g = self.inner.lock().await;
        let names: Vec<String> = g.serving.drain(..).collect();
        for s in names.iter().map(String::as_str).chain(std::iter::once("")) {
            g.reporter
                .set_service_status(s, tonic_health::ServingStatus::NotServing)
                .await;
        }
    }

    /// The service names currently reporting SERVING.
    pub async fn serving(&self) -> Vec<String> {
        self.inner.lock().await.serving.clone()
    }
}

/// Start a router hosting `grpc.health.v1.Health`.
///
/// The empty service name (`""`) — which the protocol defines as "the server as
/// a whole" — is marked SERVING immediately, because the process *is* up. Named
/// services are marked individually via [`HealthHandle::set_serving`] as they are
/// added. Both forms are supported because probes disagree about which to use:
/// k8s' `grpcService` field is optional and defaults to `""`, while a
/// service-aware balancer asks for the specific name.
///
/// The returned `Router` is the seed the seam services are added onto — which is
/// what lets one process host several seams (`--serve-all`) through the same code
/// path as one (`--serve-<seam>`).
pub async fn base_router() -> (Router, HealthHandle) {
    let (mut reporter, health_service) = tonic_health::server::health_reporter();
    reporter
        .set_service_status("", tonic_health::ServingStatus::Serving)
        .await;
    (
        Server::builder().add_service(health_service),
        HealthHandle {
            inner: tokio::sync::Mutex::new(Inner {
                reporter,
                serving: Vec::new(),
            }),
        },
    )
}

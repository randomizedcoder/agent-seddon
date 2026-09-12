//! Table-driven tests for the per-RPC [`MetricsLayer`] (config-plane observability,
//! Phase 4). Hermetic: a `service_fn` inner handler that stamps a chosen `grpc-status`
//! on its response, and a capturing [`RpcObserver`] that records what the layer reported.
//! Each case carries its `desc`/`expect` intent in its name + assert messages.

use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use rstest::rstest;
use tonic::body::BoxBody;
use tonic::codegen::http;
use tower::{Layer, Service};

use super::{MetricsLayer, RpcObserver};

/// One `(rpc, outcome, tenant)` the layer reported to the observer.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Rec {
    rpc: String,
    outcome: String,
    tenant: String,
}

/// A capturing observer + the shared log it appends to. Also asserts the timing invariant
/// (elapsed seconds are finite and non-negative) on every call.
fn recorder() -> (RpcObserver, Arc<Mutex<Vec<Rec>>>) {
    let log = Arc::new(Mutex::new(Vec::<Rec>::new()));
    let sink = log.clone();
    let obs: RpcObserver = Arc::new(move |rpc: &str, outcome: &str, tenant: &str, secs: f64| {
        assert!(
            secs.is_finite() && secs >= 0.0,
            "reported elapsed seconds must be finite and non-negative, got {secs}"
        );
        sink.lock().unwrap().push(Rec {
            rpc: rpc.to_string(),
            outcome: outcome.to_string(),
            tenant: tenant.to_string(),
        });
    });
    (obs, log)
}

fn request(path: &str, tenant: Option<&str>) -> http::Request<BoxBody> {
    let mut b = http::Request::builder().uri(path);
    if let Some(t) = tenant {
        b = b.header(agent_proto::identity::USER_ID_KEY, t);
    }
    b.body(tonic::body::empty_body()).unwrap()
}

/// Drive `req` through the layer into an inner handler that stamps `grpc_status` (as a
/// response *header* — the trailers-only shape tonic uses for an error) when `Some`,
/// mirroring a real error response; `None` mirrors a success (status rides the trailers,
/// which this layer does not buffer).
async fn drive(
    layer: &MetricsLayer,
    req: http::Request<BoxBody>,
    grpc_status: Option<&'static str>,
) -> http::Response<BoxBody> {
    let mut svc = layer.layer(tower::service_fn(
        move |_req: http::Request<BoxBody>| async move {
            let mut resp = http::Response::new(tonic::body::empty_body());
            if let Some(code) = grpc_status {
                resp.headers_mut()
                    .insert("grpc-status", http::HeaderValue::from_static(code));
            }
            Ok::<_, Infallible>(resp)
        },
    ));
    svc.call(req).await.unwrap()
}

#[rstest]
// desc: a success response (no grpc-status header) with a tenant header → the (rpc,ok,tenant) sample.
#[case::positive_ok_records_rpc_outcome_tenant(
    "/pkg.Svc/M",
    Some("acme"),
    None,
    "/pkg.Svc/M",
    "ok",
    "acme"
)]
// desc: an explicit grpc-status:0 is success → outcome ok.
#[case::positive_status_zero_is_ok(
    "/pkg.Svc/M",
    Some("acme"),
    Some("0"),
    "/pkg.Svc/M",
    "ok",
    "acme"
)]
// desc: a PermissionDenied (code 7) response maps to its canonical outcome name.
#[case::negative_permission_denied(
    "/pkg.Svc/M",
    Some("acme"),
    Some("7"),
    "/pkg.Svc/M",
    "permission_denied",
    "acme"
)]
// desc: Unavailable (code 14) maps to its canonical name — a distinct bounded outcome.
#[case::boundary_unavailable_outcome(
    "/svc/Ping",
    Some("acme"),
    Some("14"),
    "/svc/Ping",
    "unavailable",
    "acme"
)]
// desc: an unauthenticated request (no identity header) still records, with an empty tenant.
#[case::corner_unauthenticated_empty_tenant("/svc/Ping", None, None, "/svc/Ping", "ok", "")]
// desc: a non-numeric grpc-status (never emitted by tonic; hostile) is not parseable → error, never verbatim.
#[case::adversarial_nonnumeric_status_maps_error(
    "/svc/Ping",
    Some("acme"),
    Some("bogus"),
    "/svc/Ping",
    "error",
    "acme"
)]
// desc: an out-of-range status code (hostile) folds to `unknown` via the canonical map, staying bounded.
#[case::adversarial_out_of_range_status_bounded(
    "/svc/Ping",
    Some("acme"),
    Some("9999"),
    "/svc/Ping",
    "unknown",
    "acme"
)]
#[tokio::test]
async fn metrics_layer_records(
    #[case] path: &str,
    #[case] tenant: Option<&str>,
    #[case] grpc_status: Option<&'static str>,
    #[case] want_rpc: &str,
    #[case] want_outcome: &str,
    #[case] want_tenant: &str,
) {
    let (obs, log) = recorder();
    let layer = MetricsLayer::disabled().with_observer(Some(obs));
    drive(&layer, request(path, tenant), grpc_status).await;
    let recs = log.lock().unwrap();
    assert_eq!(recs.len(), 1, "exactly one RPC sample is reported");
    assert_eq!(
        recs[0],
        Rec {
            rpc: want_rpc.to_string(),
            outcome: want_outcome.to_string(),
            tenant: want_tenant.to_string(),
        },
    );
}

// desc: with NO observer attached the layer is a pure pass-through — the inner response is
// returned unchanged and nothing is recorded.
#[tokio::test]
async fn negative_no_observer_is_pass_through() {
    let layer = MetricsLayer::disabled();
    let resp = drive(&layer, request("/pkg.Svc/M", Some("acme")), None).await;
    assert!(
        resp.headers().get("grpc-status").is_none(),
        "the inner success response passes through untouched"
    );
}

// desc: the layer sits INSIDE AuthLayer, which strips any client `x-agent-user-id` and
// writes the verified tenant; a header-rewriting outer stand-in proves the observer
// records the value present at the layer's position — a client's pre-auth spoof cannot
// reach the metric.
#[tokio::test]
async fn adversarial_records_post_auth_tenant_not_client_spoof() {
    let (obs, log) = recorder();
    let metrics_svc = MetricsLayer::disabled()
        .with_observer(Some(obs))
        .layer(tower::service_fn(|_r: http::Request<BoxBody>| async move {
            Ok::<_, Infallible>(http::Response::new(tonic::body::empty_body()))
        }));
    // A tiny stand-in for the outer AuthLayer: drop whatever identity the client sent and
    // install the VERIFIED tenant before the metrics layer reads it.
    let mut outer = tower::service_fn(move |mut req: http::Request<BoxBody>| {
        let mut svc = metrics_svc.clone();
        async move {
            let name = http::HeaderName::from_static(agent_proto::identity::USER_ID_KEY);
            req.headers_mut().remove(&name);
            req.headers_mut()
                .insert(name, http::HeaderValue::from_static("verified"));
            svc.call(req).await
        }
    });
    outer
        .call(request("/pkg.Svc/M", Some("attacker")))
        .await
        .unwrap();
    let recs = log.lock().unwrap();
    assert_eq!(recs.len(), 1);
    assert_eq!(
        recs[0].tenant, "verified",
        "the recorded tenant is the verified value, never the client's pre-auth spoof"
    );
}

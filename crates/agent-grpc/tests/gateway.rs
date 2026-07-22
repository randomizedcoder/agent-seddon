//! The composable base router: health reporting and multi-seam hosting.
//!
//! These cover the two things that make gRPC a first-class transport rather than
//! a per-seam bolt-on: an orchestrator can *probe* a seam process with the
//! standard health service, and one process can host *several* seams so a
//! same-host deployment isn't one process per seam.

mod common;
use common::{spawn, Transport};

use agent_core::{ContextInput, ContextStrategy, Decision, Policy, ToolCall};
use agent_grpc::client::{GrpcContext, GrpcPolicy};
use agent_grpc::server::{base_router, ContextSvc, PolicySvc};
use agent_testkit::StaticContext;
use async_trait::async_trait;
use rstest::rstest;
use std::sync::Arc;
use tonic_health::pb::health_client::HealthClient;
use tonic_health::pb::HealthCheckRequest;
use tonic_health::ServingStatus;

struct AllowAll;

#[async_trait]
impl Policy for AllowAll {
    async fn authorize(&self, _call: &ToolCall) -> Decision {
        Decision::Allow
    }
}

/// Ask the health service about `service`, returning the raw status code.
async fn check(dial: &agent_grpc::Endpoint, service: &str) -> Result<i32, tonic::Status> {
    let channel = dial.connect_lazy().expect("connect");
    let mut client = HealthClient::new(channel);
    let resp = client
        .check(HealthCheckRequest {
            service: service.to_string(),
        })
        .await?;
    Ok(resp.into_inner().status)
}

/// The claim a k8s probe or a service-aware balancer actually reads.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_health_reports_serving_for_a_hosted_seam(#[case] transport: Transport) {
    let (router, health) = base_router().await;
    let router = router.add_service(PolicySvc::new(Arc::new(AllowAll)).into_server());
    health.set_serving("agent.v1.Policy").await;
    let (dial, _srv) = spawn(transport, router).await;

    // The named service…
    assert_eq!(
        check(&dial, "agent.v1.Policy").await.unwrap(),
        ServingStatus::Serving as i32
    );
    // …and the empty name, which the protocol defines as "the server as a whole".
    // Both are needed: k8s' `grpcService` defaults to `""`, a balancer asks by name.
    assert_eq!(
        check(&dial, "").await.unwrap(),
        ServingStatus::Serving as i32
    );
}

/// A seam that was **not** added must not advertise itself as healthy — marking
/// it SERVING would route traffic to a service that answers `UNIMPLEMENTED`.
/// This is the property that makes `--serve-all`'s skip path safe.
#[tokio::test(flavor = "multi_thread")]
async fn negative_unhosted_seam_is_not_reported_serving() {
    let (router, health) = base_router().await;
    let router = router.add_service(PolicySvc::new(Arc::new(AllowAll)).into_server());
    health.set_serving("agent.v1.Policy").await;
    let (dial, _srv) = spawn(Transport::Tcp, router).await;

    let err = check(&dial, "agent.v1.SearchService")
        .await
        .expect_err("an unhosted service must not report SERVING");
    assert_eq!(
        err.code(),
        tonic::Code::NotFound,
        "expected NOT_FOUND for an unregistered service, got {err:?}"
    );
    assert_eq!(health.serving().await, vec!["agent.v1.Policy".to_string()]);
}

/// Draining is the shape a graceful shutdown takes: stop advertising before the
/// listener goes away, so a balancer stops sending work first.
#[tokio::test(flavor = "multi_thread")]
async fn positive_drain_flips_to_not_serving() {
    let (router, health) = base_router().await;
    let router = router.add_service(PolicySvc::new(Arc::new(AllowAll)).into_server());
    health.set_serving("agent.v1.Policy").await;
    let (dial, _srv) = spawn(Transport::Tcp, router).await;
    assert_eq!(
        check(&dial, "agent.v1.Policy").await.unwrap(),
        ServingStatus::Serving as i32
    );

    health.set_all_not_serving().await;

    assert_eq!(
        check(&dial, "agent.v1.Policy").await.unwrap(),
        ServingStatus::NotServing as i32
    );
    assert_eq!(
        check(&dial, "").await.unwrap(),
        ServingStatus::NotServing as i32,
        "the server as a whole must drain too"
    );
    assert!(health.serving().await.is_empty());
}

/// Health must be discoverable **through reflection**, not merely served.
///
/// This is a regression test for a real gap: the health service was running and
/// answering generated clients, but `with_reflection` registered only the agent's
/// own descriptor set — so `grpcurl … grpc.health.v1.Health/Check` failed with
/// "server does not expose service". Reflection-based clients (grpcurl, most
/// debugging UIs) resolve a method via reflection before calling it, so a service
/// missing from the descriptor set is invisible to every one of them.
///
/// The other tests here all use generated clients, which bypass reflection
/// entirely and therefore could not catch this.
#[tokio::test(flavor = "multi_thread")]
async fn positive_health_is_discoverable_via_reflection() {
    use tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient;
    use tonic_reflection::pb::v1::server_reflection_request::MessageRequest;
    use tonic_reflection::pb::v1::server_reflection_response::MessageResponse;
    use tonic_reflection::pb::v1::ServerReflectionRequest;

    let (router, health) = base_router().await;
    let router = router.add_service(PolicySvc::new(Arc::new(AllowAll)).into_server());
    health.set_serving("agent.v1.Policy").await;
    let router = agent_grpc::server::with_reflection(router).expect("reflection");
    let (dial, _srv) = spawn(Transport::Tcp, router).await;

    let mut client = ServerReflectionClient::new(dial.connect_lazy().expect("connect"));
    let req = ServerReflectionRequest {
        host: String::new(),
        message_request: Some(MessageRequest::ListServices(String::new())),
    };
    let mut stream = client
        .server_reflection_info(tokio_stream::iter(vec![req]))
        .await
        .expect("reflection call")
        .into_inner();
    let resp = stream
        .message()
        .await
        .expect("reflection response")
        .expect("a response");
    let services: Vec<String> = match resp.message_response {
        Some(MessageResponse::ListServicesResponse(r)) => {
            r.service.into_iter().map(|s| s.name).collect()
        }
        other => panic!("unexpected reflection response: {other:?}"),
    };

    assert!(
        services.iter().any(|s| s == "grpc.health.v1.Health"),
        "health must be listed by reflection or grpcurl cannot call it; got {services:?}"
    );
    assert!(
        services.iter().any(|s| s == "agent.v1.Policy"),
        "the seam's own service must still be listed; got {services:?}"
    );
}

/// The `--serve-all` property: two seams on **one** router, both callable, and
/// health honest about both. Without this a 24-seam deployment is 24 processes.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_one_router_hosts_several_seams(#[case] transport: Transport) {
    let (router, health) = base_router().await;
    let router = router
        .add_service(PolicySvc::new(Arc::new(AllowAll)).into_server())
        .add_service(ContextSvc::new(Arc::new(StaticContext)).into_server());
    health.set_serving("agent.v1.Policy").await;
    health.set_serving("agent.v1.ContextService").await;
    let (dial, _srv) = spawn(transport, router).await;

    // Both seams answer on the same endpoint.
    let policy = GrpcPolicy::connect(&dial).unwrap();
    let call = ToolCall {
        id: "1".into(),
        name: "bash".into(),
        arguments: serde_json::json!({"cmd": "ls"}),
    };
    assert_eq!(policy.authorize(&call).await, Decision::Allow);

    let context = GrpcContext::connect(&dial).unwrap();
    let assembled = context
        .assemble(ContextInput {
            system_prompt: "you are a test".into(),
            prepend: vec![],
            recalled: vec![],
            goal: "do the thing".into(),
            append: vec![],
        })
        .await
        .expect("context assembles over the shared router");
    // StaticContext yields [system, user] — proof the *context* seam answered,
    // not just that something was listening.
    assert_eq!(
        assembled.len(),
        2,
        "the context seam must answer on the same endpoint as policy"
    );

    let mut serving = health.serving().await;
    serving.sort();
    assert_eq!(serving, vec!["agent.v1.ContextService", "agent.v1.Policy"]);
}

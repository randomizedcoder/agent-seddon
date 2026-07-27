//! Overload admission (docs/design/loadtest): the shared layer on `base_router`
//! sheds past the in-flight cap with `RESOURCE_EXHAUSTED` + a `grpc-retry-pushback-ms`
//! hint — the "please slow down" signal the client's `agent-retry` honors. Uses a
//! *raw* pb client (no retry) so the shed status is observed directly.

mod common;
use common::{spawn, Transport};

use std::sync::Arc;
use std::time::Duration;

use agent_core::{
    CompletionRequest, CompletionResponse, LlmProvider, Message, ModelCapabilities, Result, Usage,
};
use agent_proto::pb;
use async_trait::async_trait;

/// A provider slow enough that concurrent calls overlap in-flight, so a flood past
/// the cap is shed while a permit-holder is still running.
struct SlowProvider {
    delay: Duration,
}

#[async_trait]
impl LlmProvider for SlowProvider {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_tools: false,
            context_window: 1000,
            supports_response_format: false,
            supports_vision: false,
        }
    }
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
        tokio::time::sleep(self.delay).await;
        Ok(CompletionResponse {
            message: Message::assistant("ok"),
            finish_reason: "stop".into(),
            usage: Some(Usage::default()),
        })
    }
}

fn req() -> pb::CompletionRequest {
    pb::CompletionRequest::from(CompletionRequest {
        messages: vec![Message::user("hi")],
        tools: vec![],
        max_tokens: 8,
        temperature: 0.0,
        response_format: None,
    })
}

/// Serve a `ProviderService` behind `base_router(cap)` (the admission layer).
async fn serve_with_cap(cap: usize) -> (agent_grpc::Endpoint, common::TestServer) {
    let (router, _health) = agent_grpc::server::base_router(cap).await;
    let router = router.add_service(
        agent_grpc::server::ProviderService::new(Arc::new(SlowProvider {
            delay: Duration::from_millis(300),
        }))
        .into_server(),
    );
    spawn(Transport::Tcp, router).await
}

async fn flood(
    dial: &agent_grpc::Endpoint,
    n: usize,
) -> Vec<std::result::Result<(), tonic::Status>> {
    let ch = dial.connect_lazy().expect("channel");
    let mut tasks = Vec::new();
    for _ in 0..n {
        let ch = ch.clone();
        tasks.push(tokio::spawn(async move {
            let mut c = pb::provider_client::ProviderClient::new(ch);
            c.complete(req()).await.map(|_| ())
        }));
    }
    let mut out = Vec::new();
    for t in tasks {
        out.push(t.await.expect("join"));
    }
    out
}

// --- adversarial_: a flood past the cap sheds RESOURCE_EXHAUSTED + pushback ---
#[tokio::test]
async fn adversarial_flood_past_cap_sheds_resource_exhausted_with_pushback() {
    let (dial, _srv) = serve_with_cap(2).await;
    let results = flood(&dial, 8).await;

    let mut ok = 0;
    let mut shed = 0;
    let mut pushback_seen = false;
    for r in results {
        match r {
            Ok(()) => ok += 1,
            Err(status) => {
                assert_eq!(
                    status.code(),
                    tonic::Code::ResourceExhausted,
                    "overload must be RESOURCE_EXHAUSTED, got {status:?}"
                );
                if status.metadata().get("grpc-retry-pushback-ms").is_some() {
                    pushback_seen = true;
                }
                shed += 1;
            }
        }
    }
    assert!(
        shed >= 1,
        "some requests must be shed past the cap (ok={ok} shed={shed})"
    );
    assert!(ok >= 1, "some requests must succeed (ok={ok} shed={shed})");
    assert!(
        pushback_seen,
        "a shed must carry a grpc-retry-pushback-ms hint"
    );
}

// --- positive_: an unbounded cap (0) never sheds ------------------------------
#[tokio::test]
async fn positive_unbounded_cap_never_sheds() {
    let (dial, _srv) = serve_with_cap(0).await;
    for r in flood(&dial, 8).await {
        assert!(r.is_ok(), "unbounded cap (0) must not shed: {r:?}");
    }
}

// --- positive_: the shed observer fires exactly once per shed -----------------
// The serve path wires this observer to `Metrics::on_grpc_overload_shed`, so this
// is what keeps `agent_grpc_overload_shed_total` faithful to the wire behaviour.
#[tokio::test]
async fn positive_shed_observer_fires_once_per_shed() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let count = Arc::new(AtomicUsize::new(0));
    let obs: agent_grpc::server::ShedObserver = {
        let count = count.clone();
        Arc::new(move || {
            count.fetch_add(1, Ordering::Relaxed);
        })
    };
    let (router, _health) = agent_grpc::server::base_router_with_observer(2, Some(obs)).await;
    let router = router.add_service(
        agent_grpc::server::ProviderService::new(Arc::new(SlowProvider {
            delay: Duration::from_millis(300),
        }))
        .into_server(),
    );
    let (dial, _srv) = spawn(Transport::Tcp, router).await;

    let shed = flood(&dial, 8).await.iter().filter(|r| r.is_err()).count();
    assert!(shed >= 1, "some requests must be shed past the cap");
    assert_eq!(
        count.load(Ordering::Relaxed),
        shed,
        "the observer must fire exactly once per shed"
    );
}

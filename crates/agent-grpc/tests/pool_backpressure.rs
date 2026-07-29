//! Pool backpressure on the wire (docs/design/loadtest): when the pool sheds a
//! fan-out because its members are saturated, `LlmPoolService.Complete` returns
//! `RESOURCE_EXHAUSTED` + a pushback hint (not a silent empty `OK`), so a remote
//! client backs off. A genuinely empty/dead pool still returns an empty batch.

mod common;
use common::{spawn, Transport};

use std::sync::Arc;

use agent_core::{
    CompletionRequest, CompletionResponse, HealthReport, LlmPool, Message, PoolMemberHealth,
    PoolMemberResult, PoolMemberState, PoolTier, Result, Usage,
};
use agent_proto::pb;

fn member(alive: bool, saturated: bool) -> PoolMemberHealth {
    PoolMemberHealth {
        name: "m".into(),
        tier: PoolTier::Medium,
        alive,
        consecutive_failures: 0,
        last_probe_ms: 1,
        in_flight: if saturated { 2 } else { 0 },
        weight: 1.0,
        max_concurrency: if saturated { 2 } else { 0 },
        saturated,
        state: PoolMemberState::Healthy,
        latency_ms_ewma: 0,
    }
}

/// A pool whose fan-out returns nothing because its (alive) member is saturated.
struct SaturatedPool;
#[tonic::async_trait]
impl LlmPool for SaturatedPool {
    fn name(&self) -> &str {
        "saturated"
    }
    async fn health(&self) -> HealthReport {
        HealthReport {
            members: vec![member(true, true)],
        }
    }
    async fn complete_all(
        &self,
        _r: CompletionRequest,
        _t: PoolTier,
        _f: usize,
    ) -> Vec<PoolMemberResult> {
        Vec::new() // shed: all eligible at capacity
    }
    async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
        Ok(final_resp())
    }
}

/// A pool with no members — empty for a reason other than overload.
struct EmptyPool;
#[tonic::async_trait]
impl LlmPool for EmptyPool {
    fn name(&self) -> &str {
        "empty"
    }
    async fn health(&self) -> HealthReport {
        HealthReport { members: vec![] }
    }
    async fn complete_all(
        &self,
        _r: CompletionRequest,
        _t: PoolTier,
        _f: usize,
    ) -> Vec<PoolMemberResult> {
        Vec::new()
    }
    async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
        Ok(final_resp())
    }
}

fn final_resp() -> CompletionResponse {
    CompletionResponse {
        message: Message::assistant("ok"),
        finish_reason: "stop".into(),
        usage: Some(Usage::default()),
    }
}

fn req() -> pb::PoolCompleteRequest {
    pb::PoolCompleteRequest {
        req: Some(pb::CompletionRequest::from(CompletionRequest {
            messages: vec![Message::user("hi")],
            tools: vec![],
            max_tokens: 8,
            temperature: 0.0,
            response_format: None,
        })),
        tier: pb::PoolTier::from(PoolTier::Light) as i32,
        fanout: 3,
    }
}

async fn call(
    pool: Arc<dyn LlmPool>,
) -> std::result::Result<pb::PoolCompleteResponse, tonic::Status> {
    let (dial, _srv) = spawn(Transport::Tcp, agent_grpc::server::llm_pool_router(pool)).await;
    let ch = dial.connect_lazy().unwrap();
    let mut client = pb::llm_pool_service_client::LlmPoolServiceClient::new(ch);
    client
        .complete(req())
        .await
        .map(tonic::Response::into_inner)
}

// --- positive_: a saturated pool sheds RESOURCE_EXHAUSTED + pushback ----------
#[tokio::test]
async fn positive_saturated_pool_sheds_resource_exhausted() {
    let err = call(Arc::new(SaturatedPool)).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::ResourceExhausted, "got {err:?}");
    assert!(
        err.metadata().get("grpc-retry-pushback-ms").is_some(),
        "shed must carry a pushback hint"
    );
}

// --- negative_: an empty/dead pool is NOT overload — an empty OK batch --------
#[tokio::test]
async fn negative_empty_pool_is_not_overload() {
    let resp = call(Arc::new(EmptyPool))
        .await
        .expect("empty pool → OK, not an error");
    assert!(resp.results.is_empty());
}

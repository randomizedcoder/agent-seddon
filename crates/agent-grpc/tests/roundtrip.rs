//! Every seam, round-tripped over gRPC on **both** TCP and a unix domain socket.
//!
//! Each test binds a real server (an ephemeral `127.0.0.1:0` port or a temp-dir
//! socket), connects the matching client, invokes the seam, and asserts. The
//! transport is a table-driven `#[case]` so the exact same assertions run over TCP
//! and UDS.

use std::sync::Arc;

use agent_core::{
    ChangeKind, CompletionRequest, ContextInput, ContextStrategy, Decision, FileDiff, IndexState,
    LlmProvider, MemoryEvent, MemoryStore, Message, ModelCapabilities, Policy, RecallQuery,
    RepoBackend, Revision, SearchBackend, SearchMode, SearchQuery, TokenBudget, ToolCall,
    ToolContext, WorkingSet, WorktreeSpec,
};
use agent_grpc::client::{
    grpc_tools, GrpcContext, GrpcMemory, GrpcPolicy, GrpcProvider, GrpcRepo, GrpcSearch,
};
use agent_grpc::server::{
    context_router, memory_router, policy_router, provider_router, repo_router, search_router,
    tools_router,
};
mod common;
use common::{spawn, Transport};

use agent_testkit::{
    final_turn, tempdir, EchoTool, FixtureRepo, FixtureSearch, RecordingMemory, ScriptedProvider,
    StaticContext,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use rstest::rstest;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};

fn caps() -> ModelCapabilities {
    ModelCapabilities {
        supports_tools: true,
        context_window: 1000,
        supports_response_format: false,
        supports_vision: false,
    }
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn provider_complete(#[case] transport: Transport) {
    let provider = Arc::new(ScriptedProvider::new(vec![final_turn(
        "hello from gateway",
    )]));
    let (dial, _srv) = spawn(transport, provider_router(provider)).await;
    let client = GrpcProvider::connect(&dial, caps()).unwrap();

    let req = CompletionRequest {
        messages: vec![Message::user("hi")],
        tools: vec![],
        max_tokens: 16,
        temperature: 0.0,
        response_format: None,
    };
    let resp = client.complete(req).await.unwrap();
    assert_eq!(resp.message.content_text(), "hello from gateway");
    assert!(client.capabilities().supports_tools);
}

/// The 67-byte minimal 1x1 PNG (deterministic fixture, no assets).
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// A message's content blocks must survive the wire byte-for-byte — a media
/// block through the `blocks` field, and plain text through the legacy `content`
/// field, over both transports (parity spec 26).
#[rstest]
#[case::positive_image_block_survives_grpc(Transport::Tcp, true)]
#[case::positive_image_block_survives_uds(Transport::Uds, true)]
#[case::positive_text_only_uses_legacy_field(Transport::Tcp, false)]
#[tokio::test]
async fn message_blocks_roundtrip(#[case] transport: Transport, #[case] with_image: bool) {
    let content = if with_image {
        vec![
            agent_core::ContentBlock::text("look at this:"),
            agent_core::ContentBlock::image("image/png", TINY_PNG),
        ]
    } else {
        vec![agent_core::ContentBlock::text("just text")]
    };
    let sent = Message::with_blocks(agent_core::Role::User, content.clone());

    // Round-trip through the generated proto types exactly as the transport does.
    let pb: agent_proto::pb::Message = sent.clone().into();
    if with_image {
        assert!(!pb.blocks.is_empty(), "media must ride in `blocks`");
    } else {
        assert!(
            pb.blocks.is_empty(),
            "plain text must stay in the legacy `content` field (wire compatibility)"
        );
    }
    // `content` always carries the text, so a pre-spec-26 peer still reads prose.
    assert_eq!(pb.content, sent.content_text());

    let back = Message::try_from(pb).expect("decodes");
    assert_eq!(back.content, content, "blocks must survive byte-for-byte");

    // And over a real server on this transport.
    let provider = Arc::new(ScriptedProvider::new(vec![final_turn("ack")]));
    let (dial, _srv) = spawn(transport, provider_router(provider)).await;
    let client = GrpcProvider::connect(&dial, caps()).unwrap();
    let resp = client
        .complete(CompletionRequest {
            messages: vec![sent],
            tools: vec![],
            max_tokens: 16,
            temperature: 0.0,
            response_format: None,
        })
        .await
        .unwrap();
    assert_eq!(resp.message.content_text(), "ack");
}

/// A peer built before spec 26 sends only `content`; it must decode into one
/// text block rather than empty content.
#[test]
fn positive_legacy_string_content_decodes_to_one_text_block() {
    let legacy = agent_proto::pb::Message {
        role: agent_proto::pb::Role::User as i32,
        content: "from an old peer".into(),
        tool_calls: vec![],
        tool_call_id: None,
        blocks: vec![], // pre-spec-26 peers never set this
    };
    let m = Message::try_from(legacy).expect("decodes");
    assert_eq!(m.content.len(), 1);
    assert_eq!(m.content_text(), "from an old peer");
}

// --- fault injection: the client retries a transient gRPC UNAVAILABLE ---------
//
// A provider service that returns `UNAVAILABLE` (an "overloaded" code) on its first
// `fail_first` calls, then delegates to `inner`. Proves the gRPC client's canonical
// retry (agent-retry) reacts correctly to the overload codes, end to end.
use agent_proto::pb;

struct FaultyProvider {
    inner: Arc<dyn LlmProvider>,
    calls: Arc<AtomicU32>,
    fail_first: u32,
    /// Status code returned on the failing calls.
    fail_code: tonic::Code,
    /// Optional `grpc-retry-pushback-ms` header attached to the failing status.
    pushback: Option<&'static str>,
}

impl FaultyProvider {
    fn new(inner: Arc<dyn LlmProvider>, calls: Arc<AtomicU32>, fail_first: u32) -> Self {
        Self {
            inner,
            calls,
            fail_first,
            fail_code: tonic::Code::Unavailable,
            pushback: None,
        }
    }
    fn with_failure(mut self, code: tonic::Code, pushback: Option<&'static str>) -> Self {
        self.fail_code = code;
        self.pushback = pushback;
        self
    }
}

#[tonic::async_trait]
impl pb::provider_server::Provider for FaultyProvider {
    async fn capabilities(
        &self,
        _request: tonic::Request<pb::CapabilitiesRequest>,
    ) -> Result<tonic::Response<pb::ModelCapabilities>, tonic::Status> {
        Ok(tonic::Response::new(self.inner.capabilities().into()))
    }

    async fn complete(
        &self,
        request: tonic::Request<pb::CompletionRequest>,
    ) -> Result<tonic::Response<pb::CompletionResponse>, tonic::Status> {
        if self.calls.fetch_add(1, Ordering::SeqCst) < self.fail_first {
            let mut s = tonic::Status::new(self.fail_code, "injected fault");
            if let Some(p) = self.pushback {
                s.metadata_mut()
                    .insert("grpc-retry-pushback-ms", p.parse().unwrap());
            }
            return Err(s);
        }
        let req = request
            .into_inner()
            .try_into()
            .map_err(|e: agent_proto::ConvertError| tonic::Status::internal(e.to_string()))?;
        let resp = self
            .inner
            .complete(req)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;
        Ok(tonic::Response::new(resp.into()))
    }

    type StreamStream = Pin<
        Box<dyn futures_util::Stream<Item = Result<pb::CompletionChunk, tonic::Status>> + Send>,
    >;

    #[allow(clippy::result_large_err)]
    async fn stream(
        &self,
        _request: tonic::Request<pb::CompletionRequest>,
    ) -> Result<tonic::Response<Self::StreamStream>, tonic::Status> {
        Err(tonic::Status::unimplemented("stream unused in this test"))
    }
}

async fn complete_via(faulty: FaultyProvider) -> (std::result::Result<String, String>, u32) {
    let calls = faulty.calls.clone();
    let router = tonic::transport::Server::builder()
        .add_service(pb::provider_server::ProviderServer::new(faulty));
    let (dial, _srv) = spawn(Transport::Tcp, router).await;
    let client = GrpcProvider::connect(&dial, caps()).unwrap();
    let out = client
        .complete(CompletionRequest {
            messages: vec![Message::user("hi")],
            tools: vec![],
            max_tokens: 16,
            temperature: 0.0,
            response_format: None,
        })
        .await
        .map(|r| r.message.content_text())
        .map_err(|e| e.to_string());
    (out, calls.load(Ordering::SeqCst))
}

fn faulty(fail_first: u32) -> FaultyProvider {
    FaultyProvider::new(
        Arc::new(ScriptedProvider::new(vec![final_turn("recovered")])),
        Arc::new(AtomicU32::new(0)),
        fail_first,
    )
}

/// Positive: a transient `UNAVAILABLE` is retried and the next call succeeds.
#[tokio::test(flavor = "multi_thread")]
async fn provider_retries_unavailable_then_succeeds() {
    let (out, calls) = complete_via(faulty(1)).await;
    assert_eq!(out.as_deref(), Ok("recovered"));
    assert_eq!(calls, 2, "one retry after the UNAVAILABLE");
}

/// Negative: a permanent `INVALID_ARGUMENT` is not retried — one call, then fail.
#[tokio::test(flavor = "multi_thread")]
async fn provider_does_not_retry_invalid_argument() {
    let f = faulty(u32::MAX).with_failure(tonic::Code::InvalidArgument, None);
    let (out, calls) = complete_via(f).await;
    assert!(out.is_err());
    assert_eq!(calls, 1, "a permanent code must not be retried");
}

/// Adversarial: a `-1` pushback header vetoes the retry even on a retryable code.
#[tokio::test(flavor = "multi_thread")]
async fn provider_honors_minus_one_pushback() {
    let f = faulty(u32::MAX).with_failure(tonic::Code::Unavailable, Some("-1"));
    let (out, calls) = complete_via(f).await;
    assert!(out.is_err());
    assert_eq!(calls, 1, "`-1` pushback must forbid the retry");
}

/// Adversarial: a positive pushback attached to a permanent code must NOT force a
/// retry (the abuse vector fixed in grpc_retry_decision).
#[tokio::test(flavor = "multi_thread")]
async fn provider_pushback_cannot_force_retry_of_permanent_error() {
    let f = faulty(u32::MAX).with_failure(tonic::Code::InvalidArgument, Some("50"));
    let (out, calls) = complete_via(f).await;
    assert!(out.is_err());
    assert_eq!(
        calls, 1,
        "pushback must not force a retry of a permanent code"
    );
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn provider_stream(#[case] transport: Transport) {
    let provider = Arc::new(ScriptedProvider::new(vec![final_turn("streamed text")]));
    let (dial, _srv) = spawn(transport, provider_router(provider)).await;
    let client = GrpcProvider::connect(&dial, caps()).unwrap();

    let req = CompletionRequest {
        messages: vec![Message::user("hi")],
        tools: vec![],
        max_tokens: 16,
        temperature: 0.0,
        response_format: None,
    };
    let mut stream = client.stream(req).await.unwrap();
    let mut text = String::new();
    while let Some(chunk) = stream.next().await {
        text.push_str(&chunk.unwrap().delta_text);
    }
    assert_eq!(text, "streamed text");
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn tools_describe_and_execute(#[case] transport: Transport) {
    let mut registry = agent_core::ToolRegistry::new();
    registry.register(Arc::new(EchoTool));
    let (dial, _srv) = spawn(transport, tools_router(registry, std::env::temp_dir())).await;

    let tools = grpc_tools(&dial).await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name(), "echo");
    // A parallel-safe tool (EchoTool inherits the default `true`) propagates as such.
    assert!(tools[0].parallel_safe());

    let ctx = ToolContext {
        cwd: std::env::temp_dir(),
    };
    let obs = tools[0]
        .execute(serde_json::json!({ "val": "pong" }), &ctx)
        .await
        .unwrap();
    assert!(!obs.is_error);
    assert_eq!(obs.content, "pong");
}

// A real built-in tool (`apply_patch`) dispatched over the generic ToolService
// `Execute` RPC — proves a *feature* tool works end-to-end over protobuf+gRPC on
// both transports, not just the in-testkit `EchoTool`.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn apply_patch_roundtrips(#[case] transport: Transport) {
    let dir = tempdir();
    std::fs::write(dir.join("f.txt"), "before\n").unwrap();

    let mut registry = agent_core::ToolRegistry::new();
    registry.register(Arc::new(agent_tools::ApplyPatchTool));
    let (dial, _srv) = spawn(transport, tools_router(registry, dir.clone())).await;

    let tools = grpc_tools(&dial).await.unwrap();
    let patch = tools.iter().find(|t| t.name() == "apply_patch").unwrap();

    let ctx = ToolContext { cwd: dir.clone() };
    let obs = patch
        .execute(
            serde_json::json!({
                "patch": "*** Begin Patch\n*** Update File: f.txt\n@@\n-before\n+after\n*** End Patch"
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert!(!obs.is_error, "{}", obs.content);
    assert!(obs.content.contains("M f.txt"), "summary: {}", obs.content);
    assert_eq!(
        std::fs::read_to_string(dir.join("f.txt")).unwrap(),
        "after\n"
    );
}

// The file tools over gRPC: write a file then read it back through the seam on
// both transports (a real cwd flows in the ToolContext, distinct from the
// server's default).
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn read_write_roundtrips(#[case] transport: Transport) {
    let dir = tempdir();
    let mut registry = agent_core::ToolRegistry::new();
    registry.register(Arc::new(agent_tools::WriteFileTool));
    registry.register(Arc::new(agent_tools::ReadFileTool));
    let (dial, _srv) = spawn(transport, tools_router(registry, dir.clone())).await;

    let tools = grpc_tools(&dial).await.unwrap();
    let write = tools.iter().find(|t| t.name() == "write_file").unwrap();
    let read = tools.iter().find(|t| t.name() == "read_file").unwrap();
    let ctx = ToolContext { cwd: dir.clone() };

    let w = write
        .execute(
            serde_json::json!({ "path": "f.txt", "content": "over the wire" }),
            &ctx,
        )
        .await
        .unwrap();
    assert!(!w.is_error, "{}", w.content);

    let r = read
        .execute(serde_json::json!({ "path": "f.txt" }), &ctx)
        .await
        .unwrap();
    assert!(!r.is_error, "{}", r.content);
    assert_eq!(r.content, "over the wire");
    assert_eq!(
        std::fs::read_to_string(dir.join("f.txt")).unwrap(),
        "over the wire"
    );
}

// `bash` over gRPC: run a command through the ToolService seam and get its output
// back on both transports.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn bash_roundtrips(#[case] transport: Transport) {
    let mut registry = agent_core::ToolRegistry::new();
    registry.register(Arc::new(agent_tools::BashTool::default()));
    let (dial, _srv) = spawn(transport, tools_router(registry, std::env::temp_dir())).await;

    let tools = grpc_tools(&dial).await.unwrap();
    let bash = tools.iter().find(|t| t.name() == "bash").unwrap();
    // The concurrency contract now survives the seam (`parallel_safe` is carried in
    // `DescribeAll`): a *remote* `bash` reports NOT parallel-safe, so the loop
    // serializes it exactly as it would a local `bash`.
    assert!(!bash.parallel_safe());

    let ctx = ToolContext {
        cwd: std::env::temp_dir(),
    };
    let obs = bash
        .execute(serde_json::json!({ "command": "echo over-the-wire" }), &ctx)
        .await
        .unwrap();
    assert!(!obs.is_error, "{}", obs.content);
    assert!(
        obs.content.contains("over-the-wire"),
        "output: {}",
        obs.content
    );
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn memory_append_and_recall(#[case] transport: Transport) {
    let mem = Arc::new(RecordingMemory::new());
    let (dial, _srv) = spawn(transport, memory_router(mem.clone())).await;
    let client = GrpcMemory::connect(&dial).unwrap();

    let event = MemoryEvent {
        kind: "assistant".into(),
        message: Message::assistant("remembered over the wire"),
        ts_ms: 42,
        session_id: "s1".into(),
        usage: None,
        iter: None,
        verification: None,
        review: None,
        dimensional: None,
    };
    client.append(event).await.unwrap();

    // RecordingMemory recalls nothing but records appends — assert both RPCs work.
    let recalled = client
        .recall(&RecallQuery {
            text: "anything".into(),
            limit: 5,
        })
        .await
        .unwrap();
    assert!(recalled.is_empty());

    let events = mem.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].message.content_text(), "remembered over the wire");
    assert_eq!(events[0].ts_ms, 42);
}

// A store that records the ambient tenant (`current_identity().user`) seen at each
// call — proves the served handler scopes the caller's wire identity into the store,
// so a per-user backend (docs/design/multi-session/04-tenancy.md) routes to the right
// tenant instead of the default (increment 04b).
#[derive(Default)]
struct TenantProbeMemory {
    seen: Arc<std::sync::Mutex<Vec<Option<String>>>>,
}

#[async_trait]
impl MemoryStore for TenantProbeMemory {
    async fn recall(&self, _q: &RecallQuery) -> agent_core::Result<Vec<agent_core::MemoryItem>> {
        self.seen
            .lock()
            .unwrap()
            .push(agent_core::current_identity().map(|k| k.user.as_str().to_string()));
        Ok(vec![])
    }
    async fn append(&self, _e: MemoryEvent) -> agent_core::Result<()> {
        Ok(())
    }
    async fn distill(&self) -> agent_core::Result<usize> {
        Ok(0)
    }
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn memory_recall_scopes_caller_tenant(#[case] transport: Transport) {
    let probe = Arc::new(TenantProbeMemory::default());
    let (dial, _srv) = spawn(transport, memory_router(probe.clone())).await;
    let client = GrpcMemory::connect(&dial).unwrap();
    let q = RecallQuery {
        text: "x".into(),
        limit: 1,
    };

    // A scoped caller's identity rides the wire (the client injects it) and the handler
    // re-scopes it, so the per-user store sees *this* tenant.
    agent_core::scope(
        agent_core::SessionKey::parse("alice", "s1").unwrap(),
        async {
            client.recall(&q).await.unwrap();
        },
    )
    .await;

    // No ambient identity → nothing injected → the store runs under the default tenant.
    client.recall(&q).await.unwrap();

    let seen = probe.seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![Some("alice".to_string()), None],
        "handler scopes a present identity, leaves an absent one as the default tenant"
    );
}

// ---------------------------------------------------------------------------
// SessionStore ownership (04c): a bare checkpoint id is not a cross-tenant read
// oracle over the wire — the served handler scopes the caller's identity and the
// store authorizes the read by session-reachability.
// ---------------------------------------------------------------------------

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn session_restore_denies_foreign_checkpoint_over_the_wire(#[case] transport: Transport) {
    use agent_core::{scope, SessionKey, SessionStore};
    use agent_grpc::client::GrpcSession;
    use agent_grpc::server::session_router;

    let store = Arc::new(agent_session::FileSessionStore::new(tempdir()));
    let (dial, _srv) = spawn(transport, session_router(store)).await;
    let client = GrpcSession::connect(&dial).unwrap();

    // Alice checkpoints under her session (the id is content-addressed / guessable).
    let ws = WorkingSet {
        messages: vec![Message::user("alice's secret working set")],
    };
    let id = client.checkpoint("alice-s1", &ws, "t1").await.unwrap();

    // Bob, scoped to his own identity (which the client injects into metadata), is
    // denied — and the error is indistinguishable from an absent id.
    let bob = SessionKey::parse("bob", "bob-s1").unwrap();
    let denied = scope(bob, client.restore(&id)).await;
    assert!(denied.is_err(), "bob must not restore alice's checkpoint");

    // Alice, whose session owns it, restores it.
    let alice = SessionKey::parse("alice", "alice-s1").unwrap();
    let restored = scope(alice, client.restore(&id)).await.unwrap();
    assert_eq!(restored.messages.len(), 1);
}

// ---------------------------------------------------------------------------
// SessionRegistry lifecycle (05b): Open server-mints an id; Close/Heartbeat name it;
// a capacity rejection maps to RESOURCE_EXHAUSTED across the wire.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct FakeRegistry {
    opened: Arc<std::sync::Mutex<Vec<String>>>,
    closed: Arc<std::sync::Mutex<Vec<(String, String)>>>,
    beats: Arc<std::sync::Mutex<Vec<(String, String)>>>,
}

#[async_trait]
impl agent_core::SessionRegistry for FakeRegistry {
    async fn open(&self, user: &str) -> agent_core::Result<agent_core::SessionId> {
        if user == "over" {
            return Err(agent_core::Error::Overloaded("per-user limit".into()));
        }
        self.opened.lock().unwrap().push(user.to_string());
        Ok(agent_core::SessionId::new(format!("minted-{user}")))
    }
    async fn close(&self, key: &agent_core::SessionKey) -> agent_core::Result<()> {
        self.closed
            .lock()
            .unwrap()
            .push((key.user.as_str().into(), key.session.as_str().into()));
        Ok(())
    }
    async fn heartbeat(&self, key: &agent_core::SessionKey) -> agent_core::Result<()> {
        self.beats
            .lock()
            .unwrap()
            .push((key.user.as_str().into(), key.session.as_str().into()));
        Ok(())
    }
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn session_registry_lifecycle_roundtrip(#[case] transport: Transport) {
    use agent_core::{SessionKey, SessionRegistry};
    use agent_grpc::client::GrpcSessionRegistry;
    use agent_grpc::server::session_registry_router;

    let reg = Arc::new(FakeRegistry::default());
    let (dial, _srv) = spawn(transport, session_registry_router(reg.clone())).await;
    let client = GrpcSessionRegistry::connect(&dial).unwrap();

    // Open returns the server-minted id (the client never chooses it).
    let id = client.open("alice").await.unwrap();
    assert_eq!(id.as_str(), "minted-alice");
    assert_eq!(
        reg.opened.lock().unwrap().as_slice(),
        &["alice".to_string()]
    );

    // Close + Heartbeat name the session by `(user, session_id)`.
    let key = SessionKey::parse("alice", "s1").unwrap();
    client.close(&key).await.unwrap();
    client.heartbeat(&key).await.unwrap();
    assert_eq!(
        reg.closed.lock().unwrap().as_slice(),
        &[("alice".to_string(), "s1".to_string())]
    );
    assert_eq!(reg.beats.lock().unwrap().len(), 1);

    // A capacity rejection surfaces as an error across the wire (RESOURCE_EXHAUSTED).
    let err = client.open("over").await.unwrap_err().to_string();
    assert!(
        err.contains("overloaded"),
        "capacity error not surfaced: {err}"
    );
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn context_assemble_and_compact(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, context_router(Arc::new(StaticContext))).await;
    let client = GrpcContext::connect(&dial).unwrap();

    let messages = client
        .assemble(ContextInput {
            system_prompt: "you are a test".into(),
            prepend: vec![],
            recalled: vec![],
            goal: "do the thing".into(),
            append: vec![],
        })
        .await
        .unwrap();
    // StaticContext yields [system, user].
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].content_text(), "do the thing");

    // StaticContext.compact is a no-op — the working set survives the round-trip.
    let mut working = WorkingSet {
        messages: vec![Message::user("keep me")],
    };
    client
        .compact(
            &mut working,
            &TokenBudget {
                max_context_tokens: 1000,
                reserve_output: 100,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(working.messages.len(), 1);
    assert_eq!(working.messages[0].content_text(), "keep me");
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

struct AllowAll;

#[async_trait]
impl Policy for AllowAll {
    async fn authorize(&self, _call: &ToolCall) -> Decision {
        Decision::Allow
    }
}

struct DenyWith(&'static str);

#[async_trait]
impl Policy for DenyWith {
    async fn authorize(&self, _call: &ToolCall) -> Decision {
        Decision::Deny(self.0.into())
    }
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn policy_authorize_allow(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, policy_router(Arc::new(AllowAll))).await;
    let client = GrpcPolicy::connect(&dial).unwrap();

    let call = ToolCall {
        id: "1".into(),
        name: "bash".into(),
        arguments: serde_json::json!({ "cmd": "ls" }),
    };
    assert_eq!(client.authorize(&call).await, Decision::Allow);
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn policy_authorize_deny(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, policy_router(Arc::new(DenyWith("nope")))).await;
    let client = GrpcPolicy::connect(&dial).unwrap();

    let call = ToolCall {
        id: "1".into(),
        name: "bash".into(),
        arguments: serde_json::json!({}),
    };
    assert_eq!(client.authorize(&call).await, Decision::Deny("nope".into()));
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

fn literal(text: &str) -> SearchQuery {
    SearchQuery {
        text: text.into(),
        mode: SearchMode::Literal,
        path_globs: vec![],
        lang: None,
        limit: 10,
        fuzzy_distance: None,
    }
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn search_status_and_query(#[case] transport: Transport) {
    let backend = Arc::new(FixtureSearch::new().with_hits(vec![FixtureSearch::hit(
        "src/main.rs",
        12,
        "fn main()",
    )]));
    let (dial, _srv) = spawn(transport, search_router(backend)).await;
    let client = GrpcSearch::connect(&dial).unwrap();

    // status → the fixture's fresh index
    let status = client.status().await.unwrap();
    assert_eq!(status.state, IndexState::Fresh);
    assert_eq!(status.indexed_files, 3);

    // query → the fixture's hit list, converted back across the wire
    let hits = client.query(&literal("main")).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path.to_string_lossy(), "src/main.rs");
    assert_eq!(hits[0].line, 12);
    assert_eq!(hits[0].snippet, "fn main()");
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn search_reindex_streams_progress(#[case] transport: Transport) {
    let backend = Arc::new(FixtureSearch::new());
    let (dial, _srv) = spawn(transport, search_router(backend)).await;
    let client = GrpcSearch::connect(&dial).unwrap();

    // Drive the server-streamed reindex; count progress increments to the terminal.
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen2 = seen.clone();
    let progress = move |p: agent_core::ReindexProgress| {
        seen2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if p.done {
            assert_eq!(p.files_done, p.files_total);
        }
    };
    let status = client.reindex(&progress).await.unwrap();
    assert_eq!(status.state, IndexState::Fresh);
    assert!(
        seen.load(std::sync::atomic::Ordering::SeqCst) >= 2,
        "expected at least two progress increments (incl. the terminal one)"
    );
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn search_list_files(#[case] transport: Transport) {
    // `list_files` previously fell through to the trait default (unsupported) on the
    // gRPC client; assert the paths now round-trip over the seam.
    let files = vec![
        std::path::PathBuf::from("src/lib.rs"),
        std::path::PathBuf::from("src/main.rs"),
    ];
    let backend = Arc::new(FixtureSearch::new().with_files(files.clone()));
    let (dial, _srv) = spawn(transport, search_router(backend)).await;
    let client = GrpcSearch::connect(&dial).unwrap();

    let got = client.list_files(&["**/*.rs".into()]).await.unwrap();
    assert_eq!(got, files);
}

// ---------------------------------------------------------------------------
// Repo (multi-branch git)
// ---------------------------------------------------------------------------

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn repo_reads_over_the_wire(#[case] transport: Transport) {
    let backend = Arc::new(
        FixtureRepo::new()
            .with_branch("main", "a".repeat(40))
            .with_blob("main", "a.txt", "hello over the wire")
            .with_diff(vec![FileDiff {
                change: ChangeKind::Added,
                old_path: None,
                new_path: Some("b.txt".into()),
                old_oid: None,
                new_oid: None,
                additions: 2,
                deletions: 0,
                patch: "+hi".into(),
            }]),
    );
    let (dial, _srv) = spawn(transport, repo_router(backend)).await;
    let client = GrpcRepo::connect(&dial).unwrap();

    // resolve → a full oid derived from the revision
    let oid = client.resolve(&Revision::from("main")).await.unwrap();
    assert_eq!(oid.as_str().len(), 40);

    // read_file → the canned blob, converted back across the wire
    let blob = client
        .read_file(&Revision::from("main"), std::path::Path::new("a.txt"))
        .await
        .unwrap();
    assert_eq!(blob.text, "hello over the wire");

    // diff → the canned FileDiff, change kind + patch preserved
    let diff = client
        .diff(&Revision::from("main"), &Revision::from("main"), &[])
        .await
        .unwrap();
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].change, ChangeKind::Added);
    assert_eq!(
        diff.files[0].new_path.as_deref(),
        Some(std::path::Path::new("b.txt"))
    );

    // branches → the fixture branch
    let branches = client.branches().await.unwrap();
    assert!(branches.iter().any(|(n, _)| n == "main"));
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn repo_worktree_lifecycle_over_the_wire(#[case] transport: Transport) {
    let backend = Arc::new(FixtureRepo::new().with_branch("main", "a".repeat(40)));
    let (dial, _srv) = spawn(transport, repo_router(backend)).await;
    let client = GrpcRepo::connect(&dial).unwrap();

    assert!(client.worktree_list().await.unwrap().is_empty());
    let handle = client
        .worktree_add(&WorktreeSpec {
            revision: Revision::from("main"),
            writable: false,
            id: Some("cmp".into()),
        })
        .await
        .unwrap();
    assert_eq!(handle.id, "cmp");
    assert_eq!(client.worktree_list().await.unwrap().len(), 1);
    // status reflects the live worktree over the wire.
    assert_eq!(client.status().await.unwrap().live_worktrees, 1);
    client.worktree_remove("cmp").await.unwrap();
    assert!(client.worktree_list().await.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Prompt (docs/design/portal) — the real FilePromptStore over the wire
// ---------------------------------------------------------------------------

// A put→get→list→delete cycle survives the hop, and a mode-lens default reads
// back as `builtin`. The same assertions run over TCP and UDS.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test]
async fn prompt_crud_roundtrips(#[case] transport: Transport) {
    use agent_core::{PromptContext, PromptEntry, PromptKind, PromptRef, PromptStore};
    let root = tempdir();
    let store = Arc::new(agent_prompt::FilePromptStore::new(
        root.join("context.d"),
        root.join("prompts"),
        "CONFIG SYS",
    ));
    let (dial, _srv) = spawn(transport, agent_grpc::server::prompt_router(store)).await;
    let client = agent_grpc::client::GrpcPrompts::connect(&dial).unwrap();

    // A mode lens reads back as its compiled default (builtin) before any override.
    let debug = client
        .get(&PromptRef {
            kind: PromptKind::ModeLens,
            id: "debug".into(),
        })
        .await
        .unwrap();
    assert!(debug.builtin);
    assert!(debug.content.contains("DEBUGGING"));

    // Put a prepend file, then it appears in a filtered list, ordered, non-builtin.
    client
        .put(PromptEntry {
            kind: PromptKind::Prepend,
            id: "0001_persona.md".into(),
            content: "You are helpful.".into(),
            builtin: false,
            read_only: false,
            order: 0,
            tags: Vec::new(),
        })
        .await
        .unwrap();
    let pre = client.list(Some(PromptKind::Prepend)).await.unwrap();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0].id, "0001_persona.md");
    assert_eq!(pre[0].order, 1);
    assert!(!pre[0].builtin);

    // Preview assembles [system, user, system?] with the prepend folded into system.
    let msgs = client
        .preview_assembled(&PromptContext::new().with_tag("mode:implement"), "GOAL")
        .await
        .unwrap();
    assert!(msgs[0].content_text().contains("You are helpful."));
    assert_eq!(msgs[1].content_text(), "GOAL");

    // Delete removes it; a second delete is a benign `false`.
    assert!(client
        .delete(&PromptRef {
            kind: PromptKind::Prepend,
            id: "0001_persona.md".into()
        })
        .await
        .unwrap());
    assert!(!client
        .delete(&PromptRef {
            kind: PromptKind::Prepend,
            id: "0001_persona.md".into()
        })
        .await
        .unwrap());
}

// A traversing id is rejected server-side and surfaces as an Err across the wire —
// the untrusted-input guard survives the hop.
#[tokio::test]
async fn adversarial_prompt_traversal_rejected_over_wire() {
    use agent_core::{PromptEntry, PromptKind, PromptStore};
    let root = tempdir();
    let store = Arc::new(agent_prompt::FilePromptStore::new(
        root.join("context.d"),
        root.join("prompts"),
        "CONFIG SYS",
    ));
    let (dial, _srv) = spawn(Transport::Tcp, agent_grpc::server::prompt_router(store)).await;
    let client = agent_grpc::client::GrpcPrompts::connect(&dial).unwrap();

    let err = client
        .put(PromptEntry {
            kind: PromptKind::Prepend,
            id: "../../evil.md".into(),
            content: "x".into(),
            builtin: false,
            read_only: false,
            order: 0,
            tags: Vec::new(),
        })
        .await;
    assert!(err.is_err(), "traversal must be rejected across the wire");
    assert!(!root.join("evil.md").exists());
}

// A tagged system fragment survives the hop: the `SystemFragment` kind and the
// derived `tags` (dir tag ∪ frontmatter) read back over the wire — the additive
// wire (PROMPT_KIND_SYSTEM_FRAGMENT + PromptEntry.tags) is faithful on TCP and UDS.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test]
async fn system_fragment_tags_roundtrip(#[case] transport: Transport) {
    use agent_core::{PromptEntry, PromptKind, PromptRef, PromptStore};
    let root = tempdir();
    let store = Arc::new(agent_prompt::FilePromptStore::new(
        root.join("context.d"),
        root.join("prompts"),
        "CONFIG SYS",
    ));
    let (dial, _srv) = spawn(transport, agent_grpc::server::prompt_router(store)).await;
    let client = agent_grpc::client::GrpcPrompts::connect(&dial).unwrap();

    client
        .put(PromptEntry {
            kind: PromptKind::SystemFragment,
            id: "review/0001_focus.md".into(),
            content: "---\ntags: [language:rust]\n---\nGROUND IT".into(),
            builtin: false,
            read_only: false,
            order: 0,
            tags: Vec::new(),
        })
        .await
        .unwrap();

    let e = client
        .get(&PromptRef {
            kind: PromptKind::SystemFragment,
            id: "review/0001_focus.md".into(),
        })
        .await
        .unwrap();
    assert_eq!(e.kind, PromptKind::SystemFragment);
    assert_eq!(
        e.tags,
        vec!["language:rust".to_string(), "mode:review".into()] // sorted
    );

    let frags = client.list(Some(PromptKind::SystemFragment)).await.unwrap();
    assert_eq!(frags.len(), 1);
    assert_eq!(frags[0].id, "review/0001_focus.md");
    assert!(frags[0].tags.contains(&"language:rust".to_string()));
}

// The `Select(PromptContext)` RPC and preview-by-context survive the hop: a Review
// context selects the review fragment and folds it into the previewed system message,
// while an unrelated context selects nothing. Runs on TCP and UDS.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test]
async fn prompt_select_and_preview_by_context_roundtrip(#[case] transport: Transport) {
    use agent_core::{PromptContext, PromptEntry, PromptKind, PromptStore};
    let root = tempdir();
    let store = Arc::new(agent_prompt::FilePromptStore::new(
        root.join("context.d"),
        root.join("prompts"),
        "CONFIG SYS",
    ));
    let (dial, _srv) = spawn(transport, agent_grpc::server::prompt_router(store)).await;
    let client = agent_grpc::client::GrpcPrompts::connect(&dial).unwrap();

    client
        .put(PromptEntry {
            kind: PromptKind::SystemFragment,
            id: "review/0001_focus.md".into(),
            content: "GROUND EVERY COMMENT".into(),
            builtin: false,
            read_only: false,
            order: 0,
            tags: Vec::new(),
        })
        .await
        .unwrap();

    // Select for the Review context returns the fragment as a full entry.
    let review = PromptContext::new().with_tag("mode:review");
    let sel = client.select(&review).await.unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel[0].id, "review/0001_focus.md");
    assert_eq!(sel[0].kind, PromptKind::SystemFragment);

    // An unrelated context selects nothing.
    let sel = client
        .select(&PromptContext::new().with_tag("mode:debug"))
        .await
        .unwrap();
    assert!(sel.is_empty());

    // Preview-by-context folds the selected fragment in at index 1.
    let msgs = client.preview_assembled(&review, "GOAL").await.unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[1].content_text(), "GROUND EVERY COMMENT");
    assert_eq!(msgs[2].content_text(), "GOAL");
}

// ---------------------------------------------------------------------------
// MetricsProxy (docs/design/portal) — PromQL over gRPC, via a double
// ---------------------------------------------------------------------------

// A canned proxy (no real Prometheus in the sandbox): echoes a fixed vector for a
// normal query and an error shape for the query carrying "boom", so the round-trip
// asserts labels + samples + the class-only error survive the hop on TCP and UDS.
struct StaticMetricsProxy;

#[async_trait]
impl agent_core::MetricsProxy for StaticMetricsProxy {
    async fn query(&self, q: &agent_core::PromQuery) -> agent_core::Result<agent_core::PromResult> {
        if q.query.contains("boom") {
            return Ok(agent_core::PromResult {
                error: "upstream error (bad_data)".into(),
                ..Default::default()
            });
        }
        let mut labels = std::collections::HashMap::new();
        labels.insert("job".to_string(), "agent".to_string());
        Ok(agent_core::PromResult {
            result_type: "vector".into(),
            series: vec![agent_core::PromSeries {
                labels,
                samples: vec![agent_core::PromSample {
                    t_unix_ms: 1_700_000_000_000,
                    value: 42.0,
                }],
            }],
            error: String::new(),
        })
    }

    async fn query_range(
        &self,
        _q: &agent_core::PromRangeQuery,
    ) -> agent_core::Result<agent_core::PromResult> {
        Ok(agent_core::PromResult {
            result_type: "matrix".into(),
            series: vec![agent_core::PromSeries {
                labels: Default::default(),
                samples: vec![
                    agent_core::PromSample {
                        t_unix_ms: 1000,
                        value: 1.0,
                    },
                    agent_core::PromSample {
                        t_unix_ms: 2000,
                        value: 2.0,
                    },
                ],
            }],
            error: String::new(),
        })
    }
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test]
async fn metrics_proxy_roundtrips(#[case] transport: Transport) {
    use agent_core::{MetricsProxy, PromQuery, PromRangeQuery};
    let (dial, _srv) = spawn(
        transport,
        agent_grpc::server::metrics_proxy_router(Arc::new(StaticMetricsProxy)),
    )
    .await;
    let client = agent_grpc::client::GrpcMetricsProxy::connect(&dial).unwrap();

    // A vector result: labels + sample survive the hop.
    let r = client
        .query(&PromQuery {
            query: "up".into(),
            time_unix_ms: Some(1_700_000_000_000),
        })
        .await
        .unwrap();
    assert_eq!(r.result_type, "vector");
    assert_eq!(r.series[0].labels.get("job").unwrap(), "agent");
    assert_eq!(r.series[0].samples[0].value, 42.0);
    assert!(r.error.is_empty());

    // A range result: multiple samples survive.
    let rr = client
        .query_range(&PromRangeQuery {
            query: "rate(x[5m])".into(),
            start_unix_ms: 1000,
            end_unix_ms: 2000,
            step_secs: 15,
        })
        .await
        .unwrap();
    assert_eq!(rr.result_type, "matrix");
    assert_eq!(rr.series[0].samples.len(), 2);

    // A class-only error survives without a raw body.
    let err = client
        .query(&PromQuery {
            query: "boom".into(),
            time_unix_ms: None,
        })
        .await
        .unwrap();
    assert_eq!(err.error, "upstream error (bad_data)");
    assert!(err.series.is_empty());
}

// ---------------------------------------------------------------------------
// AgentSession observation (docs/design/portal) — stream + snapshot, via a double
// ---------------------------------------------------------------------------

// A canned session source: a fixed snapshot + a short scripted event stream, so the
// round-trip asserts the leading snapshot event, the oneof mapping, and the Snapshot
// RPC survive the hop on TCP and UDS.
struct FakeSession;

impl agent_core::SessionSource for FakeSession {
    fn snapshot(&self) -> agent_core::StatusSnapshot {
        agent_core::StatusSnapshot {
            current_mode: "debug".into(),
            context_tokens: 42,
            context_window: 8192,
            context_messages: 5,
            active: true,
        }
    }
    fn subscribe(&self) -> agent_core::SessionEventStream {
        let events = vec![
            agent_core::SessionEvent::IterationStart { iter: 1 },
            agent_core::SessionEvent::TokenDelta { text: "hi".into() },
            agent_core::SessionEvent::RunFinished { ok: true },
        ];
        Box::pin(futures_util::stream::iter(events))
    }
}

// A registry holding one named session, so the round-trip exercises the `session_id`
// selector added in multi-session increment 03b (hazard B): an empty selector
// resolves the sole session, an unknown id is NOT_FOUND, a malformed one is rejected.
struct OneSessionRegistry {
    id: String,
    source: Arc<dyn agent_core::SessionSource>,
}

impl agent_core::SessionSourceRegistry for OneSessionRegistry {
    fn source(&self, session_id: &str) -> Option<Arc<dyn agent_core::SessionSource>> {
        (session_id == self.id).then(|| self.source.clone())
    }
    fn sole_source(&self) -> Option<Arc<dyn agent_core::SessionSource>> {
        Some(self.source.clone())
    }
    fn live_session_ids(&self) -> Vec<String> {
        vec![self.id.clone()]
    }
}

fn one_session_registry() -> Arc<dyn agent_core::SessionSourceRegistry> {
    Arc::new(OneSessionRegistry {
        id: "sess-1".into(),
        source: Arc::new(FakeSession),
    })
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test]
async fn agent_session_stream_and_snapshot_roundtrip(#[case] transport: Transport) {
    use agent_proto::pb;
    let (dial, _srv) = spawn(
        transport,
        agent_grpc::server::agent_session_router(one_session_registry()),
    )
    .await;
    let channel = dial.connect_lazy().unwrap();
    let mut client = pb::agent_session_service_client::AgentSessionServiceClient::new(channel);

    // Snapshot RPC: current state survives the hop (empty selector → sole session).
    let snap = client
        .snapshot(pb::SnapshotRequest {
            session_id: String::new(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(snap.current_mode, "debug");
    assert_eq!(snap.context_tokens, 42);
    assert!(snap.active);

    // The `session_id` selector survives the hop: naming the live session resolves it.
    let by_id = client
        .snapshot(pb::SnapshotRequest {
            session_id: "sess-1".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(by_id.current_mode, "debug");

    // Adversarial / miss cases: an unknown id is NOT_FOUND, a malformed one (path
    // traversal) is rejected as INVALID_ARGUMENT before any lookup.
    let unknown = client
        .snapshot(pb::SnapshotRequest {
            session_id: "ghost".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), tonic::Code::NotFound);
    let malformed = client
        .snapshot(pb::SnapshotRequest {
            session_id: "../../etc/passwd".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(malformed.code(), tonic::Code::InvalidArgument);

    // Subscribe: the first event is the status snapshot, then the scripted tail.
    let mut stream = client
        .subscribe(pb::SubscribeRequest {
            session_id: String::new(),
        })
        .await
        .unwrap()
        .into_inner();

    let mut kinds: Vec<&'static str> = Vec::new();
    while let Some(ev) = stream.message().await.unwrap() {
        match ev.kind.unwrap() {
            pb::session_event::Kind::StatusSnapshot(s) => {
                assert_eq!(s.current_mode, "debug");
                kinds.push("snapshot");
            }
            pb::session_event::Kind::Iteration(i) => {
                assert_eq!(i.iter, 1);
                kinds.push("iteration");
            }
            pb::session_event::Kind::Token(t) => {
                assert_eq!(t.text, "hi");
                kinds.push("token");
            }
            pb::session_event::Kind::RunFinished(f) => {
                assert!(f.ok);
                kinds.push("run_finished");
            }
            other => panic!("unexpected event {other:?}"),
        }
    }
    assert_eq!(
        kinds,
        vec!["snapshot", "iteration", "token", "run_finished"]
    );
}

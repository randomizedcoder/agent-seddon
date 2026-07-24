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

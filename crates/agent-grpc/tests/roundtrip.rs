//! Every seam, round-tripped over gRPC on **both** TCP and a unix domain socket.
//!
//! Each test binds a real server (an ephemeral `127.0.0.1:0` port or a temp-dir
//! socket), connects the matching client, invokes the seam, and asserts. The
//! transport is a table-driven `#[case]` so the exact same assertions run over TCP
//! and UDS.

use std::sync::Arc;

use agent_core::{
    CompletionRequest, ContextInput, ContextStrategy, Decision, LlmProvider, MemoryEvent,
    MemoryStore, Message, ModelCapabilities, Policy, RecallQuery, TokenBudget, ToolCall,
    ToolContext, WorkingSet,
};
use agent_grpc::client::{grpc_tools, GrpcContext, GrpcMemory, GrpcPolicy, GrpcProvider};
use agent_grpc::server::{
    context_router, memory_router, policy_router, provider_router, tools_router,
};
use agent_grpc::Endpoint;
use agent_testkit::{
    final_turn, tempdir, EchoTool, RecordingMemory, ScriptedProvider, StaticContext,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use rstest::rstest;
use tokio::sync::oneshot;
use tonic::transport::server::Router;

/// Which transport a test case uses.
#[derive(Clone, Copy)]
enum Transport {
    Tcp,
    Uds,
}

impl Transport {
    /// A fresh listen endpoint: an ephemeral loopback port, or a temp-dir socket.
    fn listen(self) -> Endpoint {
        match self {
            Transport::Tcp => Endpoint::parse("127.0.0.1:0"),
            Transport::Uds => {
                let path = tempdir().join("seam.sock");
                Endpoint::parse(&format!("unix:{}", path.display()))
            }
        }
    }
}

/// A running test server; signals shutdown on drop so the socket is cleaned up.
struct TestServer {
    shutdown: Option<oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

/// Bind `router` on `transport` and spawn it; return the dial endpoint + a guard.
async fn spawn(transport: Transport, router: Router) -> (Endpoint, TestServer) {
    let bound = transport.listen().bind().await.expect("bind");
    let dial = bound.dial_endpoint().expect("dial endpoint");
    let (tx, rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let _ = bound
            .serve(router, async {
                let _ = rx.await;
            })
            .await;
    });
    (
        dial,
        TestServer {
            shutdown: Some(tx),
            _handle: handle,
        },
    )
}

fn caps() -> ModelCapabilities {
    ModelCapabilities {
        supports_tools: true,
        context_window: 1000,
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
    };
    let resp = client.complete(req).await.unwrap();
    assert_eq!(resp.message.content, "hello from gateway");
    assert!(client.capabilities().supports_tools);
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
    assert_eq!(events[0].message.content, "remembered over the wire");
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
    assert_eq!(messages[1].content, "do the thing");

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
    assert_eq!(working.messages[0].content, "keep me");
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

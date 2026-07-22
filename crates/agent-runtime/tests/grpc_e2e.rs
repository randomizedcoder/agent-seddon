//! The distributed loop, end to end.
//!
//! Every other gRPC test in this project verifies **one client against one
//! server**. That is necessary and not sufficient: it proves each seam can cross
//! a wire, not that the *agent* still works when several of them do at once.
//!
//! These tests build a real `Agent` through `build_agent_with` with four seams
//! resolved over gRPC simultaneously — policy, context, memory, tokenizer — each
//! bound to its own ephemeral endpoint, and then run real turns through it. The
//! only thing faked is the model.
//!
//! This is the claim the whole distribution program rests on: *the loop cannot
//! tell a remote seam from a local one.*

#![cfg(feature = "grpc")]

use std::sync::Arc;

use agent_core::{
    CompletionResponse, Decision, LlmProvider, MemoryEvent, MemoryItem, MemoryStore, Policy,
    RecallQuery, Result, ToolCall,
};
use agent_grpc::server::{context_router, memory_router, policy_router, tokenizer_router};
use agent_grpc::Endpoint;
use agent_runtime::{build_agent_with, parse_config, register_builtins, Metrics, Registry};
use async_trait::async_trait;
use std::sync::Mutex;
use tokio::sync::oneshot;

/// A running seam server; shuts down on drop.
struct Server {
    _shutdown: Option<oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

/// Bind `router` on an ephemeral loopback port and return the dial address.
async fn serve(router: agent_grpc::server::Router) -> (String, Server) {
    let bound = Endpoint::parse("127.0.0.1:0").bind().await.expect("bind");
    let dial = match bound.dial_endpoint().expect("dial") {
        Endpoint::Tcp(hostport) => hostport,
        Endpoint::Uds(p) => format!("unix:{}", p.display()),
    };
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
        Server {
            _shutdown: Some(tx),
            _handle: handle,
        },
    )
}

/// Allows everything, and counts what it was asked about — so a test can prove
/// the *remote* policy was consulted, not a local fallback.
#[derive(Default)]
struct CountingPolicy {
    seen: Mutex<Vec<String>>,
}

#[async_trait]
impl Policy for CountingPolicy {
    async fn authorize(&self, call: &ToolCall) -> Decision {
        self.seen.lock().unwrap().push(call.name.clone());
        Decision::Allow
    }
}

/// Denies one named tool, to prove a remote denial reaches the loop.
struct DenyTool(&'static str);

#[async_trait]
impl Policy for DenyTool {
    async fn authorize(&self, call: &ToolCall) -> Decision {
        if call.name == self.0 {
            Decision::Deny("denied by the remote policy service".into())
        } else {
            Decision::Allow
        }
    }
}

/// Records appends so a test can prove the remote store received them.
#[derive(Default)]
struct RecordingMemory {
    appended: Mutex<Vec<MemoryEvent>>,
}

#[async_trait]
impl MemoryStore for RecordingMemory {
    async fn recall(&self, _q: &RecallQuery) -> Result<Vec<MemoryItem>> {
        Ok(vec![])
    }
    async fn append(&self, event: MemoryEvent) -> Result<()> {
        self.appended.lock().unwrap().push(event);
        Ok(())
    }
    async fn distill(&self) -> Result<usize> {
        Ok(0)
    }
}

fn config(
    policy: &str,
    context: &str,
    memory: &str,
    tokenizer: &str,
    dir: &std::path::Path,
) -> String {
    format!(
        r#"
        [agent]
        provider    = "scripted"
        policy      = "grpc"
        context     = "grpc"
        stream      = false
        working_dir = "{dir}"
        max_iterations = 6

        [provider]
        model = "scripted-model"

        [memory]
        backend       = "grpc"
        episodic_path = "{dir}/.agent/episodic.jsonl"
        semantic_dir  = "{dir}/.agent/memory"

        [search]
        index_dir  = "{dir}/.agent/index"
        auto_index = false

        [git]
        mirror_dir      = "{dir}/.agent/mirror"
        worktrees_dir   = "{dir}/.agent/worktrees"
        auto_fetch_secs = 0

        [tokenizer]
        backend = "grpc"

        [grpc.policy]
        endpoint = "{policy}"
        [grpc.context]
        endpoint = "{context}"
        [grpc.memory]
        endpoint = "{memory}"
        [grpc.tokenizer]
        endpoint = "{tokenizer}"
        "#,
        dir = dir.display()
    )
}

async fn agent_with(
    cfg_toml: &str,
    script: Vec<CompletionResponse>,
) -> anyhow::Result<agent_runtime::Agent> {
    let cfg = parse_config(cfg_toml).expect("parse config");
    let mut registry = Registry::new();
    register_builtins(&mut registry);
    let script = Mutex::new(Some(script));
    registry.provider("scripted", move |_ctx| {
        let s = script
            .lock()
            .unwrap()
            .take()
            .expect("scripted provider built once");
        Ok(Arc::new(agent_testkit::ScriptedProvider::new(s)) as Arc<dyn LlmProvider>)
    });
    build_agent_with(&registry, cfg, None, "grpc-e2e".into(), Metrics::new()).await
}

/// **The claim.** A real turn, with policy, context, memory and the tokenizer
/// all resolved over gRPC at the same time. If the loop can't tell the
/// difference, this passes and the remote policy saw the tool call.
#[tokio::test(flavor = "multi_thread")]
async fn positive_a_turn_runs_with_four_seams_over_the_wire() {
    let dir = agent_testkit::tempdir();
    let policy = Arc::new(CountingPolicy::default());
    let memory = Arc::new(RecordingMemory::default());
    let policy_probe = policy.clone();
    let memory_probe = memory.clone();

    let (p, _s1) = serve(policy_router(policy)).await;
    let (c, _s2) = serve(context_router(Arc::new(agent_testkit::StaticContext))).await;
    let (m, _s3) = serve(memory_router(memory)).await;
    let (t, _s4) = serve(tokenizer_router(Arc::new(
        agent_tokenizer::ApproxTokenizer::new(),
    )))
    .await;

    let agent = agent_with(
        &config(&p, &c, &m, &t, &dir),
        vec![
            agent_testkit::tool_turn(vec![ToolCall {
                id: "1".into(),
                name: "echo".into(),
                arguments: serde_json::json!({"text": "hello"}),
            }]),
            agent_testkit::final_turn("done over grpc"),
        ],
    )
    .await
    .expect("build an agent whose seams are all remote");

    let mut session = agent.session();
    let answer = session
        .send("do the thing")
        .await
        .expect("the turn completes");
    assert_eq!(answer.trim(), "done over grpc");

    // The REMOTE policy was consulted — not a local default that silently took
    // over. Without this the test would pass even if the grpc wiring were
    // ignored entirely.
    let seen = policy_probe.seen.lock().unwrap().clone();
    assert!(
        seen.iter().any(|n| n == "echo"),
        "the remote policy service should have authorized `echo`, saw {seen:?}"
    );
    // …and the remote memory received the turn.
    assert!(
        !memory_probe.appended.lock().unwrap().is_empty(),
        "the remote memory service should have received an append"
    );
}

/// A denial from the **remote** policy must reach the loop as a denial — the
/// gate works across the wire, or distributing it silently disables it.
#[tokio::test(flavor = "multi_thread")]
async fn negative_a_remote_denial_stops_the_tool_call() {
    let dir = agent_testkit::tempdir();
    let (p, _s1) = serve(policy_router(Arc::new(DenyTool("echo")))).await;
    let (c, _s2) = serve(context_router(Arc::new(agent_testkit::StaticContext))).await;
    let (m, _s3) = serve(memory_router(Arc::new(RecordingMemory::default()))).await;
    let (t, _s4) = serve(tokenizer_router(Arc::new(
        agent_tokenizer::ApproxTokenizer::new(),
    )))
    .await;

    let agent = agent_with(
        &config(&p, &c, &m, &t, &dir),
        vec![
            agent_testkit::tool_turn(vec![ToolCall {
                id: "1".into(),
                name: "echo".into(),
                arguments: serde_json::json!({"text": "hello"}),
            }]),
            agent_testkit::final_turn("stopped"),
        ],
    )
    .await
    .expect("build agent");

    let mut session = agent.session();
    let answer = session.send("do the thing").await.expect("turn completes");
    assert_eq!(answer.trim(), "stopped");

    // The denial reached the model as an observation rather than vanishing.
    let transcript = format!("{:?}", session.messages());
    assert!(
        transcript.contains("denied by the remote policy service"),
        "the remote denial must reach the loop; transcript: {transcript}"
    );
}

/// A seam server that is **down** must fail the build or the turn loudly — never
/// silently fall back to a local implementation. A quiet fallback would mean an
/// operator who configured a central policy service was not actually using it.
#[tokio::test(flavor = "multi_thread")]
async fn negative_an_unreachable_seam_does_not_silently_fall_back() {
    let dir = agent_testkit::tempdir();
    // Everything real except policy, which points at a dead port.
    let (c, _s2) = serve(context_router(Arc::new(agent_testkit::StaticContext))).await;
    let (m, _s3) = serve(memory_router(Arc::new(RecordingMemory::default()))).await;
    let (t, _s4) = serve(tokenizer_router(Arc::new(
        agent_tokenizer::ApproxTokenizer::new(),
    )))
    .await;

    let agent = agent_with(
        &config("127.0.0.1:1", &c, &m, &t, &dir),
        vec![
            agent_testkit::tool_turn(vec![ToolCall {
                id: "1".into(),
                name: "echo".into(),
                arguments: serde_json::json!({"text": "hello"}),
            }]),
            agent_testkit::final_turn("finished"),
        ],
    )
    .await
    .expect("the channel is lazy, so the build succeeds");

    let mut session = agent.session();
    let answer = session.send("do the thing").await.expect("turn completes");
    // The policy client fails SAFE: an unreachable policy service denies rather
    // than allowing, so the tool did not run.
    let transcript = format!("{:?}", session.messages());
    assert!(
        transcript.contains("policy rpc failed"),
        "an unreachable policy must deny visibly, not allow silently; got: {transcript}"
    );
    assert_eq!(answer.trim(), "finished");
}

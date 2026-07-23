//! End-to-end tests for the `Verifier` seam wired into the real loop, in SHADOW
//! mode (increment 1): the verifier evaluates each allowed tool call and its
//! verdict is observed, but it does NOT change the loop's behaviour yet.
//!
//! These drive the production path (`build_agent_with` → registry → loop) with a
//! scripted model, and prove three things: the seam is invoked for the calls that
//! would run, shadow does not block (even on a Deny/Revise), and a denied call is
//! never verified. The `SchemaVerifier`'s own logic is unit-tested in
//! `agent-verifier`; here we prove the wiring.

use agent_core::{
    CompletionResponse, LlmProvider, ToolCall, Verifier, VerifierReport, VerifyCtx, VerifyVerdict,
};
use agent_runtime::{build_agent_with, parse_config, register_builtins, Metrics, Registry};
use agent_testkit::{final_turn, tempdir, tool_turn};
use async_trait::async_trait;
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A verifier double: counts how many calls it was asked to judge, and returns a
/// fixed verdict. Lets a test assert the loop invoked the seam (and how often)
/// without depending on span capture across async.
struct RecordingVerifier {
    seen: Arc<AtomicUsize>,
    verdict: VerifyVerdict,
}

#[async_trait]
impl Verifier for RecordingVerifier {
    fn name(&self) -> &str {
        "recording"
    }
    async fn verify(&self, _ctx: &VerifyCtx<'_>) -> VerifierReport {
        self.seen.fetch_add(1, Ordering::SeqCst);
        VerifierReport {
            verdict: self.verdict.clone(),
            confidence: 1.0,
            model: "recording".into(),
        }
    }
}

fn config_toml(dir: &Path, verifier_backend: &str) -> String {
    let d = dir.display();
    format!(
        r#"
        [agent]
        provider = "scripted"
        policy = "auto-approve"
        stream = false
        working_dir = "{d}"
        max_iterations = 20

        [provider]
        model = "scripted-model"

        [memory]
        episodic_path = "{d}/.agent/episodic.jsonl"
        semantic_dir = "{d}/.agent/memory"

        [search]
        index_dir = "{d}/.agent/index"
        auto_index = false

        [git]
        auto_fetch_secs = 0

        [tokenizer]
        backend = "approx"

        [verifier]
        backend = "{verifier_backend}"
    "#
    )
}

fn call(id: &str, name: &str, args: serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: args,
    }
}

/// Build the agent with the scripted model and a `RecordingVerifier` registered
/// under `"recording"`, returning the shared call-counter so a test can assert it.
async fn agent_with_recording_verifier(
    dir: &Path,
    verdict: VerifyVerdict,
    script: Vec<CompletionResponse>,
) -> (agent_runtime::Agent, Arc<AtomicUsize>) {
    let cfg = parse_config(&config_toml(dir, "recording")).expect("parse config");
    let mut registry = Registry::new();
    register_builtins(&mut registry);

    let script = std::sync::Mutex::new(Some(script));
    registry.provider("scripted", move |_ctx| {
        let s = script.lock().unwrap().take().expect("built once");
        Ok(Arc::new(agent_testkit::ScriptedProvider::new(s)) as Arc<dyn LlmProvider>)
    });

    let seen = Arc::new(AtomicUsize::new(0));
    let seen_for_factory = seen.clone();
    let verdict = std::sync::Mutex::new(Some(verdict));
    registry.verifier("recording", move |_ctx| {
        Ok(Arc::new(RecordingVerifier {
            seen: seen_for_factory.clone(),
            verdict: verdict.lock().unwrap().take().expect("built once"),
        }) as Arc<dyn Verifier>)
    });

    let agent = build_agent_with(&registry, cfg, None, "verifier-e2e".into(), Metrics::new())
        .await
        .expect("build agent");
    (agent, seen)
}

/// The seam is invoked once per allowed tool call, and the call still runs
/// (shadow is transparent on the happy path).
#[tokio::test]
async fn positive_verifier_runs_in_shadow_on_each_allowed_call() {
    let dir = tempdir();
    let script = vec![
        tool_turn(vec![call(
            "1",
            "write_file",
            json!({"path": "note.txt", "content": "hi"}),
        )]),
        final_turn("done"),
    ];
    let (agent, seen) = agent_with_recording_verifier(&dir, VerifyVerdict::Allow, script).await;

    agent.run("write a note").await.expect("run");

    assert_eq!(
        seen.load(Ordering::SeqCst),
        1,
        "verifier judged the one call"
    );
    // Shadow is transparent: the tool actually ran.
    assert_eq!(
        std::fs::read_to_string(dir.join("note.txt")).expect("written"),
        "hi"
    );
}

/// **Shadow semantics.** A `Revise` verdict is observed but NOT enforced in
/// increment 1: the call still executes. (Enforcement is a follow-up.)
#[tokio::test]
async fn boundary_shadow_revise_does_not_block_the_call() {
    let dir = tempdir();
    let script = vec![
        tool_turn(vec![call(
            "1",
            "write_file",
            json!({"path": "still.txt", "content": "written despite revise"}),
        )]),
        final_turn("done"),
    ];
    let (agent, seen) =
        agent_with_recording_verifier(&dir, VerifyVerdict::Revise("please fix".into()), script)
            .await;

    agent.run("write").await.expect("run");

    assert_eq!(seen.load(Ordering::SeqCst), 1);
    assert!(
        dir.join("still.txt").exists(),
        "shadow must NOT block a Revise'd call in increment 1"
    );
}

/// **Shadow semantics.** Even a `Deny` verdict does not block in shadow mode —
/// this pins the increment-1 contract so enforcement is an explicit later change.
#[tokio::test]
async fn boundary_shadow_deny_does_not_block_the_call() {
    let dir = tempdir();
    let script = vec![
        tool_turn(vec![call(
            "1",
            "write_file",
            json!({"path": "denied.txt", "content": "still written"}),
        )]),
        final_turn("done"),
    ];
    let (agent, _seen) =
        agent_with_recording_verifier(&dir, VerifyVerdict::Deny("nope".into()), script).await;

    agent.run("write").await.expect("run");

    assert!(
        dir.join("denied.txt").exists(),
        "shadow must NOT enforce a Deny in increment 1"
    );
}

/// A call the **policy** denies is never handed to the verifier — only calls that
/// would actually run are shadowed.
#[tokio::test]
async fn negative_policy_denied_call_is_not_verified() {
    let dir = tempdir();
    // allow-list policy with an empty allow set denies every call.
    let cfg = parse_config(&format!(
        r#"
        [agent]
        provider = "scripted"
        policy = "allow-list"
        stream = false
        working_dir = "{d}"
        max_iterations = 3
        [provider]
        model = "m"
        [memory]
        episodic_path = "{d}/.agent/episodic.jsonl"
        semantic_dir = "{d}/.agent/memory"
        [search]
        auto_index = false
        [git]
        auto_fetch_secs = 0
        [tokenizer]
        backend = "approx"
        [verifier]
        backend = "recording"
        [policy]
        allow = []
    "#,
        d = dir.display()
    ))
    .expect("parse");

    let mut registry = Registry::new();
    register_builtins(&mut registry);
    let script = std::sync::Mutex::new(Some(vec![
        tool_turn(vec![call(
            "1",
            "write_file",
            json!({"path": "x", "content": "y"}),
        )]),
        final_turn("done"),
    ]));
    registry.provider("scripted", move |_ctx| {
        Ok(Arc::new(agent_testkit::ScriptedProvider::new(
            script.lock().unwrap().take().unwrap(),
        )) as Arc<dyn LlmProvider>)
    });
    let seen = Arc::new(AtomicUsize::new(0));
    let seen2 = seen.clone();
    registry.verifier("recording", move |_ctx| {
        Ok(Arc::new(RecordingVerifier {
            seen: seen2.clone(),
            verdict: VerifyVerdict::Allow,
        }) as Arc<dyn Verifier>)
    });
    let agent = build_agent_with(&registry, cfg, None, "s".into(), Metrics::new())
        .await
        .expect("build");

    let _ = agent.run("write").await;

    assert_eq!(
        seen.load(Ordering::SeqCst),
        0,
        "a policy-denied call must not be verified"
    );
    assert!(!dir.join("x").exists(), "denied call must not have run");
}

/// The real registered `"schema"` backend wires up and, in shadow, a
/// schema-violating call is judged but still runs (the tool itself then errors) —
/// proving the built-in verifier, not just the test double, is reachable.
#[tokio::test]
async fn positive_schema_backend_wires_up_in_shadow() {
    let dir = tempdir();
    let cfg = parse_config(&config_toml(&dir, "schema")).expect("parse");
    let mut registry = Registry::new();
    register_builtins(&mut registry);
    // write_file with `path` as the wrong type: the schema verifier flags it, but
    // shadow does not block — the tool runs and returns its own error. The loop
    // must still terminate cleanly.
    let script = std::sync::Mutex::new(Some(vec![
        tool_turn(vec![call("1", "write_file", json!({"path": 123}))]),
        final_turn("handled"),
    ]));
    registry.provider("scripted", move |_ctx| {
        Ok(Arc::new(agent_testkit::ScriptedProvider::new(
            script.lock().unwrap().take().unwrap(),
        )) as Arc<dyn LlmProvider>)
    });
    let agent = build_agent_with(&registry, cfg, None, "s".into(), Metrics::new())
        .await
        .expect("build");

    let answer = agent.run("write badly").await.expect("run completes");
    assert!(
        answer.contains("handled"),
        "loop terminated normally: {answer}"
    );
}

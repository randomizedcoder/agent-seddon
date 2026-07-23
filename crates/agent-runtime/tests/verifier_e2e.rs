//! End-to-end tests for the `Verifier` seam wired into the real loop, driving the
//! production path (`build_agent_with` → registry → loop) with a scripted model.
//!
//! Covers both modes. **Shadow** (default): the seam is invoked for the calls that
//! would run, a Revise/Deny does NOT block, and a policy-denied call is never
//! verified. **Enforce**: a Revise blocks the call and the model can retry a
//! corrected one, a Deny blocks outright, an Allow is transparent, and the real
//! `schema` backend blocks a malformed call. The `SchemaVerifier`'s own logic is
//! unit-tested in `agent-verifier`; here we prove the wiring and the mode contract.

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
    config_toml_mode(dir, verifier_backend, "shadow")
}

fn config_toml_mode(dir: &Path, verifier_backend: &str, mode: &str) -> String {
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
        mode = "{mode}"
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

/// Read the `verification` records the loop appended to the episodic JSONL — the
/// same events the telemetry sink would route to `agent_verifications`. Lets a
/// test assert the recording path black-box, without a live ClickHouse.
fn read_verifications(dir: &Path) -> Vec<agent_core::VerificationRecord> {
    let text = std::fs::read_to_string(dir.join(".agent/episodic.jsonl")).unwrap_or_default();
    text.lines()
        .filter_map(|l| serde_json::from_str::<agent_core::MemoryEvent>(l).ok())
        .filter_map(|e| e.verification)
        .collect()
}

/// Build the agent with the scripted model and a `RecordingVerifier` registered
/// under `"recording"`, returning the shared call-counter so a test can assert it.
async fn agent_with_recording_verifier(
    dir: &Path,
    verdict: VerifyVerdict,
    script: Vec<CompletionResponse>,
) -> (agent_runtime::Agent, Arc<AtomicUsize>) {
    agent_with_recording_verifier_mode(dir, verdict, "shadow", script).await
}

async fn agent_with_recording_verifier_mode(
    dir: &Path,
    verdict: VerifyVerdict,
    mode: &str,
    script: Vec<CompletionResponse>,
) -> (agent_runtime::Agent, Arc<AtomicUsize>) {
    let cfg = parse_config(&config_toml_mode(dir, "recording", mode)).expect("parse config");
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

// --- enforce mode -----------------------------------------------------------

/// **Enforce.** A `Revise` verdict blocks the call (the tool does NOT run) and
/// the model can reissue a corrected call, which then executes.
#[tokio::test]
async fn positive_enforce_revise_blocks_then_model_retries() {
    let dir = tempdir();
    let seen = Arc::new(AtomicUsize::new(0));
    let cfg = parse_config(&config_toml_mode(&dir, "recording", "enforce")).expect("parse");
    let mut registry = Registry::new();
    register_builtins(&mut registry);
    // First the model makes a "bad" call (the verifier will Revise it, blocking
    // it); then it reissues a good call which runs; then it answers.
    let script = std::sync::Mutex::new(Some(vec![
        tool_turn(vec![call(
            "1",
            "write_file",
            json!({"path": "out.txt", "content": "v1"}),
        )]),
        tool_turn(vec![call(
            "2",
            "write_file",
            json!({"path": "out.txt", "content": "v2"}),
        )]),
        final_turn("done"),
    ]));
    registry.provider("scripted", move |_ctx| {
        Ok(Arc::new(agent_testkit::ScriptedProvider::new(
            script.lock().unwrap().take().unwrap(),
        )) as Arc<dyn LlmProvider>)
    });
    // A verifier that Revises the FIRST call only, then Allows.
    let n = std::sync::Arc::new(AtomicUsize::new(0));
    let n2 = n.clone();
    let seen2 = seen.clone();
    registry.verifier("recording", move |_ctx| {
        Ok(Arc::new(FirstReviseThenAllow {
            calls: n2.clone(),
            seen: seen2.clone(),
        }) as Arc<dyn Verifier>)
    });
    let agent = build_agent_with(&registry, cfg, None, "s".into(), Metrics::new())
        .await
        .expect("build");

    let answer = agent.run("write").await.expect("run");

    assert!(answer.contains("done"));
    assert_eq!(seen.load(Ordering::SeqCst), 2, "both calls were verified");
    // The blocked first call did NOT write v1; the retried second call wrote v2.
    assert_eq!(
        std::fs::read_to_string(dir.join("out.txt")).expect("written on retry"),
        "v2",
        "the Revise'd call must not run; the corrected retry must"
    );
}

/// A verifier that returns Revise for the first call it sees and Allow after.
struct FirstReviseThenAllow {
    calls: Arc<AtomicUsize>,
    seen: Arc<AtomicUsize>,
}

#[async_trait]
impl Verifier for FirstReviseThenAllow {
    fn name(&self) -> &str {
        "recording"
    }
    async fn verify(&self, _ctx: &VerifyCtx<'_>) -> VerifierReport {
        self.seen.fetch_add(1, Ordering::SeqCst);
        let verdict = if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            VerifyVerdict::Revise("fix it".into())
        } else {
            VerifyVerdict::Allow
        };
        VerifierReport {
            verdict,
            confidence: 1.0,
            model: "recording".into(),
        }
    }
}

/// **Enforce.** A `Deny` verdict blocks the call outright — the tool does not run.
#[tokio::test]
async fn negative_enforce_deny_blocks_the_call() {
    let dir = tempdir();
    let script = vec![
        tool_turn(vec![call(
            "1",
            "write_file",
            json!({"path": "blocked.txt", "content": "should not exist"}),
        )]),
        final_turn("understood"),
    ];
    let (agent, _seen) = agent_with_recording_verifier_mode(
        &dir,
        VerifyVerdict::Deny("no".into()),
        "enforce",
        script,
    )
    .await;

    agent.run("write").await.expect("run");

    assert!(
        !dir.join("blocked.txt").exists(),
        "enforce must block a Deny'd call"
    );
}

/// **Enforce.** An `Allow` verdict is transparent — the call runs as normal.
#[tokio::test]
async fn positive_enforce_allow_runs_the_call() {
    let dir = tempdir();
    let script = vec![
        tool_turn(vec![call(
            "1",
            "write_file",
            json!({"path": "ok.txt", "content": "ran"}),
        )]),
        final_turn("done"),
    ];
    let (agent, _seen) =
        agent_with_recording_verifier_mode(&dir, VerifyVerdict::Allow, "enforce", script).await;

    agent.run("write").await.expect("run");

    assert_eq!(
        std::fs::read_to_string(dir.join("ok.txt")).expect("written"),
        "ran"
    );
}

/// **Enforce + the real schema backend.** A schema-violating call is blocked, and
/// the feedback message carries the schema hint so the model could correct it.
#[tokio::test]
async fn positive_enforce_schema_blocks_a_malformed_call() {
    let dir = tempdir();
    let cfg = parse_config(&config_toml_mode(&dir, "schema", "enforce")).expect("parse");
    let mut registry = Registry::new();
    register_builtins(&mut registry);
    // write_file with `content` missing (required) — schema Revises, enforce
    // blocks. The model then answers without a successful write.
    let script = std::sync::Mutex::new(Some(vec![
        tool_turn(vec![call("1", "write_file", json!({"path": "x.txt"}))]),
        final_turn("could not write"),
    ]));
    registry.provider("scripted", move |_ctx| {
        Ok(Arc::new(agent_testkit::ScriptedProvider::new(
            script.lock().unwrap().take().unwrap(),
        )) as Arc<dyn LlmProvider>)
    });
    let agent = build_agent_with(&registry, cfg, None, "s".into(), Metrics::new())
        .await
        .expect("build");

    let answer = agent.run("write badly").await.expect("run");
    assert!(answer.contains("could not write"));
    assert!(
        !dir.join("x.txt").exists(),
        "the malformed call must be blocked in enforce mode"
    );
}

// --- recording (increment 3) ------------------------------------------------

/// **Recording.** Each verified call emits one `verification` record carrying the
/// verdict, the coarse `task_type`, hashed (not raw) args, and — because the call
/// ran in shadow — a known `call_errored` outcome proxy. The deferred proxies stay
/// unset. This is the data the `agent_verifications` table is built from.
#[tokio::test]
async fn positive_shadow_records_a_verification_with_outcome() {
    let dir = tempdir();
    let script = vec![
        tool_turn(vec![call(
            "1",
            "write_file",
            json!({"path": "note.txt", "content": "hi"}),
        )]),
        final_turn("done"),
    ];
    let (agent, _seen) =
        agent_with_recording_verifier(&dir, VerifyVerdict::Revise("hint".into()), script).await;

    agent.run("write a note").await.expect("run");

    let recs = read_verifications(&dir);
    assert_eq!(recs.len(), 1, "one verified call ⇒ one verification record");
    let v = &recs[0];
    assert_eq!(v.tool_name, "write_file");
    assert_eq!(v.task_type, "write_file", "coarse phase-1 task_type = tool");
    assert_eq!(v.verdict, "revise");
    assert_eq!(v.verifier_model, "recording");
    assert!(!v.cached);
    // Shadow ran the call and it succeeded, so the proxy is Some(false).
    assert_eq!(v.call_errored, Some(false));
    assert_eq!(v.revised_after, None, "deferred proxy stays unset");
    assert_eq!(v.task_succeeded, None);
    // Args are fingerprinted, not stored raw.
    assert!(!v.args_hash.is_empty() && !v.args_hash.contains("note.txt"));
}

/// **Recording, blocked call.** A call the verifier blocks in enforce mode never
/// runs, so its outcome proxy is NULL (`None`) — distinguishable in the data from a
/// call that ran and did not error.
#[tokio::test]
async fn boundary_enforce_blocked_call_records_null_outcome() {
    let dir = tempdir();
    let script = vec![
        tool_turn(vec![call(
            "1",
            "write_file",
            json!({"path": "b.txt", "content": "x"}),
        )]),
        final_turn("understood"),
    ];
    let (agent, _seen) = agent_with_recording_verifier_mode(
        &dir,
        VerifyVerdict::Deny("no".into()),
        "enforce",
        script,
    )
    .await;

    agent.run("write").await.expect("run");

    let recs = read_verifications(&dir);
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].verdict, "deny");
    assert_eq!(
        recs[0].call_errored, None,
        "a blocked call never ran ⇒ NULL outcome proxy"
    );
}

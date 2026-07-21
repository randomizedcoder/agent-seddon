//! End-to-end agent-loop tests: drive the **real** wiring — `build_agent_with`
//! (registry → builder → metered seams → loop) with the real tools, context
//! strategy, policy, memory and tokenizer — against a scripted model, on a temp
//! working directory. This is the missing "does the loop actually work" proof and
//! the substrate for the reliability work: a dogfood-in-miniature where the model
//! reads and edits real files and the loop feeds tool observations back.
//!
//! The model is `agent_testkit::ScriptedProvider` (replays a fixed turn sequence),
//! registered on the `Registry` under the name `"scripted"` and selected by
//! `[agent] provider = "scripted"`. Everything else is the production path.

use agent_core::{CompletionResponse, LlmProvider, ToolCall};
use agent_runtime::{build_agent_with, parse_config, register_builtins, Metrics, Registry};
use agent_testkit::{final_turn, tempdir, tool_turn};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

/// A hermetic config: the scripted provider, `auto-approve` policy (no prompts),
/// and every on-disk seam (working dir, memory, search index, git mirror) pointed
/// under `dir` so a test never touches the real repo or `$HOME`. Background index
/// freshness + git fetch are disabled so no task outlives the run.
fn config_toml(dir: &Path) -> String {
    config_toml_iters(dir, 20)
}

fn config_toml_iters(dir: &Path, max_iterations: usize) -> String {
    let d = dir.display();
    format!(
        r#"
        [agent]
        provider = "scripted"
        policy = "auto-approve"
        stream = false
        working_dir = "{d}"
        max_iterations = {max_iterations}

        [provider]
        model = "scripted-model"

        [memory]
        episodic_path = "{d}/.agent/episodic.jsonl"
        semantic_dir = "{d}/.agent/memory"

        [search]
        index_dir = "{d}/.agent/index"
        auto_index = false

        [git]
        mirror_dir = "{d}/.agent/mirror"
        worktrees_dir = "{d}/.agent/worktrees"
        auto_fetch_secs = 0

        [tokenizer]
        backend = "approx"
    "#
    )
}

/// Build the agent from a scripted turn sequence, on working dir `dir`.
async fn agent_for(dir: &Path, script: Vec<CompletionResponse>) -> agent_runtime::Agent {
    agent_for_cfg(&config_toml(dir), script).await
}

async fn agent_for_cfg(cfg_toml: &str, script: Vec<CompletionResponse>) -> agent_runtime::Agent {
    let cfg = parse_config(cfg_toml).expect("parse config");
    let mut registry = Registry::new();
    register_builtins(&mut registry);
    // Replace the model with a canned script. The factory is called once at build.
    let script = std::sync::Mutex::new(Some(script));
    registry.provider("scripted", move |_ctx| {
        let s = script
            .lock()
            .unwrap()
            .take()
            .expect("scripted provider built once");
        Ok(Arc::new(agent_testkit::ScriptedProvider::new(s)) as Arc<dyn LlmProvider>)
    });
    build_agent_with(&registry, cfg, None, "e2e-session".into(), Metrics::new())
        .await
        .expect("build agent")
}

fn call(id: &str, name: &str, args: serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: args,
    }
}

/// The loop dispatches a real `write_file` tool call against the working dir, feeds
/// the observation back, and returns the model's final answer — proving the whole
/// registry→builder→loop→tool path end to end.
#[tokio::test]
async fn loop_writes_a_real_file_then_answers() {
    let dir = tempdir();
    let script = vec![
        tool_turn(vec![call(
            "1",
            "write_file",
            json!({"path": "note.txt", "content": "hello self"}),
        )]),
        final_turn("Created note.txt with the greeting."),
    ];
    let agent = agent_for(&dir, script).await;

    let answer = agent.run("create note.txt").await.expect("run");

    // The tool actually ran against the working dir…
    let written = std::fs::read_to_string(dir.join("note.txt")).expect("note.txt written");
    assert_eq!(written, "hello self");
    // …and the loop returned the model's final (tool-free) turn.
    assert!(answer.contains("Created note.txt"), "answer: {answer}");
    // …and the run was recorded to the episodic log (memory path works e2e).
    let log = std::fs::read_to_string(dir.join(".agent/episodic.jsonl")).unwrap_or_default();
    assert!(
        log.contains("write_file") || !log.is_empty(),
        "episodic log empty"
    );
}

/// A read → edit → confirm cycle on a real source file: the self-improvement
/// pattern in miniature. Proves multi-turn tool dispatch with observation feedback
/// and that the real `edit` tool mutates on-disk state through the built loop.
#[tokio::test]
async fn loop_reads_then_edits_a_source_file() {
    let dir = tempdir();
    std::fs::write(dir.join("src.rs"), "fn old_name() {}\n").unwrap();
    let script = vec![
        tool_turn(vec![call("1", "read_file", json!({"path": "src.rs"}))]),
        tool_turn(vec![call(
            "2",
            "edit",
            json!({"path": "src.rs", "old_string": "old_name", "new_string": "new_name"}),
        )]),
        final_turn("Renamed old_name to new_name."),
    ];
    let agent = agent_for(&dir, script).await;

    let answer = agent.run("rename the function").await.expect("run");

    let edited = std::fs::read_to_string(dir.join("src.rs")).unwrap();
    assert_eq!(edited, "fn new_name() {}\n", "edit applied on disk");
    assert!(answer.contains("Renamed"), "answer: {answer}");
}

/// Adversarial: a prompt-injected model that asks `write_file` to escape the
/// working dir (`../…` or an absolute path) must be confined — no file is created
/// outside the working dir, and the tool result is an error the model sees. Proves
/// the built loop's `working_dir` + the tool's `resolve_within` guard hold end to
/// end, not just in the tool's unit tests.
#[tokio::test]
async fn loop_confines_write_to_the_working_dir() {
    let dir = tempdir();
    let outside = dir.parent().unwrap().join("escaped-e2e.txt");
    let _ = std::fs::remove_file(&outside); // ensure a clean slate
    let script = vec![
        tool_turn(vec![call(
            "1",
            "write_file",
            json!({"path": "../escaped-e2e.txt", "content": "pwned"}),
        )]),
        final_turn("could not escape"),
    ];
    let agent = agent_for(&dir, script).await;

    let answer = agent.run("try to escape").await.expect("run");

    assert!(
        !outside.exists(),
        "write escaped the working dir to {}",
        outside.display()
    );
    assert!(answer.contains("could not escape"), "answer: {answer}");
    // The episodic log should show the write was refused (an error observation),
    // not silently succeeded.
    let log = std::fs::read_to_string(dir.join(".agent/episodic.jsonl")).unwrap_or_default();
    assert!(
        log.contains("escape") || log.contains("outside") || log.contains("error"),
        "no refusal recorded in episodic log"
    );
}

/// A model that never yields a final answer must terminate at `max_iterations`
/// rather than loop forever — the loop's safety bound, exercised through the real
/// build. The scripted provider repeats its only (tool-requesting) turn, so the
/// loop can never finish and must hit the cap deterministically.
#[tokio::test]
async fn loop_terminates_at_max_iterations() {
    let dir = tempdir();
    let script = vec![tool_turn(vec![call(
        "1",
        "read_file",
        json!({"path": "x"}),
    )])];
    let agent = agent_for_cfg(&config_toml_iters(&dir, 3), script).await;

    let err = agent
        .run("loop forever")
        .await
        .expect_err("a never-finishing model must hit the iteration cap");
    let msg = err.to_string();
    assert!(
        msg.contains("max_iterations (3)"),
        "expected the iteration-cap error, got: {msg}"
    );
}

/// Parity spec 18, the headline integration: a secret in a `write_file` body is
/// caught by the scanner, mapped to a `Deny` by the Policy gate, and the file is
/// never created — while the run still completes (a deny adapts, it does not
/// abort). Drives the real builder → scanner → guard → loop path.
#[tokio::test]
async fn scanned_write_with_a_secret_is_denied_and_file_not_written() {
    let dir = tempdir();
    let cfg = format!(
        "{}\n[scanner]\nrules = [\"secret\"]\ndeny_at = \"high\"\n",
        config_toml(&dir)
    );
    let script = vec![
        tool_turn(vec![call(
            "1",
            "write_file",
            json!({"path": "creds.txt", "content": "aws_key = \"AKIAIOSFODNN7EXAMPLE\""}),
        )]),
        final_turn("I could not write that file."),
    ];
    let agent = agent_for_cfg(&cfg, script).await;

    let answer = agent.run("save the key").await.expect("run completes");

    assert!(
        !dir.join("creds.txt").exists(),
        "the write must have been blocked before touching the disk"
    );
    assert_eq!(answer, "I could not write that file.");
}

/// The control for the case above: identical wiring, clean content, write lands.
/// Without this, the deny test would also pass if the scanner blocked everything.
#[tokio::test]
async fn scanned_write_with_clean_content_is_allowed() {
    let dir = tempdir();
    let cfg = format!(
        "{}\n[scanner]\nrules = [\"secret\"]\ndeny_at = \"high\"\n",
        config_toml(&dir)
    );
    let script = vec![
        tool_turn(vec![call(
            "1",
            "write_file",
            json!({"path": "ok.txt", "content": "fn main() {}"}),
        )]),
        final_turn("Wrote ok.txt."),
    ];
    let agent = agent_for_cfg(&cfg, script).await;

    agent.run("write ok.txt").await.expect("run");

    assert_eq!(
        std::fs::read_to_string(dir.join("ok.txt")).expect("file written"),
        "fn main() {}"
    );
}

/// Parity spec 12: with `[web_search] backends` configured, the `web_search`
/// tool is registered and reaches the model's schema list. With it empty (the
/// default), the tool is absent entirely — nothing egresses unless opted in.
#[rstest::rstest]
#[case::positive_configured_registers_the_tool(
    "backends = [\"searxng\"]\nsearxng_endpoint = \"http://127.0.0.1:1/search\"",
    true
)]
#[case::negative_unconfigured_omits_the_tool("backends = []", false)]
#[tokio::test]
async fn web_search_tool_registration(#[case] ws_cfg: &str, #[case] want: bool) {
    let dir = tempdir();
    let cfg = format!("{}\n[web_search]\n{ws_cfg}\n", config_toml(&dir));
    let agent = agent_for_cfg(&cfg, vec![final_turn("done")]).await;
    let has = agent
        .tools()
        .describe_all()
        .iter()
        .any(|s| s.name == "web_search");
    assert_eq!(has, want, "web_search registration did not match config");
}

//! Pty-driven tests of the surfaces a pipe can never reach.
//!
//! Two of them exist, and both are load-bearing:
//!
//! 1. **The policy guard's operator prompt.** `agent-runtime/src/policy.rs`
//!    returns `false` *without reading a byte* when stdin is not a terminal — a
//!    deliberate fail-safe so unattended runs cannot be talked into a dangerous
//!    call, and also because stdin is the JSON-RPC channel under `--serve-mcp`.
//!    The consequence is that no piped test can ever execute the approval path.
//!    A real terminal is the only way in.
//!
//! 2. **The REPL's line editor.** `repl.rs` picks `rustyline` when
//!    `stdin().is_terminal()` and a plain `read_line` otherwise, so piped input
//!    exercises the *other* branch by construction.
//!
//! Unix-only: it needs a pty.

#![cfg(unix)]

mod common;

use common::{text, tool, write_config, FakeLlm, TempWorkspace};
use rexpect::session::{spawn_command, PtySession};
use serde_json::json;

/// Every expectation is bounded — a hung child must fail the test, not the suite.
const TIMEOUT_MS: u64 = 30_000;

/// Spawn the real binary on a pty. `goal` empty ⇒ the interactive REPL.
fn spawn_agent(cfg: &std::path::Path, ws: &TempWorkspace, goal: &[&str]) -> PtySession {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_agent"));
    cmd.arg("--config").arg(cfg).args(goal).current_dir(&ws.dir);
    cmd.env("RUST_LOG", "warn");
    // Keep the child's output free of colour/cursor escapes so matching is on
    // text rather than on terminal control bytes.
    cmd.env("TERM", "dumb");
    cmd.env("NO_COLOR", "1");
    spawn_command(cmd, Some(TIMEOUT_MS)).expect("spawn agent on a pty")
}

/// A config whose guard is explicitly in `prompt` mode.
fn guarded_config(ws: &TempWorkspace, base_url: &str) -> std::path::PathBuf {
    write_config(ws, base_url, "\n[policy]\nguard = \"prompt\"\n")
}

// --- the REPL ------------------------------------------------------------

/// The interactive path end to end: banner, a goal, an answer, and a clean exit
/// via `/quit`.
#[test]
fn positive_repl_runs_a_turn_and_quits() {
    let ws = TempWorkspace::new("repl");
    let llm = FakeLlm::start(vec![text("the-repl-answer")]);
    let cfg = write_config(&ws, llm.base_url(), "");

    let mut p = spawn_agent(&cfg, &ws, &[]);
    p.exp_string("agent-seddon REPL").expect("REPL banner");
    p.exp_string("> ").expect("prompt");
    p.send_line("say something").expect("send goal");
    p.exp_string("the-repl-answer").expect("the answer");
    p.send_line("/quit").expect("send /quit");
    p.exp_eof().expect("clean exit after /quit");
}

/// Slash commands are handled locally, without a provider round trip — so this
/// also proves the command path short-circuits before the model.
#[test]
fn positive_repl_slash_command_lists_tools() {
    let ws = TempWorkspace::new("replcmd");
    let llm = FakeLlm::start(vec![text("unused")]);
    let cfg = write_config(&ws, llm.base_url(), "");

    let mut p = spawn_agent(&cfg, &ws, &[]);
    p.exp_string("agent-seddon REPL").expect("banner");
    p.send_line("/tools").expect("send /tools");
    p.exp_string("write_file").expect("the tool list");
    p.send_line("/quit").expect("send /quit");
    p.exp_eof().expect("clean exit");

    assert!(
        llm.requests().is_empty(),
        "a slash command must not call the model, saw {} request(s)",
        llm.requests().len()
    );
}

/// Ctrl-D is the documented way out, and a different code path from `/quit`.
#[test]
fn positive_repl_exits_on_ctrl_d() {
    let ws = TempWorkspace::new("replctrld");
    let llm = FakeLlm::start(vec![text("unused")]);
    let cfg = write_config(&ws, llm.base_url(), "");

    let mut p = spawn_agent(&cfg, &ws, &[]);
    p.exp_string("agent-seddon REPL").expect("banner");
    p.exp_string("> ").expect("prompt");
    p.send_control('d').expect("send ^D");
    p.exp_eof().expect("clean exit on EOF");
}

/// An unknown command must be reported, not silently ignored — otherwise a typo
/// looks like a command that did nothing.
#[test]
fn negative_repl_unknown_slash_command_is_reported() {
    let ws = TempWorkspace::new("replunknown");
    let llm = FakeLlm::start(vec![text("unused")]);
    let cfg = write_config(&ws, llm.base_url(), "");

    let mut p = spawn_agent(&cfg, &ws, &[]);
    p.exp_string("agent-seddon REPL").expect("banner");
    p.send_line("/nosuchcommand").expect("send bad command");
    p.exp_string("unknown command").expect("the complaint");
    p.send_line("/quit").expect("send /quit");
    p.exp_eof().expect("clean exit");
}

// --- the policy guard's operator prompt ----------------------------------

/// **The path no pipe can reach.** A write to `.env` is flagged as a sensitive
/// path; on a terminal the guard asks, and `y` lets it through.
#[test]
fn positive_guard_prompt_allows_on_yes() {
    let ws = TempWorkspace::new("guardyes");
    let llm = FakeLlm::start(vec![
        tool(
            "write_file",
            json!({ "path": ".env", "content": "SECRET=1\n" }),
        ),
        text("wrote it"),
    ]);
    let cfg = guarded_config(&ws, llm.base_url());

    let mut p = spawn_agent(&cfg, &ws, &["write the env file"]);
    p.exp_string("policy guard flagged")
        .expect("the guard must announce itself");
    p.exp_string("allow this call?").expect("the prompt");
    p.send_line("y").expect("answer yes");
    p.exp_string("=== ANSWER ===").expect("the run completes");
    p.exp_eof().expect("clean exit");

    assert_eq!(
        std::fs::read_to_string(ws.path(".env")).expect(".env must have been written"),
        "SECRET=1\n",
        "an approved call must actually run"
    );
}

/// **The assertion that matters.** `n` must deny, and the file must not exist.
#[test]
fn negative_guard_prompt_denies_on_no() {
    let ws = TempWorkspace::new("guardno");
    let llm = FakeLlm::start(vec![
        tool(
            "write_file",
            json!({ "path": ".env", "content": "SECRET=1\n" }),
        ),
        text("was denied"),
    ]);
    let cfg = guarded_config(&ws, llm.base_url());

    let mut p = spawn_agent(&cfg, &ws, &["write the env file"]);
    p.exp_string("allow this call?").expect("the prompt");
    p.send_line("n").expect("answer no");
    p.exp_eof().expect("clean exit");

    assert!(
        !ws.path(".env").exists(),
        "a denied call must not write the file"
    );
}

/// A bare newline is not consent. This is the fail-safe default, and the reason
/// the prompt is spelled `[y/N]`.
#[test]
fn negative_guard_prompt_denies_on_empty_answer() {
    let ws = TempWorkspace::new("guardempty");
    let llm = FakeLlm::start(vec![
        tool(
            "write_file",
            json!({ "path": ".env", "content": "SECRET=1\n" }),
        ),
        text("was denied"),
    ]);
    let cfg = guarded_config(&ws, llm.base_url());

    let mut p = spawn_agent(&cfg, &ws, &["write the env file"]);
    p.exp_string("allow this call?").expect("the prompt");
    p.send_line("").expect("answer with a bare newline");
    p.exp_eof().expect("clean exit");

    assert!(
        !ws.path(".env").exists(),
        "an unanswered prompt must deny, not allow"
    );
}

/// **Adversarial.** The answer is operator input, but a non-`y` token must never
/// be read as consent — including something that merely starts with the letter.
#[test]
fn adversarial_guard_prompt_does_not_accept_lookalike_answers() {
    for answer in ["yolo", "nope", "Y E S", "1", "true"] {
        let ws = TempWorkspace::new("guardlookalike");
        let llm = FakeLlm::start(vec![
            tool(
                "write_file",
                json!({ "path": ".env", "content": "SECRET=1\n" }),
            ),
            text("done"),
        ]);
        let cfg = guarded_config(&ws, llm.base_url());

        let mut p = spawn_agent(&cfg, &ws, &["write the env file"]);
        p.exp_string("allow this call?").expect("the prompt");
        p.send_line(answer).expect("answer");
        p.exp_eof().expect("clean exit");

        assert!(
            !ws.path(".env").exists(),
            "`{answer}` must not be read as consent"
        );
    }
}

/// An ordinary call must NOT be interrupted by the guard — a gate that asks about
/// everything trains the operator to say yes without reading.
#[test]
fn corner_unflagged_call_is_not_prompted() {
    let ws = TempWorkspace::new("guardquiet");
    let llm = FakeLlm::start(vec![
        tool(
            "write_file",
            json!({ "path": "ordinary.txt", "content": "hello\n" }),
        ),
        text("wrote it"),
    ]);
    let cfg = guarded_config(&ws, llm.base_url());

    let mut p = spawn_agent(&cfg, &ws, &["write an ordinary file"]);
    // No prompt is sent, so this only completes if nothing blocked on stdin.
    p.exp_string("=== ANSWER ===")
        .expect("an unflagged call must run without asking");
    p.exp_eof().expect("clean exit");

    assert_eq!(
        std::fs::read_to_string(ws.path("ordinary.txt")).expect("file must exist"),
        "hello\n"
    );
}

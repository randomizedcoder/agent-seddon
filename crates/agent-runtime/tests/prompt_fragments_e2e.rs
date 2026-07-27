//! End-to-end proof that situational system-prompt fragments (docs/design/prompts/)
//! reach the model. The loop selects the fragments whose tags match the current mode
//! and injects them as a leading system message; here we capture the exact
//! `CompletionRequest` the provider received and assert on its system messages.
//!
//! The default mode is `Other`, so a fragment under `prompts/modes/other/` is
//! selected with no classifier needed; a goal that classifies as `Review` (the
//! deterministic prefilter) selects `prompts/modes/review/` instead — exercising the
//! mode → fragment path. A mid-session switch then swaps the situational message in
//! place (`updated`) or drops it when the new mode has no fragment (`removed`). With
//! no `prompts` dir, nothing is injected and behaviour is byte-identical to today.
//!
//! Scope: these cover the *loop composition* — how the selected text becomes the
//! index-1 leading system message and how it tracks a mode switch. The resolver's own
//! `corner_`/`boundary_`/`adversarial_` selection cases live at the unit level in
//! `agent-context/src/system_fragments.rs`.

use agent_core::{CompletionResponse, LlmProvider, Role};
use agent_runtime::{build_agent_with, parse_config, register_builtins, Metrics, Registry};
use agent_testkit::{final_turn, tempdir};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// A hermetic config with every on-disk seam under `dir`; `prompts_dir` (when
/// non-empty) points the situational-fragment resolver at a fragment tree.
fn config_toml(dir: &Path, prompts_dir: &str) -> String {
    let d = dir.display();
    let prompts = if prompts_dir.is_empty() {
        String::new()
    } else {
        format!("[prompts]\ndir = \"{prompts_dir}\"\n")
    };
    format!(
        r#"
        [agent]
        provider = "capture"
        policy = "auto-approve"
        stream = false
        working_dir = "{d}"
        max_iterations = 4

        [provider]
        model = "capture-model"

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

        {prompts}
    "#
    )
}

/// A provider that records the system-message text of every request it sees, then
/// answers with a tool-free final turn (ending the loop immediately).
struct Capture {
    seen: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl LlmProvider for Capture {
    fn capabilities(&self) -> agent_core::ModelCapabilities {
        agent_core::ModelCapabilities {
            supports_tools: false,
            context_window: 4096,
            supports_response_format: false,
            supports_vision: false,
        }
    }
    async fn complete(
        &self,
        req: agent_core::CompletionRequest,
    ) -> agent_core::Result<CompletionResponse> {
        let mut sys = self.seen.lock().unwrap();
        for m in &req.messages {
            if m.role == Role::System {
                sys.push(m.content_text());
            }
        }
        Ok(final_turn("done"))
    }
}

/// Build an agent whose model is the capturing provider; returns the agent and the
/// shared buffer the provider writes each request's system messages into.
async fn agent_and_capture(
    cfg_toml: &str,
) -> (
    std::sync::Arc<agent_runtime::Agent>,
    Arc<Mutex<Vec<String>>>,
) {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let cfg = parse_config(cfg_toml).expect("parse config");
    let mut registry = Registry::new();
    register_builtins(&mut registry);
    let seen_for_factory = seen.clone();
    registry.provider("capture", move |_ctx| {
        Ok(Arc::new(Capture {
            seen: seen_for_factory.clone(),
        }) as Arc<dyn LlmProvider>)
    });
    let agent = build_agent_with(&registry, cfg, None, "frag-e2e".into(), Metrics::new())
        .await
        .expect("build agent");
    (agent, seen)
}

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// The joined system text the model saw across the run.
fn system_text(seen: &Arc<Mutex<Vec<String>>>) -> String {
    seen.lock().unwrap().join("\n---\n")
}

/// The `n`th system message the model saw (in message order), or `""` if there are
/// fewer than `n+1`. With the loop's placement `messages[0]` is the base head and
/// `messages[1]` is the situational fragment, so `nth_system(&seen, 1)` is the
/// leading situational message — used to prove the index-1 invariant, not just
/// presence somewhere in the request.
fn nth_system(seen: &Arc<Mutex<Vec<String>>>, n: usize) -> String {
    seen.lock().unwrap().get(n).cloned().unwrap_or_default()
}

/// Drop everything captured so far, so the next turn's request can be inspected in
/// isolation (each `send` is exactly one `complete`, hence one request, since the
/// provider answers with a tool-free final turn).
fn clear(seen: &Arc<Mutex<Vec<String>>>) {
    seen.lock().unwrap().clear();
}

/// positive: a `modes/other/` fragment is injected for the default (`Other`) mode.
#[tokio::test]
async fn positive_other_mode_fragment_injected() {
    let dir = tempdir();
    let prompts = dir.join("prompts");
    write(
        &prompts.join("modes/other/0001_note.md"),
        "SITU_OTHER_MARKER: stay concise.\n",
    );
    let (agent, seen) = agent_and_capture(&config_toml(&dir, prompts.to_str().unwrap())).await;

    agent.run("please say hello").await.expect("run");

    assert!(
        system_text(&seen).contains("SITU_OTHER_MARKER"),
        "the Other-mode fragment should reach the model; saw: {}",
        system_text(&seen)
    );
}

/// positive: a goal that classifies as `Review` selects the `modes/review/` fragment
/// (the mode → fragment path), not the `Other` one.
#[tokio::test]
async fn positive_review_goal_selects_review_fragment() {
    let dir = tempdir();
    let prompts = dir.join("prompts");
    write(
        &prompts.join("modes/other/0001_note.md"),
        "SITU_OTHER_MARKER\n",
    );
    write(
        &prompts.join("modes/review/0001_focus.md"),
        "SITU_REVIEW_MARKER: ground every comment.\n",
    );
    let (agent, seen) = agent_and_capture(&config_toml(&dir, prompts.to_str().unwrap())).await;

    // A deterministic review cue → the classifier switches to Review on turn 1.
    agent
        .run("please do a code review of this change")
        .await
        .expect("run");

    let text = system_text(&seen);
    assert!(
        text.contains("SITU_REVIEW_MARKER"),
        "the Review fragment should be selected; saw: {text}"
    );
    assert!(
        !text.contains("SITU_OTHER_MARKER"),
        "the Other fragment must not be selected in Review mode; saw: {text}"
    );
}

/// negative: with no `prompts` dir, nothing is injected — behaviour is unchanged.
#[tokio::test]
async fn negative_no_prompts_dir_injects_nothing() {
    let dir = tempdir();
    let (agent, seen) = agent_and_capture(&config_toml(&dir, "")).await;

    agent.run("please say hello").await.expect("run");

    let text = system_text(&seen);
    assert!(
        !text.is_empty(),
        "the base system prompt should still be sent"
    );
    assert!(
        !text.contains("SITU_"),
        "no situational fragment should appear without a prompts dir"
    );
}

/// positive: a mid-session mode switch swaps the situational message *in place* — the
/// `updated` branch. Turn 1 (Other) injects the Other fragment at index 1; turn 2 (a
/// review cue) switches to Review and replaces index 1 with the Review fragment.
#[tokio::test]
async fn positive_switch_updates_situational_message() {
    let dir = tempdir();
    let prompts = dir.join("prompts");
    write(
        &prompts.join("modes/other/0001_note.md"),
        "SITU_OTHER_MARKER\n",
    );
    write(
        &prompts.join("modes/review/0001_focus.md"),
        "SITU_REVIEW_MARKER: ground every comment.\n",
    );
    let (agent, seen) = agent_and_capture(&config_toml(&dir, prompts.to_str().unwrap())).await;
    let mut session = agent.session();

    // Turn 1: default Other mode → the Other fragment is the leading situational
    // message (right after the base head).
    session.send("please say hello").await.expect("turn 1");
    assert!(
        nth_system(&seen, 1).contains("SITU_OTHER_MARKER"),
        "turn 1 should place the Other fragment at index 1; saw: {}",
        system_text(&seen)
    );

    // Turn 2: a review cue switches the mode, so the situational message is swapped
    // in place for the Review fragment.
    clear(&seen);
    session
        .send("please do a code review of this change")
        .await
        .expect("turn 2");
    let text = system_text(&seen);
    assert!(
        !text.contains("SITU_OTHER_MARKER"),
        "the Other fragment must be gone after the switch; saw: {text}"
    );
    assert!(
        nth_system(&seen, 1).contains("SITU_REVIEW_MARKER"),
        "the Review fragment must be swapped in at index 1 (leading); saw: {text}"
    );
}

/// corner: a mid-session switch to a mode with no fragment *removes* the situational
/// message — the `removed` branch. Only an Other fragment exists, so switching to
/// Review (which has none) drops the message and leaves just the base head.
#[tokio::test]
async fn corner_switch_removes_situational_message() {
    let dir = tempdir();
    let prompts = dir.join("prompts");
    write(
        &prompts.join("modes/other/0001_note.md"),
        "SITU_OTHER_MARKER\n",
    );
    let (agent, seen) = agent_and_capture(&config_toml(&dir, prompts.to_str().unwrap())).await;
    let mut session = agent.session();

    session.send("please say hello").await.expect("turn 1");
    assert!(
        nth_system(&seen, 1).contains("SITU_OTHER_MARKER"),
        "turn 1 should inject the Other fragment; saw: {}",
        system_text(&seen)
    );

    // Switch to Review, which has no fragment → the situational message is removed.
    clear(&seen);
    session
        .send("please do a code review of this change")
        .await
        .expect("turn 2");
    let text = system_text(&seen);
    assert!(
        !text.is_empty(),
        "the base system prompt should still be sent after removal"
    );
    assert!(
        !text.contains("SITU_"),
        "no situational fragment should remain after switching to a mode with none; saw: {text}"
    );
}

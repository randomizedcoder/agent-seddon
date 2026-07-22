//! End-to-end tests of the shipped `agent` BINARY, driven as a subprocess against
//! a scripted OpenAI-compatible server.
//!
//! `agent-runtime/tests/loop_e2e.rs` already covers the loop thoroughly — but as a
//! *library*, through `build_agent_with`. That leaves a whole tier untested:
//! argv parsing, config loading, the stdout contract, exit codes, and the real
//! HTTP request/response serialization. Every bug in that tier is invisible to an
//! in-process test and visible to every user on their first run.
//!
//! Nothing here touches the network: the "provider" is a `tiny_http` server on an
//! ephemeral loopback port, the same idiom as `agent-web-search/tests/http_e2e.rs`.

mod common;

use common::{run_agent, status, text, tool, write_config, FakeLlm, TempWorkspace};
use serde_json::json;

/// The headline: a goal goes in, a real tool call comes out, a file lands on
/// disk, the answer is on stdout, and the process exits 0.
#[test]
fn positive_writes_a_c_hello_world_and_exits_zero() {
    let ws = TempWorkspace::new("hello");
    let llm = FakeLlm::start(vec![
        tool(
            "write_file",
            json!({
                "path": "hello.c",
                "content": "#include <stdio.h>\n\nint main(void) {\n    printf(\"Hello, World!\\n\");\n    return 0;\n}\n"
            }),
        ),
        text("Wrote hello.c."),
    ]);
    let cfg = write_config(&ws, llm.base_url(), "");

    let (code, stdout, stderr) = run_agent(
        &cfg,
        &ws,
        &["write a hello world program in C called hello.c"],
    );

    assert_eq!(code, 0, "exit code; stderr:\n{stderr}");
    assert!(
        stdout.contains("=== ANSWER ==="),
        "stdout must carry the answer banner, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Wrote hello.c."),
        "stdout must carry the final answer, got:\n{stdout}"
    );

    let written = std::fs::read_to_string(ws.path("hello.c")).expect("hello.c must exist");
    assert!(
        written.contains("int main(void)") && written.contains("Hello, World!"),
        "file content survived the wire intact, got:\n{written}"
    );
    // The escape handling is the subtle part: a real provider sends `arguments`
    // as a JSON *string*, so the C source is escaped twice on the way in. A bug
    // there yields a file with literal `n` where the newlines should be — which
    // still "succeeds" and still writes a file. Observed for real from a local
    // llama3.1, so this is not hypothetical.
    assert!(
        written.lines().count() >= 5,
        "newlines must survive double-escaping, got a {}-line file:\n{written}",
        written.lines().count()
    );
    // Note the `\n` INSIDE the printf is legitimate C source and must survive as
    // two characters; only the line separators become real newlines. Asserting
    // "no backslash-n anywhere" would be wrong, so check the specific corruption:
    // a structural newline that arrived as the letter `n`.
    assert!(
        !written.contains("stdio.h>n") && !written.contains(";n"),
        "structural newlines were flattened to the letter `n`, got:\n{written}"
    );
    assert!(
        written.contains(r#"printf("Hello, World!\n")"#),
        "the escaped \\n inside the C string literal must survive as two chars, got:\n{written}"
    );
}

/// What the loop SENDS is half the contract, and the half no in-process double
/// can check. A malformed request is a bug every openai-compat user hits.
#[test]
fn positive_request_is_well_formed_openai_compat() {
    let ws = TempWorkspace::new("req");
    let llm = FakeLlm::start(vec![text("ok")]);
    let cfg = write_config(&ws, llm.base_url(), "");

    let (code, _out, err) = run_agent(&cfg, &ws, &["say ok"]);
    assert_eq!(code, 0, "stderr:\n{err}");

    let reqs = llm.requests();
    assert!(!reqs.is_empty(), "the server received no request at all");
    let r = &reqs[0];

    assert_eq!(r["model"], "test-model", "model name must reach the wire");
    assert!(
        r["messages"].as_array().is_some_and(|m| !m.is_empty()),
        "messages must be a non-empty array, got: {r}"
    );
    assert!(
        r["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "system"),
        "the system prompt must be sent, got: {}",
        r["messages"]
    );
    assert!(
        r["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "user"),
        "the goal must be sent as a user message, got: {}",
        r["messages"]
    );

    // Tool schemas must be advertised, or the model cannot act at all.
    let tools = r["tools"].as_array().expect("tools array must be present");
    assert!(!tools.is_empty(), "tools must not be empty");
    assert!(
        tools
            .iter()
            .any(|t| t["function"]["name"] == "write_file" && t["type"] == "function"),
        "write_file must be advertised in OpenAI function shape, got: {r}"
    );
    assert!(
        tools
            .iter()
            .all(|t| t["function"]["parameters"].is_object()),
        "every tool needs a parameters schema object, got: {r}"
    );
}

/// A tool result must be fed back in the shape the API requires, or the model is
/// answering a question it cannot see the result of.
#[test]
fn positive_tool_result_is_returned_to_the_model() {
    let ws = TempWorkspace::new("toolresult");
    std::fs::write(ws.path("data.txt"), "the-sentinel-value").unwrap();
    let llm = FakeLlm::start(vec![
        tool("read_file", json!({ "path": "data.txt" })),
        text("read it"),
    ]);
    let cfg = write_config(&ws, llm.base_url(), "");

    let (code, _out, err) = run_agent(&cfg, &ws, &["read data.txt"]);
    assert_eq!(code, 0, "stderr:\n{err}");

    let reqs = llm.requests();
    assert!(
        reqs.len() >= 2,
        "expected a second turn carrying the tool result, got {} request(s)",
        reqs.len()
    );
    let followup = reqs[1]["messages"].to_string();
    assert!(
        followup.contains("the-sentinel-value"),
        "the tool's output must reach the model, got:\n{followup}"
    );
    assert!(
        followup.contains("\"tool\""),
        "the result must use the `tool` role, got:\n{followup}"
    );
}

/// Streaming is the shipped default (`[agent] stream = true`), and it is a
/// completely separate parser from the buffered path — including how tool-call
/// argument fragments are accumulated.
#[test]
fn positive_streaming_path_also_executes_tools() {
    let ws = TempWorkspace::new("stream");
    let llm = FakeLlm::start(vec![
        tool(
            "write_file",
            json!({ "path": "streamed.txt", "content": "via sse\n" }),
        ),
        text("streamed"),
    ]);
    let cfg = write_config(&ws, llm.base_url(), "");
    // Flip on the SSE path that the buffered tests above bypass.
    let raw = std::fs::read_to_string(&cfg).unwrap();
    std::fs::write(&cfg, raw.replace("stream = false", "stream = true")).unwrap();

    let (code, _out, err) = run_agent(&cfg, &ws, &["write streamed.txt"]);
    assert_eq!(code, 0, "stderr:\n{err}");
    assert_eq!(
        std::fs::read_to_string(ws.path("streamed.txt")).expect("streamed.txt must exist"),
        "via sse\n"
    );
}

/// A dead provider must fail loudly: nonzero exit, diagnosis on stderr, and
/// **nothing** on stdout — a scripted caller reads stdout as the answer.
#[test]
fn negative_unreachable_provider_exits_nonzero_with_clean_stdout() {
    let ws = TempWorkspace::new("dead");
    // Port 1 is reserved and never listening.
    let cfg = write_config(&ws, "http://127.0.0.1:1/v1", "");

    let (code, stdout, stderr) = run_agent(&cfg, &ws, &["anything"]);

    assert_ne!(code, 0, "a dead provider must not report success");
    assert!(
        !stdout.contains("=== ANSWER ==="),
        "no answer banner may be printed on failure, got:\n{stdout}"
    );
    assert!(
        stderr.to_lowercase().contains("error"),
        "the failure must be diagnosable from stderr, got:\n{stderr}"
    );
}

/// A provider HTTP error is not a crash and not a success.
#[test]
fn negative_provider_http_error_exits_nonzero() {
    let ws = TempWorkspace::new("http500");
    let llm = FakeLlm::start(vec![status(500, "upstream exploded")]);
    let cfg = write_config(&ws, llm.base_url(), "");

    let (code, stdout, _err) = run_agent(&cfg, &ws, &["anything"]);
    assert_ne!(code, 0, "an HTTP 500 must not report success");
    assert!(!stdout.contains("=== ANSWER ==="), "got:\n{stdout}");
}

/// `--help` is the one path that must work with no config and no model.
#[test]
fn corner_help_exits_zero_on_stdout() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_agent"))
        .arg("--help")
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("--config") && stdout.contains("--serve-mcp"),
        "usage must document the flags, got:\n{stdout}"
    );
}

/// A goal is positional and joined with spaces, so an unquoted multi-word goal
/// behaves the same as a quoted one.
#[test]
fn corner_multi_word_goal_is_joined() {
    let ws = TempWorkspace::new("multiword");
    let llm = FakeLlm::start(vec![text("ok")]);
    let cfg = write_config(&ws, llm.base_url(), "");

    let (code, _out, err) = run_agent(&cfg, &ws, &["say", "the", "words"]);
    assert_eq!(code, 0, "stderr:\n{err}");

    let reqs = llm.requests();
    assert!(
        reqs[0]["messages"].to_string().contains("say the words"),
        "positional words must join with single spaces, got:\n{}",
        reqs[0]["messages"]
    );
}

/// The loop must stop at `max_iterations` even when the model never stops asking
/// for tools — otherwise a looping model burns tokens forever.
///
/// Exhausting the budget is a **failure** exit, not a quiet success: the run
/// produced no final answer, so a scripted caller must be able to tell it apart
/// from a completed one.
#[test]
fn boundary_max_iterations_terminates_the_process() {
    let ws = TempWorkspace::new("maxiter");
    // Every scripted turn is a tool call, and the script is long enough that the
    // iteration cap, not the script, is what ends the run.
    let script = (0..20)
        .map(|i| {
            tool(
                "write_file",
                json!({ "path": format!("f{i}.txt"), "content": "x" }),
            )
        })
        .collect();
    let llm = FakeLlm::start_with(script, "never reached");
    let cfg = write_config(&ws, llm.base_url(), "");

    let (code, stdout, err) = run_agent(&cfg, &ws, &["loop forever"]);

    assert_ne!(code, 0, "an unfinished run must not report success");
    assert!(
        err.contains("max_iterations"),
        "the reason must be diagnosable from stderr, got:\n{err}"
    );
    assert!(
        !stdout.contains("=== ANSWER ==="),
        "no answer was produced, so none may be printed, got:\n{stdout}"
    );
    let calls = llm.requests().len();
    assert!(
        calls <= 7,
        "max_iterations = 6 must bound the provider calls, saw {calls}"
    );
}

/// An empty tool-call argument object must not panic the loop.
#[test]
fn boundary_empty_tool_arguments_are_handled() {
    let ws = TempWorkspace::new("emptyargs");
    let llm = FakeLlm::start(vec![tool("ls", json!({})), text("listed")]);
    let cfg = write_config(&ws, llm.base_url(), "");

    let (code, stdout, err) = run_agent(&cfg, &ws, &["list files"]);
    assert_eq!(code, 0, "stderr:\n{err}");
    assert!(stdout.contains("listed"), "got:\n{stdout}");
}

/// **Adversarial.** Tool arguments are attacker-controlled: the model is
/// prompt-injectable, so a `path` is hostile input. A traversal escape must be
/// refused and must not write outside the working dir.
#[test]
fn adversarial_path_traversal_is_refused_and_writes_nothing_outside() {
    let ws = TempWorkspace::new("traversal");
    let outside = ws.dir.parent().unwrap().join("escaped-marker.txt");
    let _ = std::fs::remove_file(&outside);

    let llm = FakeLlm::start(vec![
        tool(
            "write_file",
            json!({ "path": "../escaped-marker.txt", "content": "pwned" }),
        ),
        text("attempted"),
    ]);
    let cfg = write_config(&ws, llm.base_url(), "");

    let (_code, _out, _err) = run_agent(&cfg, &ws, &["escape the sandbox"]);

    assert!(
        !outside.exists(),
        "traversal escaped the working dir and wrote {}",
        outside.display()
    );
    // The refusal must reach the model as an error observation rather than
    // silently doing nothing — otherwise the model believes the write happened.
    let reqs = llm.requests();
    assert!(reqs.len() >= 2, "expected a turn carrying the tool result");
    let msgs = reqs[1]["messages"].as_array().expect("messages array");
    let result = msgs
        .iter()
        .find(|m| m["role"] == "tool")
        .expect("a tool-role result message must be sent back");
    let body = result["content"].as_str().unwrap_or_default();
    assert!(
        body.contains("escapes the working directory"),
        "the refusal must be visible to the model, and say why, got:\n{body}"
    );
    // Only the *result* is checked here — the assistant's own `tool_calls` are
    // replayed as conversation history and legitimately still contain what it
    // asked for.
    assert!(
        !body.contains("pwned"),
        "the refused payload must not be echoed back in the result, got:\n{body}"
    );
}

/// **Adversarial.** An absolute path outside the workspace is the other half of
/// the same escape.
#[test]
fn adversarial_absolute_path_outside_workspace_is_refused() {
    let ws = TempWorkspace::new("abspath");
    let target = std::env::temp_dir().join("agent-cli-e2e-absolute-escape.txt");
    let _ = std::fs::remove_file(&target);

    let llm = FakeLlm::start(vec![
        tool(
            "write_file",
            json!({ "path": target.to_string_lossy(), "content": "pwned" }),
        ),
        text("attempted"),
    ]);
    let cfg = write_config(&ws, llm.base_url(), "");

    let (_code, _out, _err) = run_agent(&cfg, &ws, &["write outside"]);

    assert!(
        !target.exists(),
        "absolute path escaped the working dir: {}",
        target.display()
    );
}

/// **Adversarial.** A hostile `content` must be written as data, never
/// interpreted — and the double JSON-string encoding of `arguments` is exactly
/// where a quoting bug would turn content into structure.
#[test]
fn adversarial_quote_heavy_content_survives_as_data() {
    let ws = TempWorkspace::new("quotes");
    let nasty = "\"}}],\"injected\":true,\"x\":[{\"\\n\\\\\" -- ';DROP TABLE t;--\n";
    let llm = FakeLlm::start(vec![
        tool(
            "write_file",
            json!({ "path": "nasty.txt", "content": nasty }),
        ),
        text("wrote it"),
    ]);
    let cfg = write_config(&ws, llm.base_url(), "");

    let (code, _out, err) = run_agent(&cfg, &ws, &["write nasty content"]);
    assert_eq!(code, 0, "stderr:\n{err}");
    assert_eq!(
        std::fs::read_to_string(ws.path("nasty.txt")).expect("nasty.txt must exist"),
        nasty,
        "quote-heavy content must round-trip byte for byte"
    );
}

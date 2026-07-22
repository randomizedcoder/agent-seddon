//! Shared fixtures for the CLI end-to-end tests: a scripted OpenAI-compatible
//! server, and a config generator that keeps every run inside a tempdir.
//!
//! Why a fake *server* rather than the `ScriptedProvider` double: that double
//! substitutes at the `LlmProvider` trait boundary, so it hands the loop a
//! `CompletionResponse` that never crossed a wire. Nothing else in the workspace
//! serializes a real `/chat/completions` request or parses a real
//! `choices[].message.tool_calls` — which makes the openai-compat path, the one
//! config every user touches, the one path with no end-to-end coverage. This
//! closes that: the request is built, sent, received, and answered for real.

#![allow(dead_code)] // each test file uses a subset

use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Response, Server};

/// One scripted assistant turn.
#[derive(Clone, Debug)]
pub enum Reply {
    /// A final text answer — ends the loop.
    Text(String),
    /// A tool call. `args` is the object the tool receives.
    Tool { name: String, args: Value },
    /// A raw HTTP status with a body, for failure injection.
    Status(u16, String),
}

pub fn text(s: &str) -> Reply {
    Reply::Text(s.to_string())
}

pub fn tool(name: &str, args: Value) -> Reply {
    Reply::Tool {
        name: name.to_string(),
        args,
    }
}

pub fn status(code: u16, body: &str) -> Reply {
    Reply::Status(code, body.to_string())
}

/// A scripted OpenAI-compatible `/chat/completions` server on an ephemeral
/// loopback port. Replies are consumed in order; once the script is exhausted it
/// answers with `exhausted_text` forever, so a test can never hang waiting for a
/// turn it forgot to script (the loop terminates on a text reply).
pub struct FakeLlm {
    base_url: String,
    requests: Arc<Mutex<Vec<Value>>>,
    server: Arc<Server>,
}

impl FakeLlm {
    pub fn start(script: Vec<Reply>) -> Self {
        Self::start_with(script, "done")
    }

    pub fn start_with(script: Vec<Reply>, exhausted_text: &str) -> Self {
        let server = Arc::new(Server::http("127.0.0.1:0").expect("bind fake llm"));
        let port = server.server_addr().to_ip().expect("ip addr").port();
        let requests = Arc::new(Mutex::new(Vec::new()));

        let srv = server.clone();
        let reqs = requests.clone();
        let exhausted = exhausted_text.to_string();
        std::thread::spawn(move || {
            let mut queue = script.into_iter();
            for mut request in srv.incoming_requests() {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                // Record the parsed request so a test can assert on what the loop
                // actually SENT, not merely on what came back.
                let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let streaming = parsed
                    .get("stream")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                reqs.lock().unwrap().push(parsed);

                let reply = queue
                    .next()
                    .unwrap_or_else(|| Reply::Text(exhausted.clone()));

                let _ = match reply {
                    Reply::Status(code, body) => {
                        request.respond(Response::from_string(body).with_status_code(code))
                    }
                    r if streaming => request.respond(
                        Response::from_string(sse_body(&r))
                            .with_header(header("Content-Type", "text/event-stream")),
                    ),
                    r => request.respond(
                        Response::from_string(buffered_body(&r))
                            .with_header(header("Content-Type", "application/json")),
                    ),
                };
            }
        });

        Self {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            requests,
            server,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Every request body the server received, in order.
    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for FakeLlm {
    fn drop(&mut self) {
        // Release the accept loop so the thread exits with the test.
        self.server.unblock();
    }
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("valid header")
}

/// A non-streaming `/chat/completions` response body.
fn buffered_body(reply: &Reply) -> String {
    let message = match reply {
        Reply::Text(t) => json!({ "role": "assistant", "content": t }),
        Reply::Tool { name, args } => json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                // `arguments` is a JSON *string*, as the real API sends it —
                // getting this wrong is exactly the class of bug this fixture exists
                // to catch, so it is built the same awkward way the wire does it.
                "function": { "name": name, "arguments": args.to_string() }
            }]
        }),
        Reply::Status(..) => unreachable!("handled by the caller"),
    };
    json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": if matches!(reply, Reply::Tool { .. }) { "tool_calls" } else { "stop" }
        }],
        "usage": { "prompt_tokens": 11, "completion_tokens": 7, "total_tokens": 18 }
    })
    .to_string()
}

/// The same turn as a server-sent-event stream, terminated by `[DONE]`.
fn sse_body(reply: &Reply) -> String {
    let delta = match reply {
        Reply::Text(t) => json!({ "content": t }),
        Reply::Tool { name, args } => json!({
            "tool_calls": [{
                "index": 0,
                "id": "call_1",
                "type": "function",
                "function": { "name": name, "arguments": args.to_string() }
            }]
        }),
        Reply::Status(..) => unreachable!("handled by the caller"),
    };
    let finish = if matches!(reply, Reply::Tool { .. }) {
        "tool_calls"
    } else {
        "stop"
    };
    let chunk = json!({ "choices": [{ "index": 0, "delta": delta }] });
    let last = json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": finish }] });
    format!("data: {chunk}\n\ndata: {last}\n\ndata: [DONE]\n\n")
}

/// A tempdir that is removed when the test ends.
pub struct TempWorkspace {
    pub dir: std::path::PathBuf,
}

impl TempWorkspace {
    pub fn new(tag: &str) -> Self {
        let mut dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.push(format!("agent-cli-e2e-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).expect("create workspace");
        Self { dir }
    }

    pub fn path(&self, rel: &str) -> std::path::PathBuf {
        self.dir.join(rel)
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Write an `agent.toml` into `ws` pointed at `base_url`, with every on-disk seam
/// confined to the workspace. Returns the config path.
///
/// `[metrics] enabled = false` matters: the shipped config binds a fixed port,
/// and these tests run concurrently — a fixed port would make them fight.
pub fn write_config(ws: &TempWorkspace, base_url: &str, extra: &str) -> std::path::PathBuf {
    let cfg = ws.path("agent.toml");
    let dir = ws.dir.display();
    std::fs::write(
        &cfg,
        format!(
            r#"
[agent]
provider = "openai-compat"
context  = "sliding-window"
policy   = "auto-approve"
working_dir = "{dir}"
max_iterations = 6
max_tokens = 512
context_window = 8192
reserve_output = 512
stream = false
system_prompt = "test agent"

[provider]
base_url = "{base_url}"
model    = "test-model"
api_key  = "test-key"
max_retries = 0

[memory]
backend       = "file"
episodic_path = "{dir}/.agent/episodic.jsonl"
semantic_dir  = "{dir}/.agent/memory"

[tools]
enabled = ["read_file", "write_file", "edit", "ls", "grep", "find"]

[search]
auto_index = false

[metrics]
enabled = false

[git]
auto_fetch_secs = 0
{extra}
"#
        ),
    )
    .expect("write config");
    cfg
}

/// Run the real `agent` binary one-shot with `goal`, returning
/// `(exit_code, stdout, stderr)`.
pub fn run_agent(
    cfg: &std::path::Path,
    ws: &TempWorkspace,
    goal: &[&str],
) -> (i32, String, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_agent"))
        .arg("--config")
        .arg(cfg)
        .args(goal)
        .current_dir(&ws.dir)
        // Keep the child's log level predictable regardless of the developer's env.
        .env("RUST_LOG", "warn")
        .output()
        .expect("spawn agent binary");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

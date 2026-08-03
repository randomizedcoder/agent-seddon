//! Provider × protocol matrix, cassette-replayed over the real HTTP wire.
//!
//! `cli_e2e.rs` proves the shipped binary drives the **openai-compat** wire. This
//! adds the other axis: the same canonical tool-loop ("write hello.c") replayed
//! against BOTH the openai-compat `/chat/completions` shape and the Anthropic
//! `/messages` shape — so the Anthropic provider's request serialization and typed
//! `tool_use`/`tool_result` block handling are driven end to end through the
//! binary for the first time (the unit tests exercise the parser, not the wire).
//!
//! Record/replay: each protocol's turn-by-turn response bodies live in a committed
//! **cassette** under `tests/fixtures/cassettes/`. A `CassetteServer` replays them
//! in order over a loopback `tiny_http` port; the agent's `[provider] base_url`
//! points at it. A missing cassette is a hard failure (never a silent skip); refresh
//! them from a real endpoint with `nix run .#vcr-record`.

mod common;

use common::{run_agent, TempWorkspace};
use rstest::rstest;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Response, Server};

/// A loopback server that replays a cassette's response bodies in order — the
/// record/replay counterpart to `cli_e2e`'s scripted `FakeLlm`. It is protocol-
/// agnostic at the transport layer (only one provider dials it per test), so the
/// same server serves both the openai-compat and Anthropic cassettes; it records
/// each request so a test can assert on the wire shape the loop actually SENT.
struct CassetteServer {
    base_url: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    server: Arc<Server>,
}

struct RecordedRequest {
    path: String,
    body: Value,
}

impl CassetteServer {
    fn start(turns: Vec<Value>) -> Self {
        assert!(
            !turns.is_empty(),
            "cassette is empty — a missing/empty cassette must never silently pass; \
             refresh it with `nix run .#vcr-record`"
        );
        let server = Arc::new(Server::http("127.0.0.1:0").expect("bind cassette server"));
        let port = server.server_addr().to_ip().expect("ip addr").port();
        let requests = Arc::new(Mutex::new(Vec::new()));

        let srv = server.clone();
        let reqs = requests.clone();
        std::thread::spawn(move || {
            for (turn, mut request) in srv.incoming_requests().enumerate() {
                let path = request.url().to_string();
                let mut raw = String::new();
                let _ = request.as_reader().read_to_string(&mut raw);
                reqs.lock().unwrap().push(RecordedRequest {
                    path,
                    body: serde_json::from_str(&raw).unwrap_or(Value::Null),
                });
                // Replay the next cassette turn; the last repeats so an extra probe
                // (e.g. a title-gen call) can never hang the loop.
                let body = turns.get(turn).unwrap_or(turns.last().unwrap()).to_string();
                let _ = request.respond(
                    Response::from_string(body).with_header(
                        Header::from_bytes(b"Content-Type".as_ref(), b"application/json".as_ref())
                            .unwrap(),
                    ),
                );
            }
        });

        Self {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            requests,
            server,
        }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn requests(&self) -> std::sync::MutexGuard<'_, Vec<RecordedRequest>> {
        self.requests.lock().unwrap()
    }
}

impl Drop for CassetteServer {
    fn drop(&mut self) {
        self.server.unblock();
    }
}

#[derive(Clone, Copy)]
enum Protocol {
    OpenAiCompat,
    Anthropic,
}

impl Protocol {
    fn provider(self) -> &'static str {
        match self {
            Protocol::OpenAiCompat => "openai-compat",
            Protocol::Anthropic => "anthropic",
        }
    }
    fn cassette(self) -> &'static str {
        match self {
            Protocol::OpenAiCompat => "openai_compat.json",
            Protocol::Anthropic => "anthropic.json",
        }
    }
    /// The path suffix the provider POSTs to — proof the right wire was used.
    fn endpoint_suffix(self) -> &'static str {
        match self {
            Protocol::OpenAiCompat => "/chat/completions",
            Protocol::Anthropic => "/messages",
        }
    }
}

/// Load a committed cassette; a missing file is a hard error (fail-on-missing).
fn load_cassette(name: &str) -> Vec<Value> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cassettes")
        .join(name);
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cassette {} is required but unreadable ({e}); refresh with `nix run .#vcr-record`",
            path.display()
        )
    });
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("cassette {name} is not valid JSON: {e}"))
}

fn write_config(ws: &TempWorkspace, protocol: Protocol, base_url: &str) -> std::path::PathBuf {
    let cfg = ws.path("agent.toml");
    let dir = ws.dir.display();
    std::fs::write(
        &cfg,
        format!(
            r#"
[agent]
provider = "{provider}"
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
version  = "2023-06-01"
max_retries = 0

[memory]
backend       = "file"
episodic_path = "{dir}/.agent/episodic.jsonl"
semantic_dir  = "{dir}/.agent/memory"

[tools]
enabled = ["read_file", "write_file", "edit", "ls"]

[search]
auto_index = false

[metrics]
enabled = false

[git]
auto_fetch_secs = 0
"#,
            provider = protocol.provider(),
        ),
    )
    .expect("write config");
    cfg
}

/// The canonical tool-loop, replayed for each provider protocol: a tool call
/// crosses the wire in that protocol's shape, `hello.c` lands on disk, the tool
/// result is fed back in that protocol's shape, and the run exits 0.
#[rstest]
#[case::openai_compat(Protocol::OpenAiCompat)]
#[case::anthropic(Protocol::Anthropic)]
fn positive_golden_tool_loop_replays_over_the_wire(#[case] protocol: Protocol) {
    let ws = TempWorkspace::new(protocol.provider());
    let server = CassetteServer::start(load_cassette(protocol.cassette()));
    let cfg = write_config(&ws, protocol, server.base_url());

    let (code, stdout, stderr) = run_agent(
        &cfg,
        &ws,
        &["write a hello world program in C called hello.c"],
    );

    assert_eq!(code, 0, "{} exit; stderr:\n{stderr}", protocol.provider());
    assert!(
        stdout.contains("Wrote hello.c."),
        "the cassette's final answer must reach stdout, got:\n{stdout}"
    );

    // The tool call crossed the wire and produced the file, byte-intact — the
    // openai `arguments` string and the Anthropic `input` object both decode here.
    let written = std::fs::read_to_string(ws.path("hello.c")).expect("hello.c must exist");
    assert!(
        written.contains("int main(void)") && written.contains("Hello, World!"),
        "tool args survived the {} wire, got:\n{written}",
        protocol.provider()
    );
    assert!(
        written.contains(r#"printf("Hello, World!\n")"#),
        "the escaped \\n inside the C string literal must survive, got:\n{written}"
    );

    // The request went to this protocol's endpoint, proving the right wire was used.
    let reqs = server.requests();
    assert!(
        reqs.len() >= 2,
        "expected a second turn carrying the tool result"
    );
    assert!(
        reqs[0].path.ends_with(protocol.endpoint_suffix()),
        "{} must POST to {}, got {}",
        protocol.provider(),
        protocol.endpoint_suffix(),
        reqs[0].path
    );

    // The tool RESULT is fed back in the protocol's native shape.
    match protocol {
        Protocol::OpenAiCompat => {
            // Tools advertised as OpenAI functions; result returned with role "tool".
            let tools = reqs[0].body["tools"]
                .as_array()
                .expect("openai tools array");
            assert!(
                tools
                    .iter()
                    .any(|t| t["function"]["name"] == "write_file" && t["type"] == "function"),
                "write_file must be advertised in OpenAI function shape, got:\n{}",
                reqs[0].body["tools"]
            );
            let followup = reqs[1].body["messages"].to_string();
            assert!(
                followup.contains("\"tool\""),
                "the openai tool result must use the `tool` role, got:\n{followup}"
            );
        }
        Protocol::Anthropic => {
            // System prompt is hoisted OUT of messages (into `system`, as a string
            // or a cache-anchored block array); tools carry `input_schema`.
            assert!(
                !reqs[0].body["system"].is_null(),
                "Anthropic must hoist the system prompt into `system`, got:\n{}",
                reqs[0].body
            );
            assert!(
                reqs[0].body["messages"]
                    .as_array()
                    .is_some_and(|m| m.iter().all(|msg| msg["role"] != "system")),
                "no message may carry the `system` role on the Anthropic wire, got:\n{}",
                reqs[0].body["messages"]
            );
            let tools = reqs[0].body["tools"]
                .as_array()
                .expect("anthropic tools array");
            assert!(
                tools
                    .iter()
                    .any(|t| t["name"] == "write_file" && t["input_schema"].is_object()),
                "write_file must be advertised with an Anthropic `input_schema`, got:\n{}",
                reqs[0].body["tools"]
            );
            // The result rides back as a user `tool_result` content block.
            let followup = reqs[1].body["messages"].to_string();
            assert!(
                followup.contains("tool_result"),
                "the Anthropic tool result must be a `tool_result` block, got:\n{followup}"
            );
        }
    }
}

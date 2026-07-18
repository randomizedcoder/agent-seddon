//! End-to-end: our own `agent-mcp` client drives `agent --serve-mcp`.
//!
//! Exercises both MCP halves talking to each other over stdio — the `initialize`
//! handshake and `tools/list` — without any network (the agent is built but the
//! model is never called for discovery).

use agent_mcp::{connect, ServerConfig, Transport, TransportRegistry};

fn tempdir() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("agent-mcp-server-test-{nanos}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[tokio::test]
async fn server_advertises_run_tool() {
    let dir = tempdir();
    let cfg = dir.join("agent.toml");
    // Inline key so build_agent succeeds; the base_url is never hit for
    // initialize/tools.list.
    std::fs::write(
        &cfg,
        "[agent]\n\
         provider = \"openai-compat\"\n\
         policy = \"auto-approve\"\n\
         [provider]\n\
         base_url = \"http://localhost:9\"\n\
         model = \"test\"\n\
         api_key = \"x\"\n",
    )
    .unwrap();

    let server = ServerConfig {
        name: "seddon".into(),
        transport: Transport::Stdio {
            command: env!("CARGO_BIN_EXE_agent").to_string(),
            args: vec![
                "--serve-mcp".into(),
                "--config".into(),
                cfg.display().to_string(),
            ],
            env: vec![],
        },
    };

    let (_client, defs) = connect(&TransportRegistry::with_builtins(), &server)
        .await
        .expect("connect to agent --serve-mcp");
    assert!(
        defs.iter().any(|d| d.name == "run"),
        "server should advertise a `run` tool, got: {:?}",
        defs.iter().map(|d| &d.name).collect::<Vec<_>>()
    );
}

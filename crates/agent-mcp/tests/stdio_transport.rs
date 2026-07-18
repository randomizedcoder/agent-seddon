//! Hermetic stdio-transport test using `cat` as an echo "server".
//!
//! `cat` echoes each request line straight back. The echoed line carries our
//! request `id` but no `result`/`error`, so the transport correlates it by id and
//! resolves the request with a null result — exercising the full
//! write → read → route-by-id loop without needing a real MCP server.

use agent_mcp::{McpTransport, StdioTransport};
use serde_json::json;

#[tokio::test]
async fn stdio_round_trip_via_cat() {
    let transport = StdioTransport::spawn("cat", &[], &[])
        .await
        .expect("spawn cat");
    let result = transport
        .request("ping", json!({ "x": 1 }))
        .await
        .expect("round trip");
    assert!(result.is_null(), "echoed request has no result field");
}

#[tokio::test]
async fn stdio_spawn_missing_command_errors() {
    let err = StdioTransport::spawn("definitely-not-a-real-binary-xyz", &[], &[]).await;
    assert!(err.is_err());
}

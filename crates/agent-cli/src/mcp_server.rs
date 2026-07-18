//! Expose agent-seddon as an MCP server over stdio.
//!
//! `agent --serve-mcp` speaks JSON-RPC 2.0 on stdin/stdout so any MCP client
//! (Claude Desktop, another agent, …) can drive the whole agent loop as a single
//! `run` tool: pass a goal, get the final answer back. stdout carries *only*
//! JSON-RPC — logs and the streaming echo go to stderr.
//!
//! This is the server counterpart to the `agent-mcp` client crate. Only the tool
//! surface is implemented (initialize / tools.list / tools.call).
//!
//! Note: the agent's policy must be non-interactive (`auto-approve`) here —
//! stdin is the JSON-RPC channel, so an interactive approval prompt can't read it.

use agent_runtime::Agent;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL_VERSION: &str = "2025-06-18";

/// Serve until stdin closes. Each request is handled sequentially.
pub async fn serve(agent: &Agent) -> anyhow::Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    tracing::info!("mcp server ready on stdio (tool: `run`)");

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("mcp: ignoring malformed JSON-RPC ({e})");
                continue;
            }
        };

        // Requests have an id; notifications (e.g. notifications/initialized) do
        // not and get no response.
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");

        let response = match method {
            "initialize" => success(
                id,
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "agent-seddon", "version": env!("CARGO_PKG_VERSION") },
                }),
            ),
            "tools/list" => success(id, json!({ "tools": [run_tool_schema()] })),
            "tools/call" => handle_call(agent, id, &msg).await,
            "ping" => success(id, json!({})),
            other => error(id, -32601, &format!("method not found: {other}")),
        };

        let mut out = serde_json::to_string(&response)?;
        out.push('\n');
        stdout.write_all(out.as_bytes()).await?;
        stdout.flush().await?;
    }
    Ok(())
}

fn run_tool_schema() -> Value {
    json!({
        "name": "run",
        "description": "Run a task/goal through the agent-seddon coding agent (it may read \
                        and edit files and run commands in its working directory) and return \
                        the final answer.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "goal": { "type": "string", "description": "The task for the agent to perform." }
            },
            "required": ["goal"]
        }
    })
}

async fn handle_call(agent: &Agent, id: Value, msg: &Value) -> Value {
    let params = msg.get("params");
    let name = params
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if name != "run" {
        return error(id, -32602, &format!("unknown tool: `{name}`"));
    }
    let goal = params
        .and_then(|p| p.get("arguments"))
        .and_then(|a| a.get("goal"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if goal.is_empty() {
        return tool_result(id, "missing required `goal` argument", true);
    }

    tracing::info!(goal, "mcp: running delegated goal");
    match agent.run(goal).await {
        Ok(answer) => tool_result(id, &answer, false),
        Err(e) => tool_result(id, &format!("agent failed: {e}"), true),
    }
}

/// A JSON-RPC success carrying an MCP tool result (`content` + `isError`).
fn tool_result(id: Value, text: &str, is_error: bool) -> Value {
    success(
        id,
        json!({
            "content": [ { "type": "text", "text": text } ],
            "isError": is_error,
        }),
    )
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

//! A minimal Model Context Protocol (MCP) client.
//!
//! It connects to an MCP server over a transport (subprocess **stdio** or
//! **Streamable HTTP**), runs the `initialize` handshake, discovers the server's
//! tools (`tools/list`), and exposes each as an [`McpTool`] implementing the
//! `agent_core::Tool` seam — so MCP tools drop straight into the harness's
//! `ToolRegistry` alongside the built-ins. Tools are namespaced
//! `mcp_<server>_<tool>` to avoid collisions.
//!
//! Only the client half of MCP is implemented (request/response for tool
//! discovery + calls); server-initiated sampling/notifications are ignored.

mod stdio;
pub use stdio::StdioTransport;

mod http;
pub use http::HttpTransport;

use agent_core::{Observation, Tool, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// The MCP protocol version this client advertises.
const PROTOCOL_VERSION: &str = "2025-06-18";

pub type Result<T> = std::result::Result<T, McpError>;

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("mcp transport: {0}")]
    Transport(String),
    #[error("mcp protocol: {0}")]
    Protocol(String),
    #[error("mcp rpc error {code}: {message}")]
    Rpc { code: i64, message: String },
    #[error("mcp io: {0}")]
    Io(#[from] std::io::Error),
    #[error("mcp json: {0}")]
    Json(#[from] serde_json::Error),
}

/// A JSON-RPC transport to a single MCP server.
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// Send a JSON-RPC request and await its result (the `result` field).
    async fn request(&self, method: &str, params: Value) -> Result<Value>;
    /// Send a fire-and-forget JSON-RPC notification.
    async fn notify(&self, method: &str, params: Value) -> Result<()>;
}

/// How to reach an MCP server.
#[derive(Debug, Clone)]
pub enum Transport {
    /// Spawn a subprocess and speak JSON-RPC over its stdio.
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    /// Streamable HTTP endpoint.
    Http {
        url: String,
        headers: Vec<(String, String)>,
    },
}

/// A configured MCP server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub name: String,
    pub transport: Transport,
}

/// A tool as advertised by an MCP server.
#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

impl McpToolDef {
    fn from_value(v: &Value) -> Self {
        McpToolDef {
            name: v
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            description: v
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            // MCP calls it inputSchema; default to a permissive object schema.
            input_schema: v
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object"})),
        }
    }
}

/// A connected MCP server.
pub struct McpClient {
    transport: Box<dyn McpTransport>,
}

impl McpClient {
    /// Run the `initialize` handshake, then send `notifications/initialized`.
    pub async fn initialize(&self) -> Result<()> {
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "agent-seddon", "version": env!("CARGO_PKG_VERSION") },
        });
        self.transport.request("initialize", params).await?;
        self.transport
            .notify("notifications/initialized", json!({}))
            .await?;
        Ok(())
    }

    /// Discover the server's tools.
    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>> {
        let res = self.transport.request("tools/list", json!({})).await?;
        let tools = res
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| McpError::Protocol("tools/list: missing `tools` array".into()))?;
        Ok(tools.iter().map(McpToolDef::from_value).collect())
    }

    /// Invoke a tool by its (server-native) name.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<CallResult> {
        let res = self
            .transport
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await?;
        let is_error = res.get("isError").and_then(Value::as_bool).unwrap_or(false);
        Ok(CallResult {
            text: extract_text(&res),
            is_error,
        })
    }
}

/// The outcome of a `tools/call`, flattened to text for the agent loop.
pub struct CallResult {
    pub text: String,
    pub is_error: bool,
}

/// Flatten an MCP `content` array into text. Non-text blocks are summarised so
/// the model at least knows they were returned.
fn extract_text(result: &Value) -> String {
    let Some(blocks) = result.get("content").and_then(Value::as_array) else {
        return String::new();
    };
    let mut out = String::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    out.push_str(t);
                    out.push('\n');
                }
            }
            Some(other) => out.push_str(&format!("[{other} content omitted]\n")),
            None => {}
        }
    }
    out.trim_end().to_string()
}

/// Connect to a server, initialize it, and return the client plus its tool defs.
pub async fn connect(cfg: &ServerConfig) -> Result<(Arc<McpClient>, Vec<McpToolDef>)> {
    let transport: Box<dyn McpTransport> = match &cfg.transport {
        Transport::Stdio { command, args, env } => {
            Box::new(StdioTransport::spawn(command, args, env).await?)
        }
        Transport::Http { url, headers } => Box::new(HttpTransport::new(url, headers)?),
    };
    let client = Arc::new(McpClient { transport });
    client.initialize().await?;
    let defs = client.list_tools().await?;
    Ok((client, defs))
}

/// Connect and wrap every discovered tool as an `agent_core::Tool`.
pub async fn connect_tools(cfg: &ServerConfig) -> Result<Vec<Arc<dyn Tool>>> {
    let (client, defs) = connect(cfg).await?;
    Ok(defs
        .into_iter()
        .map(|def| Arc::new(McpTool::new(client.clone(), &cfg.name, def)) as Arc<dyn Tool>)
        .collect())
}

/// Adapts one MCP tool to the harness `Tool` seam.
pub struct McpTool {
    client: Arc<McpClient>,
    /// The server-native tool name (used on the wire).
    native_name: String,
    /// The namespaced name the model sees: `mcp_<server>_<tool>`.
    registered_name: String,
    description: String,
    input_schema: Value,
}

impl McpTool {
    pub fn new(client: Arc<McpClient>, server: &str, def: McpToolDef) -> Self {
        let registered_name = sanitize(&format!("mcp_{server}_{}", def.name));
        McpTool {
            client,
            native_name: def.name,
            registered_name,
            description: def.description,
            input_schema: def.input_schema,
        }
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.registered_name
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.registered_name.clone(),
            description: self.description.clone(),
            parameters: self.input_schema.clone(),
        }
    }
    async fn execute(
        &self,
        args: Value,
        _ctx: &agent_core::ToolContext,
    ) -> agent_core::Result<Observation> {
        match self.client.call_tool(&self.native_name, args).await {
            Ok(r) if r.is_error => Ok(Observation::error(r.text)),
            Ok(r) => Ok(Observation::ok(r.text)),
            Err(e) => Ok(Observation::error(format!(
                "mcp tool `{}` failed: {e}",
                self.registered_name
            ))),
        }
    }
}

/// Keep tool names within the character set model APIs accept for function names
/// (`[A-Za-z0-9_-]`), replacing anything else with `_`.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Parse a JSON-RPC response object into `Result<result_value>`, mapping a JSON-RPC
/// `error` object to [`McpError::Rpc`]. Shared by the transports.
pub(crate) fn parse_rpc_response(msg: &Value) -> Result<Value> {
    if let Some(err) = msg.get("error") {
        let code = err.get("code").and_then(Value::as_i64).unwrap_or(0);
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error")
            .to_string();
        return Err(McpError::Rpc { code, message });
    }
    Ok(msg.get("result").cloned().unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_def_parsing_and_namespacing() {
        let v = json!({
            "name": "read.file",
            "description": "read a file",
            "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}}
        });
        let def = McpToolDef::from_value(&v);
        assert_eq!(def.name, "read.file");
        // A dummy client so we can construct the adapter (no I/O).
        let client = Arc::new(McpClient {
            transport: Box::new(NullTransport),
        });
        let tool = McpTool::new(client, "fs", def);
        assert_eq!(tool.name(), "mcp_fs_read_file"); // '.' sanitized to '_'
    }

    #[test]
    fn extract_text_joins_text_blocks() {
        let res = json!({"content": [
            {"type": "text", "text": "line1"},
            {"type": "image", "data": "…"},
            {"type": "text", "text": "line2"}
        ]});
        assert_eq!(extract_text(&res), "line1\n[image content omitted]\nline2");
    }

    #[test]
    fn rpc_error_maps() {
        let msg = json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"no method"}});
        match parse_rpc_response(&msg) {
            Err(McpError::Rpc { code, message }) => {
                assert_eq!(code, -32601);
                assert_eq!(message, "no method");
            }
            other => panic!("expected rpc error, got {other:?}"),
        }
    }

    struct NullTransport;
    #[async_trait]
    impl McpTransport for NullTransport {
        async fn request(&self, _m: &str, _p: Value) -> Result<Value> {
            Ok(Value::Null)
        }
        async fn notify(&self, _m: &str, _p: Value) -> Result<()> {
            Ok(())
        }
    }
}

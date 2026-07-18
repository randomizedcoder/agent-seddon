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
use std::collections::BTreeMap;
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
///
/// The built-in variants (`Stdio`, `Http`) carry their own config; `Other` is the
/// escape hatch for out-of-tree transports registered on a [`TransportRegistry`] —
/// it names the transport `kind` and carries a free-form `params` blob the custom
/// factory interprets. Every variant reports a `kind()` string that the registry
/// keys off, so adding a transport never means editing [`connect`].
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
    /// A transport supplied out-of-tree, resolved by its `kind` on the registry.
    Other { kind: String, params: Value },
}

impl Transport {
    /// The registry key for this transport. Built-ins are `"stdio"` / `"http"`;
    /// an `Other` reports its own kind.
    pub fn kind(&self) -> &str {
        match self {
            Transport::Stdio { .. } => "stdio",
            Transport::Http { .. } => "http",
            Transport::Other { kind, .. } => kind,
        }
    }
}

/// Builds an [`McpTransport`] for a configured server. Registered on a
/// [`TransportRegistry`] under a `kind` string, this is the MCP-side equivalent of
/// the runtime's seam factories: a new transport (websocket, in-process, …) drops
/// in by registering a factory — no change to [`connect`].
#[async_trait]
pub trait TransportFactory: Send + Sync {
    async fn build(&self, cfg: &ServerConfig) -> Result<Box<dyn McpTransport>>;
}

/// `kind` → factory map for MCP transports. Mirrors `agent_runtime::Registry`:
/// built-ins are wired by [`TransportRegistry::with_builtins`]; out-of-tree code
/// registers its own before calling [`connect`] / [`connect_tools`] (the runtime
/// threads one through `agent_runtime::Registry`).
#[derive(Default)]
pub struct TransportRegistry {
    factories: BTreeMap<String, Box<dyn TransportFactory>>,
}

impl TransportRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// A registry pre-populated with the built-in `stdio` + `http` transports.
    pub fn with_builtins() -> Self {
        let mut r = Self::new();
        r.register("stdio", StdioFactory);
        r.register("http", HttpFactory);
        r
    }

    pub fn register(&mut self, kind: impl Into<String>, factory: impl TransportFactory + 'static) {
        self.factories.insert(kind.into(), Box::new(factory));
    }

    /// Look up the factory for `cfg`'s transport kind and build it.
    pub async fn build(&self, cfg: &ServerConfig) -> Result<Box<dyn McpTransport>> {
        let kind = cfg.transport.kind();
        let factory = self.factories.get(kind).ok_or_else(|| {
            let known: Vec<&str> = self.factories.keys().map(String::as_str).collect();
            McpError::Transport(format!(
                "unknown transport kind `{kind}` (known: {})",
                known.join(", ")
            ))
        })?;
        factory.build(cfg).await
    }
}

/// Built-in factory for the subprocess-stdio transport.
struct StdioFactory;
#[async_trait]
impl TransportFactory for StdioFactory {
    async fn build(&self, cfg: &ServerConfig) -> Result<Box<dyn McpTransport>> {
        match &cfg.transport {
            Transport::Stdio { command, args, env } => {
                Ok(Box::new(StdioTransport::spawn(command, args, env).await?))
            }
            other => Err(McpError::Transport(format!(
                "stdio factory received a `{}` transport",
                other.kind()
            ))),
        }
    }
}

/// Built-in factory for the streamable-HTTP transport.
struct HttpFactory;
#[async_trait]
impl TransportFactory for HttpFactory {
    async fn build(&self, cfg: &ServerConfig) -> Result<Box<dyn McpTransport>> {
        match &cfg.transport {
            Transport::Http { url, headers } => Ok(Box::new(HttpTransport::new(url, headers)?)),
            other => Err(McpError::Transport(format!(
                "http factory received a `{}` transport",
                other.kind()
            ))),
        }
    }
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
    /// Wrap an already-built transport. `connect` uses this after resolving a
    /// transport from the registry; tests use it to drive the client with a fake
    /// transport (e.g. `agent_testkit::mcp::ScriptedTransport`) directly.
    pub fn with_transport(transport: Box<dyn McpTransport>) -> Self {
        Self { transport }
    }

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
/// The transport is resolved through `transports` (use
/// [`TransportRegistry::with_builtins`] for the stock stdio/http set).
pub async fn connect(
    transports: &TransportRegistry,
    cfg: &ServerConfig,
) -> Result<(Arc<McpClient>, Vec<McpToolDef>)> {
    let transport = transports.build(cfg).await?;
    let client = Arc::new(McpClient::with_transport(transport));
    client.initialize().await?;
    let defs = client.list_tools().await?;
    Ok((client, defs))
}

/// Connect and wrap every discovered tool as an `agent_core::Tool`.
pub async fn connect_tools(
    transports: &TransportRegistry,
    cfg: &ServerConfig,
) -> Result<Vec<Arc<dyn Tool>>> {
    let (client, defs) = connect(transports, cfg).await?;
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
    use rstest::rstest;

    // --- sanitize: model-safe tool names -----------------------------------
    #[rstest]
    #[case::positive_alnum("abc123", "abc123")]
    #[case::positive_keeps_underscore_hyphen("a_b-c", "a_b-c")]
    #[case::negative_dot("read.file", "read_file")]
    #[case::negative_slash_and_space("a/b c", "a_b_c")]
    #[case::corner_unicode("café", "caf_")]
    #[case::corner_all_invalid("!!!", "___")]
    #[case::boundary_empty("", "")]
    fn sanitize_cases(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(sanitize(input), expected);
    }

    // --- McpToolDef::from_value: field extraction + defaults ----------------
    #[rstest]
    #[case::positive_all(json!({"name":"n","description":"d"}), "n", "d")]
    #[case::boundary_missing_name(json!({"description":"d"}), "", "d")]
    #[case::boundary_missing_description(json!({"name":"n"}), "n", "")]
    #[case::boundary_empty_object(json!({}), "", "")]
    #[case::corner_null_fields(json!({"name":null,"description":null}), "", "")]
    fn tool_def_from_value_cases(#[case] v: Value, #[case] name: &str, #[case] desc: &str) {
        let def = McpToolDef::from_value(&v);
        assert_eq!(def.name, name);
        assert_eq!(def.description, desc);
    }

    #[test]
    fn tool_def_missing_input_schema_defaults_to_object() {
        let def = McpToolDef::from_value(&json!({"name": "n"}));
        assert_eq!(def.input_schema, json!({"type": "object"}));
    }

    #[test]
    fn mcp_tool_name_is_namespaced_and_sanitized() {
        let def = McpToolDef::from_value(&json!({"name": "read.file"}));
        let client = Arc::new(McpClient {
            transport: Box::new(NullTransport),
        });
        assert_eq!(McpTool::new(client, "fs", def).name(), "mcp_fs_read_file");
    }

    // --- extract_text: MCP content-block flattening ------------------------
    #[rstest]
    #[case::boundary_no_content(json!({}), "")]
    #[case::boundary_empty_array(json!({"content": []}), "")]
    #[case::positive_text_only(
        json!({"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}),
        "a\nb"
    )]
    #[case::positive_mixed(
        json!({"content":[
            {"type":"text","text":"line1"},
            {"type":"image","data":"…"},
            {"type":"text","text":"line2"}
        ]}),
        "line1\n[image content omitted]\nline2"
    )]
    #[case::corner_text_block_without_text(json!({"content":[{"type":"text"}]}), "")]
    fn extract_text_cases(#[case] v: Value, #[case] expected: &str) {
        assert_eq!(extract_text(&v), expected);
    }

    // --- parse_rpc_response: success path ----------------------------------
    #[rstest]
    #[case::positive_result(json!({"result": {"ok": true}}), json!({"ok": true}))]
    #[case::boundary_missing_result(json!({"jsonrpc":"2.0","id":1}), Value::Null)]
    fn parse_rpc_ok_cases(#[case] msg: Value, #[case] expected: Value) {
        assert_eq!(parse_rpc_response(&msg).unwrap(), expected);
    }

    // --- parse_rpc_response: error extraction + defaults -------------------
    #[rstest]
    #[case::positive_full(json!({"error":{"code":-32601,"message":"no method"}}), -32601, "no method")]
    #[case::boundary_missing_code(json!({"error":{"message":"boom"}}), 0, "boom")]
    #[case::boundary_missing_message(json!({"error":{"code":5}}), 5, "unknown error")]
    #[case::corner_empty_error_obj(json!({"error":{}}), 0, "unknown error")]
    fn parse_rpc_error_cases(#[case] msg: Value, #[case] code: i64, #[case] message: &str) {
        match parse_rpc_response(&msg) {
            Err(McpError::Rpc {
                code: c,
                message: m,
            }) => {
                assert_eq!(c, code);
                assert_eq!(m, message);
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

    #[rstest]
    #[case::stdio(Transport::Stdio { command: "cat".into(), args: vec![], env: vec![] }, "stdio")]
    #[case::http(Transport::Http { url: "http://x".into(), headers: vec![] }, "http")]
    #[case::other(Transport::Other { kind: "websocket".into(), params: json!({}) }, "websocket")]
    fn transport_kind_cases(#[case] transport: Transport, #[case] expected: &str) {
        assert_eq!(transport.kind(), expected);
    }

    struct NullFactory;
    #[async_trait]
    impl TransportFactory for NullFactory {
        async fn build(&self, _cfg: &ServerConfig) -> Result<Box<dyn McpTransport>> {
            Ok(Box::new(NullTransport))
        }
    }

    /// An out-of-tree transport is reachable by registering a factory under its
    /// `Other` kind — no edit to `connect`.
    #[tokio::test]
    async fn registry_builds_custom_transport() {
        let mut reg = TransportRegistry::new();
        reg.register("websocket", NullFactory);
        let cfg = ServerConfig {
            name: "ws".into(),
            transport: Transport::Other {
                kind: "websocket".into(),
                params: json!({"url": "ws://x"}),
            },
        };
        assert!(reg.build(&cfg).await.is_ok());
    }

    /// An unknown kind lists the known ones, matching the runtime registry's
    /// error style.
    #[tokio::test]
    async fn registry_unknown_kind_lists_known() {
        let reg = TransportRegistry::with_builtins();
        let cfg = ServerConfig {
            name: "x".into(),
            transport: Transport::Other {
                kind: "nope".into(),
                params: json!({}),
            },
        };
        let err = match reg.build(&cfg).await {
            Err(e) => e.to_string(),
            Ok(_) => panic!("expected an unknown-kind error"),
        };
        assert!(err.contains("unknown transport kind `nope`"), "{err}");
        assert!(err.contains("stdio"), "{err}");
        assert!(err.contains("http"), "{err}");
    }
}

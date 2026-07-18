//! stdio transport: spawn the MCP server as a subprocess and exchange
//! newline-delimited JSON-RPC messages over its stdin/stdout.

use crate::{parse_rpc_response, McpError, McpTransport, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{oneshot, Mutex};

/// How long to wait for a response to a request before giving up.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;

pub struct StdioTransport {
    stdin: Mutex<ChildStdin>,
    pending: Pending,
    next_id: AtomicU64,
    // Kept so the child (and its kill-on-drop) outlives the transport.
    _child: Child,
}

impl StdioTransport {
    /// Spawn `command args…` (with extra `env`) and start the reader task.
    pub async fn spawn(command: &str, args: &[String], env: &[(String, String)]) -> Result<Self> {
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .map_err(|e| McpError::Transport(format!("spawning `{command}`: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Transport("no child stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Transport("no child stdout".into()))?;
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        // Reader: route each JSON-RPC response to the waiting request by id.
        let pending_reader = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<Value>(&line) {
                    Ok(msg) => route(&pending_reader, &msg).await,
                    Err(e) => tracing::debug!(target: "mcp", "bad json from server: {e}"),
                }
            }
        });

        // Drain stderr to server-log debug (prevents the pipe from filling).
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "mcp.stderr", "{line}");
                }
            });
        }

        Ok(Self {
            stdin: Mutex::new(stdin),
            pending,
            next_id: AtomicU64::new(1),
            _child: child,
        })
    }

    async fn write_message(&self, msg: &Value) -> Result<()> {
        let mut line = serde_json::to_string(msg)?;
        line.push('\n');
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(line.as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }
}

async fn route(pending: &Pending, msg: &Value) {
    // Responses carry an id; server-initiated requests/notifications are ignored.
    let Some(id) = msg.get("id").and_then(Value::as_u64) else {
        return;
    };
    if let Some(tx) = pending.lock().await.remove(&id) {
        let _ = tx.send(parse_rpc_response(msg));
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if let Err(e) = self.write_message(&msg).await {
            self.pending.lock().await.remove(&id);
            return Err(e);
        }

        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => Err(McpError::Transport(
                "server closed before responding".into(),
            )),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(McpError::Transport(format!("request `{method}` timed out")))
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.write_message(&msg).await
    }
}

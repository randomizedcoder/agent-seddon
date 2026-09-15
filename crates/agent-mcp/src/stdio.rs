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

/// Cap on a single newline-delimited message from the subprocess. An MCP server
/// is untrusted, and `AsyncBufReadExt::lines`/`read_line` grow a `String` without
/// bound until a newline arrives — a server that emits a huge line (or none at
/// all) would OOM. 32 MiB matches the HTTP transport's body cap.
const MAX_LINE_BYTES: usize = 32 * 1024 * 1024;

/// Read one `\n`-delimited line into `buf` (newline stripped), capped at `max`
/// bytes. `Ok(true)` ⇒ a line was read; `Ok(false)` ⇒ EOF; `Err` ⇒ the line
/// exceeded `max` (fail closed rather than buffer without bound).
async fn read_capped_line<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max: usize,
) -> std::io::Result<bool> {
    buf.clear();
    loop {
        // Extract everything needed from the borrowed fill_buf slice *before*
        // `consume` (which needs `&mut reader` again).
        let (found, consumed, over) = {
            let chunk = reader.fill_buf().await?;
            if chunk.is_empty() {
                // EOF: report a trailing unterminated line once, else stop.
                return Ok(!buf.is_empty());
            }
            match chunk.iter().position(|&b| b == b'\n') {
                Some(pos) => {
                    let over = buf.len() + pos > max;
                    if !over {
                        buf.extend_from_slice(&chunk[..pos]);
                    }
                    (true, pos + 1, over)
                }
                None => {
                    let take = chunk.len();
                    let over = buf.len() + take > max;
                    if !over {
                        buf.extend_from_slice(chunk);
                    }
                    (false, take, over)
                }
            }
        };
        reader.consume(consumed);
        if over {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "mcp line exceeds size cap",
            ));
        }
        if found {
            return Ok(true);
        }
    }
}

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

        // Reader: route each JSON-RPC response to the waiting request by id. Each
        // line is size-capped — an untrusted server can't OOM us with a huge or
        // never-terminated line.
        let pending_reader = pending.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = Vec::new();
            loop {
                match read_capped_line(&mut reader, &mut line, MAX_LINE_BYTES).await {
                    Ok(true) => {}
                    Ok(false) => break, // EOF
                    Err(e) => {
                        tracing::debug!(target: "mcp", "stdout reader stopped: {e}");
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&line);
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match serde_json::from_str::<Value>(trimmed) {
                    Ok(msg) => route(&pending_reader, &msg).await,
                    Err(e) => tracing::debug!(target: "mcp", "bad json from server: {e}"),
                }
            }
        });

        // Drain stderr to server-log debug (prevents the pipe from filling), also
        // size-capped per line.
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = Vec::new();
                loop {
                    match read_capped_line(&mut reader, &mut line, MAX_LINE_BYTES).await {
                        Ok(true) => tracing::debug!(
                            target: "mcp.stderr",
                            "{}",
                            String::from_utf8_lossy(&line)
                        ),
                        Ok(false) => break,
                        Err(e) => {
                            tracing::debug!(target: "mcp.stderr", "reader stopped: {e}");
                            break;
                        }
                    }
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

#[cfg(test)]
mod tests {
    use super::read_capped_line;
    use rstest::rstest;
    use tokio::io::BufReader;

    /// Read all lines from `data` with cap `max`, returning `Ok(lines)` or the
    /// error string if the cap tripped mid-stream.
    async fn read_all(data: &[u8], max: usize) -> Result<Vec<String>, String> {
        let mut reader = BufReader::new(data);
        let mut out = Vec::new();
        let mut buf = Vec::new();
        loop {
            match read_capped_line(&mut reader, &mut buf, max).await {
                Ok(true) => out.push(String::from_utf8_lossy(&buf).into_owned()),
                Ok(false) => return Ok(out),
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    // Two full lines, newline stripped, then clean EOF.
    #[tokio::test]
    async fn positive_reads_delimited_lines() {
        assert_eq!(read_all(b"a\nbb\n", 1024).await.unwrap(), vec!["a", "bb"]);
    }

    // A trailing line without a newline is still yielded once (then EOF).
    #[tokio::test]
    async fn corner_trailing_unterminated_line_is_yielded() {
        assert_eq!(read_all(b"x\nyz", 1024).await.unwrap(), vec!["x", "yz"]);
    }

    // Empty input is immediate EOF, no lines.
    #[tokio::test]
    async fn boundary_empty_input_is_eof() {
        assert!(read_all(b"", 1024).await.unwrap().is_empty());
    }

    // A line exactly at the cap is fine; one byte over trips the cap.
    #[rstest]
    #[case::at_cap(4, "aaaa\n", true)]
    #[case::over_cap(4, "aaaaa\n", false)]
    #[tokio::test]
    async fn boundary_line_at_and_over_cap(
        #[case] max: usize,
        #[case] input: &str,
        #[case] ok: bool,
    ) {
        assert_eq!(read_all(input.as_bytes(), max).await.is_ok(), ok);
    }

    // A never-terminated flood far over the cap fails closed instead of buffering
    // it all (the OOM the cap exists to prevent).
    #[tokio::test]
    async fn adversarial_unterminated_flood_fails_closed() {
        let flood = vec![b'x'; 100_000]; // no '\n'
        let err = read_all(&flood, 1024).await.unwrap_err();
        assert!(err.contains("exceeds size cap"), "{err}");
    }
}

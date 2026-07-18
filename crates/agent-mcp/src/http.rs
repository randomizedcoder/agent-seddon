//! Streamable-HTTP transport: POST JSON-RPC to a single endpoint. The server may
//! answer with `application/json` (one response) or `text/event-stream` (SSE
//! carrying the response). A session id from the `Mcp-Session-Id` response header
//! is echoed on subsequent requests.
//!
//! Only the request/response path is implemented — enough for tool discovery and
//! calls. The optional server→client SSE channel (GET) is not opened.

use crate::{parse_rpc_response, McpError, McpTransport, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;

pub struct HttpTransport {
    client: reqwest::Client,
    url: String,
    headers: Vec<(String, String)>,
    session: Mutex<Option<String>>,
    next_id: AtomicU64,
}

impl HttpTransport {
    pub fn new(url: &str, headers: &[(String, String)]) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| McpError::Transport(format!("building http client: {e}")))?;
        Ok(Self {
            client,
            url: url.to_string(),
            headers: headers.to_vec(),
            session: Mutex::new(None),
            next_id: AtomicU64::new(1),
        })
    }

    /// POST a JSON-RPC message. Returns the response object when `expect_response`
    /// (a request), or `None` for a notification.
    async fn post(&self, body: Value, expect_response: bool) -> Result<Option<Value>> {
        let mut req = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .json(&body);
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }
        if let Some(sid) = self.session.lock().await.clone() {
            req = req.header("mcp-session-id", sid);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| McpError::Transport(format!("http request: {e}")))?;

        if let Some(sid) = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|h| h.to_str().ok())
        {
            *self.session.lock().await = Some(sid.to_string());
        }

        let status = resp.status();
        let ctype = resp
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(McpError::Transport(format!("http {status}: {text}")));
        }
        if !expect_response {
            return Ok(None);
        }

        if ctype.contains("text/event-stream") {
            let text = resp
                .text()
                .await
                .map_err(|e| McpError::Transport(format!("reading sse body: {e}")))?;
            first_sse_response(&text).map(Some)
        } else {
            let msg: Value = resp
                .json()
                .await
                .map_err(|e| McpError::Transport(format!("decoding json response: {e}")))?;
            Ok(Some(msg))
        }
    }
}

/// Return the first `data:` payload that is a JSON-RPC response (has an `id`).
fn first_sse_response(body: &str) -> Result<Value> {
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        if let Ok(msg) = serde_json::from_str::<Value>(data) {
            if msg.get("id").is_some() {
                return Ok(msg);
            }
        }
    }
    Err(McpError::Protocol(
        "no JSON-RPC response found in event stream".into(),
    ))
}

#[async_trait]
impl McpTransport for HttpTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let msg = self
            .post(body, true)
            .await?
            .ok_or_else(|| McpError::Protocol("empty response to request".into()))?;
        parse_rpc_response(&msg)
    }

    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let body = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.post(body, false).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::first_sse_response;

    #[test]
    fn extracts_response_from_sse() {
        let body =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n";
        let msg = first_sse_response(body).unwrap();
        assert_eq!(msg["id"], 1);
        assert_eq!(msg["result"]["ok"], true);
    }
}

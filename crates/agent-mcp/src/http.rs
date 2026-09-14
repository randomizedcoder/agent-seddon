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

/// Cap on a single MCP HTTP response body. An MCP server is untrusted (the model
/// is prompt-injectable and the server itself may be hostile or buggy), so a body
/// is streamed with a running byte cap rather than buffered whole:
/// `reqwest::Response::text`/`json` have no size limit and would OOM the process
/// on an unbounded response. 32 MiB matches the LSP transport's body cap.
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// Read a response body with a hard size cap, never trusting the advertised
/// length. A `Content-Length` over `max` is rejected up front; a chunked body
/// (no length) is capped as it streams so it can't grow memory without bound.
async fn bounded_bytes(mut resp: reqwest::Response, max: usize) -> Result<Vec<u8>> {
    if let Some(len) = resp.content_length() {
        if len > max as u64 {
            return Err(McpError::Transport(format!(
                "response too large ({len} bytes)"
            )));
        }
    }
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| McpError::Transport(format!("reading response body: {e}")))?
    {
        if buf.len() + chunk.len() > max {
            return Err(McpError::Transport(format!(
                "response too large (> {max} bytes)"
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

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
            // Best-effort error body, still size-capped (a hostile error response
            // can be as large as a success one).
            let text = bounded_bytes(resp, MAX_RESPONSE_BYTES)
                .await
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            return Err(McpError::Transport(format!("http {status}: {text}")));
        }
        if !expect_response {
            return Ok(None);
        }

        if ctype.contains("text/event-stream") {
            let bytes = bounded_bytes(resp, MAX_RESPONSE_BYTES).await?;
            first_sse_response(&String::from_utf8_lossy(&bytes)).map(Some)
        } else {
            let bytes = bounded_bytes(resp, MAX_RESPONSE_BYTES).await?;
            let msg: Value = serde_json::from_slice(&bytes)
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
    use super::{bounded_bytes, first_sse_response};
    use rstest::rstest;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Serve one canned HTTP response (raw head + body) once, then close. Returns
    /// the URL to GET. `head` must end the header block (`\r\n\r\n` is appended).
    async fn serve_once(head: String, body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut scratch = [0u8; 2048];
                let _ = sock.read(&mut scratch).await; // drain the request line/headers
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(b"\r\n\r\n").await;
                let _ = sock.write_all(&body).await;
                let _ = sock.shutdown().await;
            }
        });
        format!("http://{addr}/")
    }

    async fn get(url: &str) -> reqwest::Response {
        reqwest::Client::new().get(url).send().await.unwrap()
    }

    // A body under the cap is returned in full.
    #[tokio::test]
    async fn positive_body_under_cap_is_returned() {
        let url = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close".into(),
            b"hello".to_vec(),
        )
        .await;
        let got = bounded_bytes(get(&url).await, 1024).await.unwrap();
        assert_eq!(got, b"hello");
    }

    // An advertised Content-Length over the cap is rejected up front, before the
    // body is read (the primary defence — the server never gets to stream GBs).
    #[tokio::test]
    async fn adversarial_oversized_content_length_rejected() {
        let url = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Length: 9999999\r\nConnection: close".into(),
            vec![b'x'; 32],
        )
        .await;
        let err = bounded_bytes(get(&url).await, 1024).await.unwrap_err();
        assert!(err.to_string().contains("too large"), "{err}");
    }

    // A chunked/close-delimited body with no Content-Length is capped as it
    // streams — the header can't be trusted, so the running total is the guard.
    #[tokio::test]
    async fn adversarial_unbounded_chunked_body_is_capped() {
        // No Content-Length; body far exceeds the cap. Must error, not buffer.
        let url = serve_once(
            "HTTP/1.1 200 OK\r\nConnection: close".into(),
            vec![b'x'; 8192],
        )
        .await;
        let err = bounded_bytes(get(&url).await, 1024).await.unwrap_err();
        assert!(err.to_string().contains("too large"), "{err}");
    }

    // A body that ends exactly at the cap is accepted; one byte over is rejected.
    #[rstest]
    #[case::at_cap(4, b"data".to_vec(), true)]
    #[case::over_cap(4, b"datum".to_vec(), false)]
    #[tokio::test]
    async fn boundary_body_at_and_over_cap(
        #[case] max: usize,
        #[case] body: Vec<u8>,
        #[case] ok: bool,
    ) {
        let url = serve_once("HTTP/1.1 200 OK\r\nConnection: close".into(), body).await;
        assert_eq!(bounded_bytes(get(&url).await, max).await.is_ok(), ok);
    }

    /// `Some(id)` ⇒ parses to a response whose `id` equals it; `None` ⇒ errors.
    #[rstest]
    #[case::positive_basic(
        "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n",
        Some(1)
    )]
    #[case::positive_skips_non_data_and_no_id(
        "event: ping\ndata: {\"jsonrpc\":\"2.0\"}\ndata: {\"id\":7,\"result\":1}\n",
        Some(7)
    )]
    #[case::corner_whitespace_after_data("data:    {\"id\":3}\n", Some(3))]
    #[case::negative_empty_body("", None)]
    #[case::negative_no_data_lines("event: message\n: comment\n", None)]
    #[case::negative_empty_payload("data: \ndata:\n", None)]
    #[case::negative_non_json("data: not json at all\n", None)]
    #[case::negative_json_without_id("data: {\"result\": 1}\n", None)]
    fn first_sse_response_cases(#[case] body: &str, #[case] expected_id: Option<i64>) {
        match (first_sse_response(body), expected_id) {
            (Ok(msg), Some(id)) => assert_eq!(msg["id"], id),
            (Err(_), None) => {}
            (got, exp) => panic!("body {body:?}: got {got:?}, expected id {exp:?}"),
        }
    }
}

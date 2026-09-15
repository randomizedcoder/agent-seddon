//! `tokenizer-provider` — exact counts from a provider's own count-tokens endpoint.
//!
//! The most authoritative count for a hosted model is the provider's: this backend
//! POSTs to an Anthropic-style `…/messages/count_tokens` endpoint and reads back
//! `input_tokens`. Unlike the other backends it needs **network + an API key** at
//! runtime, so it is off by default and gate-tested against a loopback server
//! (`tests/provider_e2e.rs`), never the real endpoint.
//!
//! Security (the endpoint + its responses are untrusted — CLAUDE.md):
//! - the API key travels only in the `x-api-key` header and never appears in a
//!   result, error, span, or log; errors carry an HTTP status or a generic phrase;
//! - the returned `input_tokens` is parsed as `u64` and **clamped** to `u32`, so a
//!   hostile huge/negative number can't wrap or panic a downstream `inc_by`;
//! - the response is size-capped by a streamed byte limit (independent of the
//!   advertised `Content-Length`, which a chunked body omits) and the client is
//!   timeout-bounded, so a slow or giant response can't wedge the compaction loop;
//! - any failure returns `Err`, and the caller (compaction) falls back to its
//!   heuristic — a count is never fabricated.
//!
//! See parity spec 23.

use agent_core::{media_block_tokens, ContentBlock, Error, Message, Result, Role, Tokenizer};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

/// Cap on a count-tokens response body. A real one is a few dozen bytes; this
/// refuses a hostile giant body announced via `Content-Length`.
const MAX_RESPONSE_BYTES: u64 = 64 * 1024;

/// A [`Tokenizer`] that counts via a provider's `messages/count_tokens` endpoint.
pub struct ProviderTokenizer {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    version: String,
}

// Never derive `Debug` — it would print `api_key`.
impl std::fmt::Debug for ProviderTokenizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderTokenizer")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl ProviderTokenizer {
    /// Build the backend. Fails closed if `base_url` or `api_key` is empty (a
    /// misconfiguration that must not silently send unauthenticated requests).
    pub fn new(base_url: &str, api_key: &str, version: &str, timeout_secs: u64) -> Result<Self> {
        if base_url.trim().is_empty() {
            return Err(Error::Tokenizer(
                "provider tokenizer: base_url is empty".into(),
            ));
        }
        if api_key.is_empty() {
            return Err(Error::Tokenizer(
                "provider tokenizer: api_key is empty (set [tokenizer.provider] api_key/api_key_env)".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs.clamp(1, 600)))
            .build()
            .map_err(|e| {
                Error::Tokenizer(format!("provider tokenizer: building http client: {e}"))
            })?;
        Ok(Self {
            client,
            endpoint: format!("{}/messages/count_tokens", base_url.trim_end_matches('/')),
            api_key: api_key.to_string(),
            version: version.to_string(),
        })
    }

    /// POST one count-tokens `body` and return the clamped `input_tokens`. Errors
    /// never carry the API key or the raw response.
    async fn count_tokens(&self, body: Value) -> Result<u32> {
        let resp = self
            .client
            .post(&self.endpoint)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", &self.version)
            .json(&body)
            .send()
            .await
            .map_err(|_| Error::Tokenizer("provider count_tokens request failed".into()))?;

        let status = resp.status();
        if !status.is_success() {
            // Status only — the body may echo request content, and we never leak it.
            return Err(Error::Tokenizer(format!(
                "provider count_tokens: HTTP {status}"
            )));
        }
        let text = read_capped(resp, MAX_RESPONSE_BYTES).await?;
        let v: Value = serde_json::from_str(&text)
            .map_err(|_| Error::Tokenizer("provider count_tokens: malformed response".into()))?;
        let n = v
            .get("input_tokens")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::Tokenizer("provider count_tokens: no input_tokens".into()))?;
        // Clamp a hostile/huge value into range rather than wrap or panic.
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    }
}

/// Read a response body with a hard size cap that does *not* depend on the
/// advertised `Content-Length`. The old check only fired when `content_length()`
/// was `Some`, so a chunked response (no length header) reached `resp.text()`,
/// which buffers without bound — a hostile/compromised provider endpoint could
/// OOM us. Reject an over-cap length up front and cap the streamed body too.
async fn read_capped(mut resp: reqwest::Response, max: u64) -> Result<String> {
    if resp.content_length().is_some_and(|n| n > max) {
        return Err(Error::Tokenizer(
            "provider count_tokens: response too large".into(),
        ));
    }
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|_| Error::Tokenizer("provider count_tokens: reading response failed".into()))?
    {
        if buf.len() as u64 + chunk.len() as u64 > max {
            return Err(Error::Tokenizer(
                "provider count_tokens: response too large".into(),
            ));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Flatten a message's countable text (text blocks + tool-call name/args) into one
/// string. Media blocks are handled separately (they aren't sent as text).
fn flatten_text(m: &Message) -> String {
    let mut s = String::new();
    for block in &m.content {
        if let ContentBlock::Text { text } = block {
            s.push_str(text);
            s.push(' ');
        }
    }
    for tc in &m.tool_calls {
        s.push_str(&tc.name);
        s.push(' ');
        s.push_str(&tc.arguments.to_string());
        s.push(' ');
    }
    s.truncate(s.trim_end().len());
    s
}

#[async_trait]
impl Tokenizer for ProviderTokenizer {
    fn backend(&self) -> &str {
        "provider"
    }

    async fn count(&self, text: &str, model: &str) -> Result<u32> {
        // count_tokens rejects an empty message; short-circuit.
        if text.is_empty() {
            return Ok(0);
        }
        self.count_tokens(json!({
            "model": model,
            "messages": [{ "role": "user", "content": text }],
        }))
        .await
    }

    async fn count_messages(&self, messages: &[Message], model: &str) -> Result<u32> {
        let mut system = String::new();
        let mut arr: Vec<Value> = Vec::new();
        // Media blocks aren't sent as text; account for them with the shared
        // size-based estimate on top of the provider's text count.
        let mut media_extra: u32 = 0;
        for m in messages {
            for block in &m.content {
                if !matches!(block, ContentBlock::Text { .. }) {
                    media_extra = media_extra.saturating_add(media_block_tokens(block));
                }
            }
            let text = flatten_text(m);
            match m.role {
                Role::System => {
                    if !text.is_empty() {
                        system.push_str(&text);
                        system.push('\n');
                    }
                }
                // Only `user`/`assistant` are valid message roles; a tool result is
                // folded in as a user turn for counting purposes.
                role => {
                    if !text.is_empty() {
                        let r = if role == Role::Assistant {
                            "assistant"
                        } else {
                            "user"
                        };
                        arr.push(json!({ "role": r, "content": text }));
                    }
                }
            }
        }
        // An empty message array would be rejected; nothing to count but media.
        if arr.is_empty() {
            return Ok(media_extra);
        }
        let mut body = json!({ "model": model, "messages": arr });
        if !system.is_empty() {
            body["system"] = json!(system.trim_end());
        }
        Ok(self.count_tokens(body).await?.saturating_add(media_extra))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_fails_closed_without_key_or_url() {
        assert!(ProviderTokenizer::new("https://x/v1", "", "2023-06-01", 30).is_err());
        assert!(ProviderTokenizer::new("", "k", "2023-06-01", 30).is_err());
        assert!(ProviderTokenizer::new("https://x/v1", "k", "2023-06-01", 30).is_ok());
    }

    #[test]
    fn debug_never_prints_key() {
        let t = ProviderTokenizer::new("https://x/v1", "super-secret", "v", 30).unwrap();
        assert!(!format!("{t:?}").contains("super-secret"));
    }

    #[test]
    fn flatten_includes_text_and_tool_calls() {
        let mut m = Message::assistant("hello");
        m.tool_calls.push(agent_core::ToolCall {
            id: "1".into(),
            name: "ls".into(),
            arguments: json!({ "path": "." }),
        });
        let s = flatten_text(&m);
        assert!(s.contains("hello") && s.contains("ls") && s.contains("path"));
    }

    #[tokio::test]
    async fn empty_text_is_zero_without_a_call() {
        // No server: an empty text must short-circuit to 0, never touch the network.
        let t = ProviderTokenizer::new("http://127.0.0.1:1/v1", "k", "v", 1).unwrap();
        assert_eq!(t.count("", "m").await.unwrap(), 0);
    }

    // --- read_capped: size cap independent of Content-Length ----------------
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Serve one response once (raw head + body). `head` should NOT include the
    /// final blank line (`\r\n\r\n` is appended). Returns the URL to GET.
    async fn serve_once(head: String, body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut scratch = [0u8; 1024];
                let _ = sock.read(&mut scratch).await;
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

    #[tokio::test]
    async fn positive_body_under_cap_is_returned() {
        let url = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close".into(),
            b"hi".to_vec(),
        )
        .await;
        assert_eq!(read_capped(get(&url).await, 64).await.unwrap(), "hi");
    }

    #[tokio::test]
    async fn adversarial_oversized_content_length_rejected() {
        let url = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Length: 9999\r\nConnection: close".into(),
            vec![b'x'; 16],
        )
        .await;
        let err = read_capped(get(&url).await, 64)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("too large"), "{err}");
    }

    /// The regression this fix targets: NO Content-Length (chunked/close-delimited)
    /// with a body over the cap. The old header-only check missed this and buffered
    /// it all; the streamed cap now rejects it.
    #[tokio::test]
    async fn adversarial_unbounded_body_without_content_length_is_capped() {
        let url = serve_once(
            "HTTP/1.1 200 OK\r\nConnection: close".into(),
            vec![b'x'; 4096],
        )
        .await;
        let err = read_capped(get(&url).await, 64)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("too large"), "{err}");
    }
}

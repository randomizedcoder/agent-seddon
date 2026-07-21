//! JSON-RPC 2.0 over the LSP `Content-Length` stdio framing. This is the wire
//! codec — pure, hermetic, and the protocol-edge-tested part (hermes'
//! `test_protocol.py`): missing/bad `Content-Length`, truncated body, runaway
//! header, back-to-back frames.

use agent_core::{Error, Result};
use serde_json::{json, Value};

/// Runaway-header guard: reject a header region that never terminates.
const MAX_HEADER: usize = 8 * 1024;
/// Absolute body cap (an attacker-controlled server can't OOM us).
const MAX_BODY: usize = 32 * 1024 * 1024;

/// Encode a JSON-RPC message with its `Content-Length` header frame.
pub fn encode(msg: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(msg).unwrap_or_default();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(&body);
    out
}

/// A JSON-RPC request envelope (has an `id`).
pub fn request(id: i64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// A JSON-RPC notification envelope (no `id`).
pub fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

/// A minimal response to a server-initiated request (used to satisfy
/// `workspace/configuration`, `client/registerCapability`, etc.).
pub fn response(id: i64, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// An incrementally-fed frame decoder: `push` bytes, pull complete messages with
/// `next_message`.
#[derive(Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Pull one complete JSON message: `Ok(Some)` a full message, `Ok(None)` need
    /// more bytes, `Err` a protocol violation.
    pub fn next_message(&mut self) -> Result<Option<Value>> {
        const SEP: &[u8] = b"\r\n\r\n";
        let hdr_end = match find(&self.buf, SEP) {
            Some(i) => i,
            None => {
                if self.buf.len() > MAX_HEADER {
                    return Err(Error::Lsp("protocol: runaway header".into()));
                }
                return Ok(None);
            }
        };
        let header = std::str::from_utf8(&self.buf[..hdr_end])
            .map_err(|_| Error::Lsp("protocol: non-utf8 header".into()))?;
        let len = parse_content_length(header)?;
        if len > MAX_BODY {
            return Err(Error::Lsp("protocol: body exceeds cap".into()));
        }
        let body_start = hdr_end + SEP.len();
        if self.buf.len() < body_start + len {
            return Ok(None); // wait for the full body
        }
        let value: Value = serde_json::from_slice(&self.buf[body_start..body_start + len])
            .map_err(|e| Error::Lsp(format!("protocol: bad JSON body: {e}")))?;
        self.buf.drain(..body_start + len);
        Ok(Some(value))
    }
}

fn parse_content_length(header: &str) -> Result<usize> {
    for line in header.split("\r\n") {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            return rest
                .trim()
                .parse::<usize>()
                .map_err(|_| Error::Lsp("protocol: bad Content-Length".into()));
        }
    }
    Err(Error::Lsp("protocol: missing Content-Length".into()))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A decoded inbound message classified by JSON-RPC shape.
#[derive(Debug, PartialEq)]
pub enum Incoming {
    /// A reply to a request we sent.
    Response {
        id: i64,
        result: std::result::Result<Value, String>,
    },
    /// A server-initiated notification (e.g. `textDocument/publishDiagnostics`).
    Notification { method: String, params: Value },
    /// A server-initiated request we must answer (e.g. `workspace/configuration`).
    ServerRequest {
        id: i64,
        method: String,
        params: Value,
    },
}

/// Classify a decoded message into an [`Incoming`].
pub fn classify(msg: &Value) -> Result<Incoming> {
    let has_id = msg.get("id").map(|v| !v.is_null()).unwrap_or(false);
    let method = msg.get("method").and_then(Value::as_str);
    match (method, has_id) {
        (Some(m), true) => Ok(Incoming::ServerRequest {
            id: msg.get("id").and_then(Value::as_i64).unwrap_or(0),
            method: m.to_string(),
            params: msg.get("params").cloned().unwrap_or(Value::Null),
        }),
        (Some(m), false) => Ok(Incoming::Notification {
            method: m.to_string(),
            params: msg.get("params").cloned().unwrap_or(Value::Null),
        }),
        (None, true) => {
            let id = msg.get("id").and_then(Value::as_i64).unwrap_or(0);
            if let Some(err) = msg.get("error") {
                let m = err
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("error");
                let code = err.get("code").and_then(Value::as_i64).unwrap_or(0);
                Ok(Incoming::Response {
                    id,
                    result: Err(format!("{m} (code {code})")),
                })
            } else {
                Ok(Incoming::Response {
                    id,
                    result: Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                })
            }
        }
        (None, false) => Err(Error::Lsp("protocol: neither id nor method".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // Round-trip: encode then decode yields the original message.
    #[test]
    fn positive_encode_decode_roundtrip() {
        let msg = request(1, "textDocument/hover", json!({"x": 1}));
        let bytes = encode(&msg);
        let mut dec = FrameDecoder::default();
        dec.push(&bytes);
        assert_eq!(dec.next_message().unwrap(), Some(msg));
    }

    // Two frames back-to-back both parse (opencode/hermes framing edge).
    #[test]
    fn corner_two_frames_back_to_back() {
        let mut dec = FrameDecoder::default();
        dec.push(&encode(&notification("a", json!({}))));
        dec.push(&encode(&notification("b", json!({}))));
        assert!(dec.next_message().unwrap().is_some());
        assert!(dec.next_message().unwrap().is_some());
        assert!(dec.next_message().unwrap().is_none());
    }

    // A body split across two pushes waits, then completes.
    #[test]
    fn boundary_partial_body_waits() {
        let bytes = encode(&notification("m", json!({"k": "v"})));
        let (head, tail) = bytes.split_at(bytes.len() - 3);
        let mut dec = FrameDecoder::default();
        dec.push(head);
        assert_eq!(dec.next_message().unwrap(), None); // incomplete
        dec.push(tail);
        assert!(dec.next_message().unwrap().is_some());
    }

    // Protocol violations surface as clean errors (hermes test_protocol).
    #[rstest]
    #[case::missing_content_length(b"Header: 1\r\n\r\n{}".to_vec())]
    #[case::bad_content_length(b"Content-Length: xyz\r\n\r\n{}".to_vec())]
    fn negative_malformed_header_errors(#[case] bytes: Vec<u8>) {
        let mut dec = FrameDecoder::default();
        dec.push(&bytes);
        assert!(dec.next_message().is_err());
    }

    #[test]
    fn negative_bad_json_body_errors() {
        let mut dec = FrameDecoder::default();
        dec.push(b"Content-Length: 3\r\n\r\n{ x");
        assert!(dec.next_message().is_err());
    }

    #[test]
    fn negative_runaway_header_errors() {
        let mut dec = FrameDecoder::default();
        dec.push(&vec![b'x'; 9000]); // no CRLFCRLF within MAX_HEADER
        assert!(dec.next_message().is_err());
    }

    // classify: response / notification / server-request / invalid.
    #[test]
    fn classify_shapes() {
        assert!(matches!(
            classify(&json!({"id": 1, "result": {}})).unwrap(),
            Incoming::Response {
                id: 1,
                result: Ok(_)
            }
        ));
        assert!(matches!(
            classify(&json!({"id": 1, "error": {"code": -32601, "message": "method not found"}}))
                .unwrap(),
            Incoming::Response { result: Err(_), .. }
        ));
        assert!(matches!(
            classify(&json!({"method": "textDocument/publishDiagnostics", "params": {}})).unwrap(),
            Incoming::Notification { .. }
        ));
        assert!(matches!(
            classify(&json!({"id": 2, "method": "workspace/configuration"})).unwrap(),
            Incoming::ServerRequest { .. }
        ));
        assert!(classify(&json!({"jsonrpc": "2.0"})).is_err());
    }
}

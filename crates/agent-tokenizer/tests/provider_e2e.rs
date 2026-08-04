//! End-to-end tests for the `provider` tokenizer backend against a loopback server.
//!
//! No network: a `tiny_http` server on an ephemeral 127.0.0.1 port serves a canned
//! `count_tokens` response (the `agent-forge` / `agent-web` precedent). This
//! exercises the real request-building, auth-header, clamping, and error path.

#![cfg(feature = "tokenizer-provider")]

use agent_core::{Message, Tokenizer};
use agent_tokenizer::ProviderTokenizer;
use std::sync::mpsc;
use tiny_http::{Response, Server};

/// What the mock endpoint should reply with.
#[derive(Clone, Copy)]
enum Reply {
    /// 200 with `{"input_tokens": n}`.
    Ok(u64),
    /// A non-2xx status.
    Status(u16),
}

/// Spawn a loopback `count_tokens` server. Returns its `base_url` and a receiver of
/// the `x-api-key` header seen on each request (to prove the key is sent, and only
/// in the header).
fn spawn(reply: Reply) -> (String, mpsc::Receiver<String>) {
    let (port_tx, port_rx) = mpsc::channel();
    let (key_tx, key_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let server = Server::http("127.0.0.1:0").unwrap();
        port_tx
            .send(server.server_addr().to_ip().unwrap().port())
            .unwrap();
        for request in server.incoming_requests() {
            let key = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("x-api-key"))
                .map(|h| h.value.as_str().to_string())
                .unwrap_or_default();
            let _ = key_tx.send(key);
            let response = match reply {
                Reply::Ok(n) => Response::from_string(format!("{{\"input_tokens\": {n}}}"))
                    .with_status_code(200),
                Reply::Status(code) => {
                    // A body that *echoes* a fake secret — the backend must not leak it.
                    Response::from_string("{\"error\":\"leaked-secret-should-not-appear\"}")
                        .with_status_code(code)
                }
            };
            let _ = request.respond(response);
        }
    });
    let port = port_rx.recv().unwrap();
    (format!("http://127.0.0.1:{port}/v1"), key_rx)
}

#[tokio::test]
async fn count_returns_provider_input_tokens_and_sends_key() {
    let (base, keys) = spawn(Reply::Ok(42));
    let t = ProviderTokenizer::new(&base, "sekret-key", "2023-06-01", 5).unwrap();

    assert_eq!(t.count("hello world", "claude-sonnet-5").await.unwrap(), 42);
    // The key travelled in the x-api-key header.
    assert_eq!(keys.recv().unwrap(), "sekret-key");
}

#[tokio::test]
async fn count_messages_uses_one_call_and_adds_input_tokens() {
    let (base, _keys) = spawn(Reply::Ok(100));
    let t = ProviderTokenizer::new(&base, "k", "2023-06-01", 5).unwrap();
    let msgs = [
        Message::system("be terse"),
        Message::user("hi"),
        Message::assistant("ok"),
    ];
    assert_eq!(
        t.count_messages(&msgs, "claude-sonnet-5").await.unwrap(),
        100
    );
}

#[tokio::test]
async fn hostile_huge_count_is_clamped_not_wrapped() {
    let (base, _keys) = spawn(Reply::Ok(u64::from(u32::MAX) + 1_000)); // > u32::MAX
    let t = ProviderTokenizer::new(&base, "k", "2023-06-01", 5).unwrap();
    assert_eq!(t.count("x", "m").await.unwrap(), u32::MAX);
}

#[tokio::test]
async fn http_error_is_err_and_never_leaks_key_or_body() {
    let (base, _keys) = spawn(Reply::Status(401));
    let t = ProviderTokenizer::new(&base, "sekret-key", "2023-06-01", 5).unwrap();
    let err = t.count("x", "m").await.unwrap_err().to_string();
    assert!(err.contains("401"), "status surfaced: {err}");
    assert!(!err.contains("sekret-key"), "key leaked: {err}");
    assert!(
        !err.contains("leaked-secret"),
        "response body leaked: {err}"
    );
}

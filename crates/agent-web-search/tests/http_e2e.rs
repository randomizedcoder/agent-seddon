//! End-to-end tests for the HTTP backends against a loopback server.
//!
//! No network: a `tiny_http` server on an ephemeral 127.0.0.1 port serves canned
//! provider payloads (the same approach `agent-web` uses). This exercises the
//! real request-building, retry, and parsing path rather than a mock of it.

#![cfg(all(feature = "websearch-brave", feature = "websearch-searxng"))]

use agent_core::{WebQuery, WebSearch};
use agent_web_search::{BraveSearch, HttpSearchConfig, SearxngSearch};
use std::sync::mpsc;
use tiny_http::{Response, Server};

/// Serve a fixed route table on a loopback port; return the base URL.
/// Also records the `X-Subscription-Token` header it saw, so a test can assert
/// the key was actually sent (and, separately, never surfaced to the caller).
fn spawn_server() -> (String, mpsc::Receiver<String>) {
    let (tx, rx) = mpsc::channel();
    let (hdr_tx, hdr_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        tx.send(port).unwrap();
        for request in server.incoming_requests() {
            let url = request.url().to_string();
            let token = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("X-Subscription-Token"))
                .map(|h| h.value.as_str().to_string())
                .unwrap_or_default();
            let _ = hdr_tx.send(token);
            let body = if url.starts_with("/brave") {
                r#"{"web":{"results":[
                    {"url":"https://a.test/1","title":"First","description":"one"},
                    {"url":"https://b.test/2","title":"Second","description":"two"}]}}"#
            } else if url.starts_with("/searxng") {
                r#"{"results":[
                    {"url":"https://c.test/3","title":"Third","content":"three","score":0.8}]}"#
            } else if url.starts_with("/ratelimited") {
                let _ = request.respond(Response::empty(429));
                continue;
            } else {
                "{}"
            };
            let _ = request.respond(Response::from_string(body));
        }
    });
    let port = rx.recv().unwrap();
    (format!("http://127.0.0.1:{port}"), hdr_rx)
}

fn cfg(endpoint: String, key: &str) -> HttpSearchConfig {
    HttpSearchConfig {
        endpoint,
        api_key: key.to_string(),
        timeout_secs: 5,
        max_retries: 0,
    }
}

fn q(text: &str) -> WebQuery {
    WebQuery {
        text: text.into(),
        limit: 5,
        freshness_days: 0,
        backend: None,
    }
}

#[tokio::test]
async fn positive_brave_end_to_end() {
    let (base, headers) = spawn_server();
    let b = BraveSearch::new(cfg(format!("{base}/brave"), "secret-key-value")).unwrap();
    let got = b.search(&q("rust")).await.expect("search succeeds");

    assert_eq!(got.len(), 2);
    assert_eq!(got[0].url, "https://a.test/1");
    assert_eq!(got[0].snippet, "one");

    // The key really was sent…
    assert_eq!(headers.recv().unwrap(), "secret-key-value");
    // …and never appears in anything the caller (and thus the model) can see.
    let rendered = format!("{got:?}");
    assert!(
        !rendered.contains("secret-key-value"),
        "API key leaked into results"
    );
}

#[tokio::test]
async fn positive_searxng_end_to_end() {
    let (base, _h) = spawn_server();
    let s = SearxngSearch::new(cfg(format!("{base}/searxng"), "")).unwrap();
    let got = s.search(&q("rust")).await.expect("search succeeds");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].url, "https://c.test/3");
    assert!((got[0].score - 0.8).abs() < 1e-6, "provider score is used");
}

/// A rate-limited provider must surface an error whose message does NOT echo the
/// response body (which can contain the request, including the key).
#[tokio::test]
async fn negative_rate_limit_error_does_not_leak_the_key() {
    let (base, _h) = spawn_server();
    let b = BraveSearch::new(cfg(format!("{base}/ratelimited"), "secret-key-value")).unwrap();
    let err = b.search(&q("rust")).await.unwrap_err().to_string();
    assert!(err.contains("429"), "expected the status, got: {err}");
    assert!(
        !err.contains("secret-key-value"),
        "API key leaked into the error: {err}"
    );
}

/// An unreachable provider fails cleanly rather than hanging the turn.
#[tokio::test]
async fn negative_unreachable_provider_errors() {
    // Port 1 on loopback: reliably refused, no network involved.
    let b = BraveSearch::new(cfg("http://127.0.0.1:1/brave".into(), "k")).unwrap();
    assert!(b.search(&q("rust")).await.is_err());
}

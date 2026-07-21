//! End-to-end retry proof: a tiny mock HTTP server returns a scripted sequence of
//! responses, and we assert the provider retries transient failures (503 → 200)
//! but does *not* retry a client error (400). Complements the pure unit tests of
//! the retry policy in `src/retry.rs`.
#![cfg(feature = "provider-openai-compat")]

use agent_core::{CompletionRequest, LlmProvider};
use agent_providers::{OpenAiCompatConfig, OpenAiCompatProvider};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serve `responses` (one per incoming connection, in order), counting how many
/// connections were accepted. Each response closes the connection so the client
/// opens a fresh one for the next attempt. Returns `(base_url, connection_count)`.
async fn mock_server(responses: Vec<(u16, &'static str)>) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let count_task = count.clone();

    tokio::spawn(async move {
        for (code, body) in responses {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            count_task.fetch_add(1, Ordering::SeqCst);

            // Drain the request headers (enough to unblock the client's write).
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf).await;

            let reason = if code == 200 { "OK" } else { "ERR" };
            let resp = format!(
                "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
        }
    });

    (format!("http://{addr}/v1"), count)
}

fn provider(base_url: String, max_retries: u32) -> OpenAiCompatProvider {
    OpenAiCompatProvider::new(OpenAiCompatConfig {
        base_url,
        model: "m".into(),
        api_key: "k".into(),
        insecure_tls: false,
        context_window: 1000,
        max_retries,
    })
    .unwrap()
}

fn req() -> CompletionRequest {
    CompletionRequest {
        messages: vec![agent_core::Message::user("hi")],
        tools: vec![],
        max_tokens: 16,
        temperature: 0.0,
        response_format: None,
    }
}

const OK_BODY: &str = r#"{"choices":[{"message":{"content":"hello"},"finish_reason":"stop"}]}"#;

/// A transient 503 is retried, and the subsequent 200 succeeds — one aborted run
/// avoided. Two connections are made (the failed attempt + the retry).
#[tokio::test]
async fn retries_transient_error_then_succeeds() {
    let (base_url, conns) = mock_server(vec![(503, "overloaded"), (200, OK_BODY)]).await;
    let p = provider(base_url, 3);

    let resp = p.complete(req()).await.expect("should succeed after retry");
    assert_eq!(resp.message.content, "hello");
    assert_eq!(conns.load(Ordering::SeqCst), 2, "one retry after the 503");
}

/// A 400 (client error) is the caller's fault — retrying can't fix it — so the
/// provider errors immediately after a single request, no retry.
#[tokio::test]
async fn client_error_is_not_retried() {
    let (base_url, conns) = mock_server(vec![(400, "bad request")]).await;
    let p = provider(base_url, 3);

    let err = p.complete(req()).await.expect_err("400 must not succeed");
    assert!(err.to_string().contains("400"), "err: {err}");
    assert_eq!(conns.load(Ordering::SeqCst), 1, "no retry on a 4xx");
}

/// When every attempt fails, the provider gives up after `max_retries` and
/// surfaces the error rather than looping forever.
#[tokio::test]
async fn exhausts_retries_then_errors() {
    let (base_url, conns) = mock_server(vec![(503, "a"), (503, "b"), (503, "c"), (503, "d")]).await;
    let p = provider(base_url, 2); // 1 initial + 2 retries = 3 attempts

    let err = p.complete(req()).await.expect_err("all attempts 503");
    assert!(err.to_string().contains("503"), "err: {err}");
    assert_eq!(conns.load(Ordering::SeqCst), 3, "initial + 2 retries");
}

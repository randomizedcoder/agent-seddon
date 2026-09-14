//! Adversarial proof that the streaming SSE decode is bounded: a hostile/broken
//! endpoint that never delimits a frame, floods tool-call argument fragments, or opens
//! an unbounded set of tool-call indices must be cut off with an error rather than grow
//! memory without limit (OOM DoS). Complements the pure parse unit tests in the provider
//! modules. See the `stream_caps` module in `src/lib.rs`.
#![cfg(feature = "provider-openai-compat")]

use agent_core::{CompletionRequest, LlmProvider};
use agent_providers::{
    OpenAiCompatConfig, OpenAiCompatProvider, MAX_STREAM_BUF_BYTES, MAX_STREAM_TOOL_ARG_BYTES,
    MAX_STREAM_TOOL_CALLS,
};
use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serve one owned body as a `text/event-stream` response and close. Returns `base_url`.
async fn stream_server(body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf).await;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(&body).await;
            let _ = sock.shutdown().await;
        }
    });
    format!("http://{addr}/v1")
}

fn provider(base_url: String) -> OpenAiCompatProvider {
    OpenAiCompatProvider::new(OpenAiCompatConfig {
        base_url,
        model: "m".into(),
        api_key: "k".into(),
        supports_vision: false,
        insecure_tls: false,
        context_window: 1000,
        max_retries: 0,
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
        route: None,
    }
}

/// Drain the stream, returning whether any chunk was an `Err` (the cap tripped).
async fn stream_errs(p: &OpenAiCompatProvider) -> bool {
    let mut s = p.stream(req()).await.expect("stream opens (headers 200)");
    let mut saw_err = false;
    while let Some(chunk) = s.next().await {
        if chunk.is_err() {
            saw_err = true;
            break;
        }
    }
    saw_err
}

#[tokio::test]
async fn positive_normal_stream_completes_without_error() {
    // A well-formed small stream must complete cleanly (the caps never trip a real stream).
    let body = b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".to_vec();
    let url = stream_server(body).await;
    let p = provider(url);
    assert!(!stream_errs(&p).await, "a normal stream must not error");
}

#[tokio::test]
async fn adversarial_undelimited_frame_is_cut_off() {
    // A body that never contains a newline: the decode buffer would grow forever. Send just
    // over the cap and assert the stream errors instead of buffering unbounded.
    let body = vec![b'x'; MAX_STREAM_BUF_BYTES + 1];
    let url = stream_server(body).await;
    let p = provider(url);
    assert!(
        stream_errs(&p).await,
        "an un-delimited frame past the buffer cap must error, not OOM"
    );
}

#[tokio::test]
async fn adversarial_oversized_tool_args_is_cut_off() {
    // One tool-call delta whose `arguments` fragment alone exceeds the per-call cap.
    let big = "x".repeat(MAX_STREAM_TOOL_ARG_BYTES + 1);
    let frame = format!(
        "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"function\":{{\"arguments\":\"{big}\"}}}}]}}}}]}}\n\n"
    );
    let body = frame.into_bytes();
    let url = stream_server(body).await;
    let p = provider(url);
    assert!(
        stream_errs(&p).await,
        "tool-call arguments past the cap must error, not accumulate unbounded"
    );
}

#[tokio::test]
async fn adversarial_too_many_tool_calls_is_cut_off() {
    // A flood of distinct tool-call indices: each opens a new accumulator slot. Just past
    // the cap must error rather than grow the map without bound.
    let mut body = String::new();
    for i in 0..=MAX_STREAM_TOOL_CALLS as u32 {
        body.push_str(&format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":{i},\"function\":{{\"arguments\":\"a\"}}}}]}}}}]}}\n\n"
        ));
    }
    let body = body.into_bytes();
    let url = stream_server(body).await;
    let p = provider(url);
    assert!(
        stream_errs(&p).await,
        "opening more than the tool-call cap must error, not grow the map unbounded"
    );
}

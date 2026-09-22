//! Adversarial proof that the streaming SSE decode is bounded: a hostile/broken
//! endpoint that never delimits a frame, floods tool-call argument fragments, or opens
//! an unbounded set of tool-call indices must be cut off with an error rather than grow
//! memory without limit (OOM DoS). Complements the pure parse unit tests in the provider
//! modules. See the `stream_caps` module in `src/lib.rs`.
//!
//! Also covers the reasoning-only salvage: a reasoning model that streams
//! `reasoning_content` but ends with empty `content` (and no tool call) must have
//! its reasoning surfaced as the reply rather than dropped (which would stall the
//! agent loop). See `use_reasoning_as_reply` in `openai_compat.rs`.
#![cfg(feature = "provider-openai-compat")]

use agent_core::{CompletionRequest, LlmProvider};
use agent_providers::{
    OpenAiCompatConfig, OpenAiCompatProvider, MAX_STREAM_BUF_BYTES, MAX_STREAM_TEXT_BYTES,
    MAX_STREAM_TOOL_ARG_BYTES, MAX_STREAM_TOOL_CALLS,
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

/// Drain the stream, concatenating every `delta_text`. Panics on any error chunk.
async fn stream_text(p: &OpenAiCompatProvider) -> String {
    let mut s = p.stream(req()).await.expect("stream opens (headers 200)");
    let mut out = String::new();
    while let Some(chunk) = s.next().await {
        out.push_str(&chunk.expect("no error chunk").delta_text);
    }
    out
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
async fn adversarial_slow_drip_text_is_cut_off() {
    // Many well-formed, newline-delimited text frames — each small enough to never trip the
    // per-frame buffer cap — whose *cumulative* content exceeds MAX_STREAM_TEXT_BYTES. This is
    // the slow-drip OOM vector: the buffer cap can't see it, so the cumulative text cap must.
    let per_frame = 1024 * 1024; // 1 MiB of text per frame (< the 8 MiB frame cap)
    let frames = MAX_STREAM_TEXT_BYTES / per_frame + 2; // push just past the total cap
    let chunk = "x".repeat(per_frame);
    let mut body = String::new();
    for _ in 0..frames {
        body.push_str(&format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{chunk}\"}}}}]}}\n\n"
        ));
    }
    let url = stream_server(body.into_bytes()).await;
    let p = provider(url);
    assert!(
        stream_errs(&p).await,
        "cumulative text past the cap must error, not accumulate unbounded"
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

// --- reasoning-only salvage (streaming) -----------------------------------------

#[tokio::test]
async fn positive_reasoning_only_stream_is_salvaged() {
    // finish=stop with only `reasoning_content` and empty `content`, no tool call:
    // the reasoning is the sole output and must be surfaced as the reply.
    let body =
        b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"the answer is 42\"}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
                 data: [DONE]\n\n"
            .to_vec();
    let url = stream_server(body).await;
    let p = provider(url);
    assert_eq!(stream_text(&p).await, "the answer is 42");
}

#[tokio::test]
async fn negative_content_stream_does_not_append_reasoning() {
    // Both reasoning and content present: only `content` is the reply — reasoning is
    // never resent when there is real content.
    let body = b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking...\"}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":\"stop\"}]}\n\n\
                 data: [DONE]\n\n"
        .to_vec();
    let url = stream_server(body).await;
    let p = provider(url);
    assert_eq!(stream_text(&p).await, "hello");
}

#[tokio::test]
async fn corner_tool_call_stream_suppresses_reasoning_salvage() {
    // Empty content + reasoning + a tool call: the tool call is the turn's action, so
    // the reasoning must NOT leak out as assistant text.
    let body = b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"i should call a tool\",\
                 \"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"ls\",\"arguments\":\"{}\"}}]}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n\
                 data: [DONE]\n\n"
        .to_vec();
    let url = stream_server(body).await;
    let p = provider(url);
    assert_eq!(
        stream_text(&p).await,
        "",
        "reasoning must not leak as text when a tool call is present"
    );
}

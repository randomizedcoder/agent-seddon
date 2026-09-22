//! End-to-end streaming decode for the Anthropic provider, focused on the
//! thinking-only salvage (round7-1). A reasoning model can stream a `thinking` block
//! and stop with no text and no `tool_use`; that reasoning must be surfaced as the
//! reply rather than dropped (which would decode to an empty assistant message and
//! stall the agent loop / trip the non-convergence guard #405). This is the Anthropic
//! parallel of the openai-compat #442 fix — see `crate::use_reasoning_as_reply`.
#![cfg(feature = "provider-anthropic")]

use agent_core::{CompletionRequest, LlmProvider};
use agent_providers::{AnthropicConfig, AnthropicProvider};
use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serve one owned body as a `text/event-stream` response and close. Returns `base_url`
/// (the provider appends `/messages`).
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
    format!("http://{addr}")
}

fn provider(base_url: String) -> AnthropicProvider {
    AnthropicProvider::new(AnthropicConfig {
        base_url,
        model: "claude-x".into(),
        api_key: "k".into(),
        version: "2023-06-01".into(),
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

/// Drain the stream, concatenating every `delta_text`. Panics on any error chunk.
async fn stream_text(p: &AnthropicProvider) -> String {
    let mut s = p.stream(req()).await.expect("stream opens (headers 200)");
    let mut out = String::new();
    while let Some(chunk) = s.next().await {
        out.push_str(&chunk.expect("no error chunk").delta_text);
    }
    out
}

#[tokio::test]
async fn positive_thinking_only_stream_is_salvaged() {
    // A thinking block streams, then the turn stops with no text and no tool_use: the
    // reasoning is the sole output and must be surfaced as the reply.
    let body = b"data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":5}}}\n\n\
                 data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\"}}\n\n\
                 data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"the answer is 42\"}}\n\n\
                 data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
                 data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n\
                 data: {\"type\":\"message_stop\"}\n\n"
        .to_vec();
    let url = stream_server(body).await;
    let p = provider(url);
    assert_eq!(stream_text(&p).await, "the answer is 42");
}

#[tokio::test]
async fn negative_content_stream_does_not_append_thinking() {
    // Both thinking and text stream: only the text is the reply — thinking is never
    // resent when there is real assistant text.
    let body = b"data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"scratch\"}}\n\n\
                 data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n\
                 data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n\
                 data: {\"type\":\"message_stop\"}\n\n"
        .to_vec();
    let url = stream_server(body).await;
    let p = provider(url);
    assert_eq!(stream_text(&p).await, "hello");
}

#[tokio::test]
async fn corner_tool_use_stream_suppresses_thinking_salvage() {
    // Thinking streams, then a tool_use block opens: the tool call is the turn's action,
    // so the reasoning must NOT leak out as assistant text.
    let body = b"data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"i should call a tool\"}}\n\n\
                 data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"c1\",\"name\":\"ls\"}}\n\n\
                 data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n\
                 data: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
                 data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n\
                 data: {\"type\":\"message_stop\"}\n\n"
        .to_vec();
    let url = stream_server(body).await;
    let p = provider(url);
    assert_eq!(
        stream_text(&p).await,
        "",
        "thinking must not leak as text when a tool_use block is present"
    );
}

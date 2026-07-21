//! An OpenAI-compatible `/chat/completions` provider.
//!
//! Tested against a GLM-5.2 server. GLM is a reasoning model: it emits a
//! `reasoning_content` field (which we log for debugging but do NOT resend, per
//! OpenAI convention) and only fills `content` once reasoning is done — so
//! `max_tokens` needs real headroom.

use agent_core::{
    ChunkStream, CompletionChunk, CompletionRequest, CompletionResponse, ContentBlock, Error,
    LlmProvider, Message, ModelCapabilities, Result, Role, ToolCall, Usage,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

pub struct OpenAiCompatConfig {
    /// e.g. `https://host:8000/v1`
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub insecure_tls: bool,
    pub context_window: u32,
    /// Retries for transient failures (429 / 5xx / timeout); 0 disables.
    pub max_retries: u32,
    /// Whether the configured model accepts image blocks. This endpoint is
    /// generic (any OpenAI-compatible server, including local text-only models),
    /// so it **defaults off** and is opted into per deployment — sending an image
    /// to a model that cannot take one fails the entire request.
    pub supports_vision: bool,
}

pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    api_key: String,
    context_window: u32,
    supports_vision: bool,
    /// Prompt-cache placement policy (parity spec 24). `None` ⇒ no cache key.
    cache: Option<Arc<dyn agent_core::CacheStrategy>>,
    retry: agent_retry::RetryPolicy,
}

impl OpenAiCompatProvider {
    pub fn new(cfg: OpenAiCompatConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(cfg.insecure_tls)
            .timeout(Duration::from_secs(600)) // reasoning models can be slow
            .build()
            .map_err(|e| Error::Provider(format!("building http client: {e}")))?;
        let endpoint = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
        Ok(Self {
            client,
            endpoint,
            model: cfg.model,
            api_key: cfg.api_key,
            context_window: cfg.context_window,
            supports_vision: cfg.supports_vision,
            cache: None,
            retry: agent_retry::RetryPolicy::new(cfg.max_retries),
        })
    }

    /// Attach the prompt-cache placement strategy.
    pub fn with_cache_strategy(mut self, s: Arc<dyn agent_core::CacheStrategy>) -> Self {
        self.cache = Some(s);
        self
    }

    /// POST the request body via the canonical retry driver (shared by
    /// complete/stream): retry 429/5xx (honouring `Retry-After`) and connection/
    /// timeout errors, fail fast on other 4xx. Returns the final response for the
    /// caller to read/stream (a non-retryable error status is returned as `Ok` so
    /// the caller can read its body for the message).
    async fn send(&self, wire: &WireReq<'_>) -> Result<reqwest::Response> {
        agent_retry::run(&self.retry, || async {
            match self
                .client
                .post(&self.endpoint)
                .bearer_auth(&self.api_key)
                .json(wire)
                .send()
                .await
            {
                Ok(resp) => {
                    let code = resp.status().as_u16();
                    if agent_retry::http::retryable_status(code) {
                        let after = resp
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|v| v.to_str().ok())
                            .and_then(agent_retry::http::parse_retry_after);
                        let body = resp.text().await.unwrap_or_default();
                        agent_retry::Attempt::Retry {
                            err: Error::Provider(format!("http {code}: {body}")),
                            after,
                        }
                    } else {
                        agent_retry::Attempt::Done(resp)
                    }
                }
                Err(e) if e.is_timeout() || e.is_connect() => agent_retry::Attempt::Retry {
                    err: Error::Provider(format!("request failed: {e}")),
                    after: None,
                },
                Err(e) => {
                    agent_retry::Attempt::Fail(Error::Provider(format!("request failed: {e}")))
                }
            }
        })
        .await
    }

    fn build_wire<'a>(&'a self, req: &CompletionRequest, stream: bool) -> WireReq<'a> {
        WireReq {
            model: &self.model,
            messages: req.messages.iter().map(WireMsg::from_core).collect(),
            tools: req
                .tools
                .iter()
                .map(|t| WireTool {
                    typ: "function",
                    function: WireFn {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: t.parameters.clone(),
                    },
                })
                .collect(),
            max_tokens: req.max_tokens,
            temperature: req.temperature,
            stream,
            prompt_cache_key: self.cache_key(req),
        }
    }

    /// A stable cache key from the configured strategy, if any. This family
    /// caches prefixes automatically; the key only steers routing affinity.
    fn cache_key(&self, req: &CompletionRequest) -> Option<String> {
        let strategy = self.cache.as_ref()?;
        let shape = agent_core::PromptShape::new(true, req.tools.len(), &req.messages);
        strategy
            .place(&shape, &agent_core::CacheCapabilities::openai())
            .cache_key
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_tools: true,
            context_window: self.context_window,
            // Native `response_format` serialization is a documented follow-up; the
            // structured helper prompt-injects the schema until then (parity spec 16).
            supports_response_format: false,
            supports_vision: self.supports_vision,
        }
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let wire = self.build_wire(&req, false);

        let resp = self.send(&wire).await?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| Error::Provider(format!("reading body: {e}")))?;

        if !status.is_success() {
            return Err(Error::Provider(format!("http {status}: {body}")));
        }

        let parsed: WireResp = serde_json::from_str(&body)
            .map_err(|e| Error::Provider(format!("decoding response: {e}; body={body}")))?;

        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| Error::Provider("no choices in response".into()))?;

        if let Some(reasoning) = &choice.message.reasoning_content {
            if !reasoning.is_empty() {
                tracing::debug!(
                    chars = reasoning.len(),
                    "model reasoning_content (not resent)"
                );
            }
        }

        let tool_calls = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tc| ToolCall {
                id: tc.id,
                name: tc.function.name,
                arguments: parse_tool_args(&tc.function.arguments),
            })
            .collect();

        let text = choice.message.content.unwrap_or_default();
        let message = Message {
            role: Role::Assistant,
            // This API returns assistant text only (images come back via separate
            // modalities), so a single text block is the faithful decode.
            content: if text.is_empty() {
                Vec::new()
            } else {
                vec![ContentBlock::text(text)]
            },
            tool_calls,
            tool_call_id: None,
        };

        Ok(CompletionResponse {
            message,
            finish_reason: choice.finish_reason.unwrap_or_else(|| "stop".into()),
            usage: parsed.usage.map(|u| Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
                // OpenAI reports prompt-cache hits under `prompt_tokens_details.
                // cached_tokens`; there is no separate cache-write line (writes are
                // billed as normal input), so `cache_write_tokens` stays 0.
                cache_read_tokens: u.prompt_tokens_details.cached_tokens,
                cache_write_tokens: 0,
                cost: None,
            }),
        })
    }

    async fn stream(&self, req: CompletionRequest) -> Result<ChunkStream> {
        let wire = self.build_wire(&req, true);
        let resp = self.send(&wire).await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Provider(format!("http {status}: {body}")));
        }

        // Text streams directly; tool-call `arguments` arrive as fragments keyed
        // by `index` and are finalized when the stream ends. (Usage is not
        // requested in stream mode; use `stream=false` for token counts.)
        let stream = async_stream::stream! {
            let mut bytes = resp.bytes_stream();
            let mut buf = String::new();
            let mut tools_acc: BTreeMap<u32, ToolAcc> = BTreeMap::new();
            let mut finish: Option<String> = None;

            'read: while let Some(next) = bytes.next().await {
                let b = match next {
                    Ok(b) => b,
                    Err(e) => {
                        yield Err(Error::Provider(format!("stream error: {e}")));
                        break;
                    }
                };
                buf.push_str(&String::from_utf8_lossy(&b));
                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].trim_end_matches('\r').to_string();
                    buf.drain(..pos + 1);
                    let Some(data) = line.strip_prefix("data:") else { continue };
                    let data = data.trim();
                    if data.is_empty() {
                        continue;
                    }
                    if data == "[DONE]" {
                        break 'read;
                    }
                    let ev: StreamEvent = match serde_json::from_str(data) {
                        Ok(ev) => ev,
                        Err(_) => continue,
                    };
                    for choice in ev.choices {
                        if let Some(delta) = choice.delta {
                            if let Some(text) = delta.content {
                                if !text.is_empty() {
                                    yield Ok(CompletionChunk { delta_text: text, ..Default::default() });
                                }
                            }
                            for tc in delta.tool_calls.unwrap_or_default() {
                                let acc = tools_acc.entry(tc.index).or_default();
                                if let Some(id) = tc.id {
                                    if !id.is_empty() {
                                        acc.id = id;
                                    }
                                }
                                if let Some(f) = tc.function {
                                    if let Some(n) = f.name {
                                        if !n.is_empty() {
                                            acc.name = n;
                                        }
                                    }
                                    if let Some(a) = f.arguments {
                                        acc.args.push_str(&a);
                                    }
                                }
                            }
                        }
                        if let Some(fr) = choice.finish_reason {
                            finish = Some(fr);
                        }
                    }
                }
            }

            for (_i, acc) in tools_acc {
                yield Ok(CompletionChunk { tool_call: Some(acc.into_tool_call()), ..Default::default() });
            }
            yield Ok(CompletionChunk {
                finish_reason: Some(finish.unwrap_or_else(|| "stop".into())),
                ..Default::default()
            });
        };
        Ok(Box::pin(stream))
    }
}

/// A streamed tool call being assembled from `arguments` fragments.
#[derive(Default)]
struct ToolAcc {
    id: String,
    name: String,
    args: String,
}

impl ToolAcc {
    fn into_tool_call(self) -> ToolCall {
        ToolCall {
            arguments: parse_tool_args(&self.args),
            id: self.id,
            name: self.name,
        }
    }
}

/// Parse a tool call's `arguments`, which OpenAI-compatible APIs send as a JSON
/// *string*. Empty/whitespace ⇒ empty object; unparseable ⇒ keep the raw text as a
/// string (so the model at least sees what it sent). Shared by the buffered and
/// streaming paths.
fn parse_tool_args(raw: &str) -> Value {
    if raw.trim().is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
    }
}

// --- wire types (streaming) -----------------------------------------------

#[derive(Deserialize)]
struct StreamEvent {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Option<StreamDelta>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Deserialize)]
struct StreamToolCall {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamFn>,
}

#[derive(Deserialize)]
struct StreamFn {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

// --- wire types -----------------------------------------------------------

#[derive(Serialize)]
struct WireReq<'a> {
    model: &'a str,
    messages: Vec<WireMsg>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<WireTool>,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
    /// Routing affinity for the provider's automatic prefix cache (parity spec
    /// 24). Omitted when no strategy is wired, so the body is unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_key: Option<String>,
}

/// Render message content for the wire. A text-only message stays a plain
/// string so nothing changes for the (overwhelmingly common) text case and
/// text-only servers keep working; anything with media becomes the parts array.
fn to_openai_content(blocks: &[ContentBlock]) -> WireContent {
    if !blocks.iter().any(ContentBlock::is_media) {
        let mut s = String::new();
        for b in blocks {
            if let Some(t) = b.as_text() {
                s.push_str(t);
            }
        }
        return WireContent::Text(s);
    }
    WireContent::Parts(
        blocks
            .iter()
            .map(|b| match b {
                ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
                ContentBlock::Image { media_type, data } => json!({
                    "type": "image_url",
                    "image_url": {
                        "url": format!(
                            "data:{media_type};base64,{}",
                            agent_core::b64_encode(data)
                        ),
                    },
                }),
                // This API has no inline document part; describe it instead of
                // dropping it silently.
                ContentBlock::Document {
                    media_type,
                    data,
                    name,
                } => json!({
                    "type": "text",
                    "text": format!(
                        "[{}: {media_type}, {} bytes — not inlinable]",
                        name.as_deref().unwrap_or("attachment"),
                        data.len()
                    ),
                }),
            })
            .collect(),
    )
}

/// This API accepts `content` as either a bare string or an array of typed
/// parts. Text-only turns keep the string form (what every existing deployment
/// and every non-vision server expects); a turn carrying media uses the array.
#[derive(Serialize)]
#[serde(untagged)]
enum WireContent {
    Text(String),
    Parts(Vec<Value>),
}

#[derive(Serialize)]
struct WireMsg {
    role: &'static str,
    content: WireContent,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<WireToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl WireMsg {
    fn from_core(m: &Message) -> Self {
        WireMsg {
            role: m.role.as_str(),
            content: to_openai_content(&m.content),
            tool_calls: m
                .tool_calls
                .iter()
                .map(|tc| WireToolCall {
                    id: tc.id.clone(),
                    typ: "function",
                    function: WireToolCallFn {
                        name: tc.name.clone(),
                        arguments: serde_json::to_string(&tc.arguments)
                            .unwrap_or_else(|_| "{}".into()),
                    },
                })
                .collect(),
            tool_call_id: m.tool_call_id.clone(),
        }
    }
}

#[derive(Serialize)]
struct WireTool {
    #[serde(rename = "type")]
    typ: &'static str,
    function: WireFn,
}

#[derive(Serialize)]
struct WireFn {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Serialize)]
struct WireToolCall {
    id: String,
    #[serde(rename = "type")]
    typ: &'static str,
    function: WireToolCallFn,
}

#[derive(Serialize)]
struct WireToolCallFn {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct WireResp {
    choices: Vec<WireChoice>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireChoice {
    message: WireRespMsg,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct WireRespMsg {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<WireRespToolCall>>,
}

#[derive(Deserialize)]
struct WireRespToolCall {
    id: String,
    function: WireRespFn,
}

#[derive(Deserialize)]
struct WireRespFn {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
    #[serde(default)]
    prompt_tokens_details: PromptTokensDetails,
}

/// The `usage.prompt_tokens_details` sub-object (OpenAI + compatible gateways);
/// `cached_tokens` is the prompt-cache hit count. Defaulted so providers that omit
/// it (vLLM, Ollama, older OpenAI) parse to zero.
#[derive(Deserialize, Default)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::{parse_tool_args, to_openai_content, WireContent};
    use agent_core::ContentBlock;
    use rstest::rstest;
    use serde_json::{json, Value};

    /// The 67-byte minimal 1x1 PNG (deterministic fixture, no assets).
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// A text-only turn must stay a bare string on the wire — text-only servers
    /// (and every pre-spec-26 deployment) expect exactly that.
    #[test]
    fn positive_text_only_stays_a_bare_string() {
        let out = to_openai_content(&[ContentBlock::text("a"), ContentBlock::text("b")]);
        match serde_json::to_value(&out).unwrap() {
            Value::String(s) => assert_eq!(s, "ab"),
            other => panic!("expected a bare string, got {other}"),
        }
        assert!(matches!(out, WireContent::Text(_)));
    }

    /// An image becomes a `image_url` part carrying a data URI.
    #[test]
    fn positive_image_becomes_image_url_data_uri() {
        let out = to_openai_content(&[
            ContentBlock::text("look"),
            ContentBlock::image("image/png", TINY_PNG),
        ]);
        let v = serde_json::to_value(&out).unwrap();
        let parts = v.as_array().expect("parts array");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[1]["type"], "image_url");
        let url = parts[1]["image_url"]["url"].as_str().unwrap();
        let b64 = url
            .strip_prefix("data:image/png;base64,")
            .expect("data URI prefix");
        assert_eq!(agent_core::b64_decode(b64).unwrap(), TINY_PNG);
    }

    /// OpenAI sends tool-call `arguments` as a JSON string.
    #[rstest]
    #[case::positive_object("{\"a\":1}", json!({"a":1}))]
    #[case::positive_nested("{\"a\":{\"b\":[1,2]}}", json!({"a":{"b":[1,2]}}))]
    #[case::boundary_empty("", json!({}))]
    #[case::boundary_whitespace("   \n", json!({}))]
    #[case::negative_invalid_kept_as_string("not json", Value::String("not json".into()))]
    #[case::corner_partial_json("{oops", Value::String("{oops".into()))]
    fn parse_tool_args_cases(#[case] raw: &str, #[case] expected: Value) {
        assert_eq!(parse_tool_args(raw), expected);
    }
}
